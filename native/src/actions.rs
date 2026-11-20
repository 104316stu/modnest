//! Everything the buttons do (a port of the click handlers in ui/app.js).
use crate::app::{Menu, ModNest, Tab, View};
use crate::bridge;
use crate::dialogs::{Confirm, Modal};
use crate::game::Browse;
use crate::widgets::*;
use gpui::*;
use modnest_core as mn;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

pub type Then<T> = Box<dyn FnOnce(&mut ModNest, T, &mut Window, &mut Context<ModNest>) + 'static>;

/// Engine result as JSON (the same shape the WebView UI got).
pub fn json_of<T: Serialize>(r: Result<T, String>) -> Result<Value, String> {
    r.map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
}

impl ModNest {
    /// Runs an engine call in the background, then `then` on the UI; errors become a toast.
    pub fn exec<T, F>(&mut self, window: &mut Window, cx: &mut Context<Self>, fut: F, then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static)
    where
        T: Send + 'static,
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        self.exec_or(window, cx, fut, then, |this, e, _, cx| {
            this.toast_error(&e);
            cx.notify();
        })
    }

    pub fn exec_or<T, F>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        fut: F,
        then: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        fail: impl FnOnce(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        cx.spawn_in(window, async move |this, cx| {
            let r = bridge::run(fut).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match r {
                    Ok(v) => then(this, v, window, cx),
                    Err(e) => fail(this, e, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn core(&self) -> Arc<mn::Core> {
        self.core.clone()
    }

    fn gid(&self) -> String {
        self.game_id.clone().unwrap_or_default()
    }

    /// Takes a new game state from the engine (ignored if another game is open now).
    pub fn set_state(&mut self, st: Value, cx: &mut Context<Self>) {
        let id = s(&st["game"], "id");
        if Some(&id) != self.game_id.as_ref() {
            return;
        }
        if let Some(g) = self.games.iter_mut().find(|g| s(g, "id") == id) {
            if let (Some(dst), Some(src)) = (g.as_object_mut(), st["game"].as_object()) {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
            }
        }
        self.state = Some(st);
        cx.notify();
    }

    /// Runs an engine call that returns a game state, then shows it.
    fn state_call<F>(&mut self, window: &mut Window, cx: &mut Context<Self>, fut: F)
    where
        F: Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.exec(window, cx, fut, |this, st, _, cx| this.set_state(st, cx));
    }

    /// Before changing mod files: if the game is running, ask to close it first (like readyToChange()).
    pub fn ready_then(&mut self, game_id: String, window: &mut Window, cx: &mut Context<Self>, go: Then<()>) {
        let core = self.core();
        let gid = game_id.clone();
        self.exec_or(
            window,
            cx,
            async move { mn::game_running(&core, gid) },
            move |this, running: Option<String>, window, cx| {
                let Some(exe) = running else { return go(this, (), window, cx) };
                let name = this.games.iter().find(|g| s(g, "id") == game_id).map(|g| s(g, "name")).unwrap_or_else(|| "The game".into());
                this.modal = Some(Modal::Confirm(Confirm {
                    title: format!("{name} is running"),
                    body: vec!["Mods can't change while the game is open.".into()],
                    small: Some(format!("Unsaved progress in {exe} will be lost.")),
                    ok: "Close game and continue".into(),
                    on_ok: Some(Box::new(move |this, _, window, cx| {
                        this.toast("close", format!("Closing {name}…"), "", "", Some(-1.0), 0);
                        let core = this.core();
                        this.exec(
                            window,
                            cx,
                            async move { mn::close_game(&core, game_id).await },
                            move |this, _, window, cx| {
                                this.dismiss("close");
                                go(this, (), window, cx)
                            },
                        );
                    })),
                }));
                cx.notify();
            },
            // If the check itself fails, carry on like the WebView did.
            |this, _, window, cx| {
                let _ = (this, window, cx);
            },
        );
    }

    // ---------- navigation ----------

    pub fn open_game(&mut self, id: String, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.open_game_then(id, tab, window, cx, None);
    }

    pub fn open_game_then(&mut self, id: String, tab: Tab, window: &mut Window, cx: &mut Context<Self>, then: Option<Then<()>>) {
        self.menu = None;
        self.view = View::Game;
        self.game_id = Some(id.clone());
        self.state = None;
        self.browse = None;
        self.tab = tab;
        self.open.clear();
        self.inst_filter.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
        let core = self.core();
        let gid = id.clone();
        self.exec_or(
            window,
            cx,
            async move {
                let st = mn::game_state(&core, gid.clone()).await?;
                let v = serde_json::to_value(&st).unwrap_or(Value::Null);
                // Every game starts with a "Default" profile holding whatever is installed now.
                if arr(&v, "profiles").is_empty() && b(&v["game"], "installed") {
                    return json_of(mn::save_profile(&core, gid, "Default".into()).await);
                }
                Ok(v)
            },
            move |this, st: Value, window, cx| {
                if this.game_id.as_ref() != Some(&id) {
                    return;
                }
                this.state = Some(st);
                this.on_tab_shown(window, cx);
                this.auto_maintain(id.clone(), window, cx);
                if this.mgr_all.is_none() {
                    let core = this.core();
                    this.exec(window, cx, async move { Ok(mn::manager_profiles(&core)) }, |this, l, _, cx| {
                        this.mgr_all = Some(l);
                        cx.notify();
                    });
                }
                if let Some(f) = then {
                    f(this, (), window, cx);
                }
            },
            |this, e, _, cx| {
                this.toast_error(&e);
                this.show_library(cx);
            },
        );
    }

    pub fn show_mine(&mut self, cx: &mut Context<Self>) {
        self.view = View::Mine;
        self.state = None;
        self.menu = None;
        self.mine = None;
        cx.notify();
        let core = self.core();
        cx.spawn(async move |this, cx| {
            let r = bridge::run(async move { (mn::my_stuff(&core), mn::manager_profiles(&core)) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.view == View::Mine {
                    this.mine = Some(r);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.tab = tab;
        self.on_tab_shown(window, cx);
        cx.notify();
    }

    pub fn toggle_menu(&mut self, m: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(m) { None } else { Some(m) };
        cx.notify();
    }

    pub fn open_url(&mut self, url: String, _cx: &mut Context<Self>) {
        let _ = mn::open_url(&self.core, url);
    }

    // ---------- browse ----------

    fn current_source(&self) -> Option<String> {
        let st = self.state.as_ref()?;
        let srcs: Vec<String> = arr(&st["game"], "sources").iter().filter_map(|x| x.as_str().map(String::from)).collect();
        let picked = self.game_id.as_ref().and_then(|g| self.source_for.get(g)).cloned();
        match picked {
            Some(p) if srcs.contains(&p) => Some(p),
            _ => srcs.first().cloned(),
        }
    }

    /// When Browse / Modpacks shows: make sure there's a list for this tab and source, and load it.
    pub fn on_tab_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.tab, Tab::Browse | Tab::Modpacks) {
            return;
        }
        let Some(source) = self.current_source() else {
            self.browse = None;
            return;
        };
        let fresh = match &self.browse {
            Some(b) => b.tab != self.tab || b.source != source,
            None => true,
        };
        if fresh {
            let generation = self.browse.as_ref().map(|b| b.generation + 1).unwrap_or(0);
            self.browse = Some(Browse { tab: self.tab, source, q: String::new(), ordering: "most-downloaded".into(), category: String::new(), page: 1, items: vec![], total: 0, has_more: false, loading: false, loaded: false, error: String::new(), generation });
            let ph = format!("Search {} {}", crate::game::source_name(&self.browse.as_ref().map(|b| b.source.clone()).unwrap_or_default()), if self.tab == Tab::Modpacks { "modpacks" } else { "mods" });
            self.browse_q.update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.set_placeholder(ph, window, cx);
            });
            self.load_browse(true, cx);
        }
    }

    pub fn load_browse(&mut self, reset: bool, cx: &mut Context<Self>) {
        let Some(bw) = self.browse.as_mut() else { return };
        if reset {
            bw.page = 1;
            bw.items.clear();
            bw.error.clear();
            bw.generation += 1;
        }
        bw.loading = true;
        let (gen, source, q, page, ordering, category, tab) = (bw.generation, bw.source.clone(), bw.q.clone(), bw.page, bw.ordering.clone(), bw.category.clone(), bw.tab);
        let gid = self.gid();
        let core = self.core();
        let fkey = format!("{gid}|{source}");
        let cached = self.filters.get(&fkey).cloned();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let r = bridge::run(async move {
                let filters = match cached {
                    Some(f) => f,
                    None => mn::filters(&core, gid.clone(), source.clone()).await.unwrap_or_else(|_| json!({"sections": [], "categories": []})),
                };
                let slug = if tab == Tab::Modpacks { "modpacks" } else { "mods" };
                let section = if source == "own" { Some(slug.to_string()) } else { arr(&filters, "sections").iter().find(|s| s["slug"] == slug).and_then(|s| s.get("id")).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())) };
                if tab == Tab::Modpacks && source != "own" && section.is_none() {
                    return (filters, Err(format!("{} has no modpacks for this game.", crate::game::source_name(&source))));
                }
                let cats = if category.is_empty() { vec![] } else { vec![category] };
                let r = mn::browse(&core, gid, source, q, page, ordering, section, cats).await;
                (filters, r)
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let (filters, r) = r;
                this.filters.insert(fkey, filters);
                let Some(bw) = this.browse.as_mut() else { return };
                if bw.generation != gen {
                    return;
                }
                match r {
                    Ok(v) => {
                        bw.items.extend(arr(&v, "items").iter().cloned());
                        bw.has_more = b(&v, "hasMore");
                        bw.total = n(&v, "total");
                    }
                    Err(e) => bw.error = e,
                }
                bw.loading = false;
                bw.loaded = true;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn search_changed(&mut self, cx: &mut Context<Self>) {
        let q = self.browse_q.read(cx).value().trim().to_string();
        let Some(bw) = self.browse.as_mut() else { return };
        if bw.q == q {
            return;
        }
        bw.q = q;
        bw.generation += 1;
        let gen = bw.generation;
        // Wait for typing to pause (like the WebView's 300 ms debounce).
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
            let _ = this.update(cx, |this, cx| {
                if this.browse.as_ref().map(|b| b.generation) == Some(gen) {
                    this.load_browse(true, cx);
                }
            });
        })
        .detach();
    }

    pub fn pick_source(&mut self, v: String, cx: &mut Context<Self>) {
        if let Some(g) = self.game_id.clone() {
            self.source_for.insert(g, v);
        }
        self.browse = None;
        // on_tab_shown needs a window for the search box; reset it on the next frame.
        cx.notify();
        self.pending_tab_shown = true;
    }

    pub fn pick_select(&mut self, menu: Menu, v: String, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if let Some(bw) = self.browse.as_mut() {
            if menu == Menu::Sort {
                bw.ordering = v;
            } else {
                bw.category = v;
            }
        }
        self.load_browse(true, cx);
    }

    pub fn load_more(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bw) = self.browse.as_mut() {
            bw.page += 1;
        }
        self.load_browse(false, cx);
    }

    // ---------- mods ----------

    pub fn install(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                this.busy.insert(key.clone());
                let core = this.core();
                let (k1, k2) = (key.clone(), key.clone());
                this.exec_or(
                    window,
                    cx,
                    async move { json_of(mn::install(&core, gid, k1).await) },
                    move |this, st: Value, _, cx| {
                        this.busy.remove(&k2);
                        this.mark_installed(&st);
                        this.set_state(st, cx);
                    },
                    move |this, e, _, _| {
                        this.busy.remove(&key);
                        this.toast_error(&e);
                    },
                );
            }),
        );
    }

    fn mark_installed(&mut self, st: &Value) {
        let have: HashSet<String> = arr(st, "mods").iter().map(|m| s(m, "key")).collect();
        if let Some(bw) = self.browse.as_mut() {
            for i in bw.items.iter_mut() {
                let k = s(i, "key");
                i["installed"] = Value::Bool(have.contains(&k));
            }
        }
    }

    pub fn install_pack(&mut self, key: String, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                this.busy.insert(key.clone());
                this.batch = Some((title.clone(), HashSet::new()));
                let core = this.core();
                let (k1, k2) = (key.clone(), key.clone());
                this.exec_or(
                    window,
                    cx,
                    async move { json_of(mn::install_modpack(&core, gid, k1, title).await) },
                    move |this, r: Value, _, cx| {
                        this.dismiss("batch");
                        this.batch = None;
                        this.busy.remove(&k2);
                        let failed: Vec<String> = arr(&r, "failed").iter().filter_map(|x| x.as_str().map(String::from)).collect();
                        if !failed.is_empty() {
                            this.toast("mpf", format!("{} couldn't be installed", plural(failed.len(), "mod")), failed.join("\n"), "error", None, 12000);
                        }
                        let st = r["state"].clone();
                        this.toast("mp", format!("Modpack ready in profile “{}”", s(&st["game"], "activeProfile")), "", "ok", None, 4000);
                        this.mark_installed(&st);
                        this.set_state(st, cx);
                    },
                    move |this, e, _, _| {
                        this.dismiss("batch");
                        this.batch = None;
                        this.busy.remove(&key);
                        this.toast_error(&e);
                    },
                );
            }),
        );
    }

    pub fn toggle_mod(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        let enabled = !self.state.as_ref().and_then(|st| arr(st, "mods").iter().find(|m| s(m, "key") == key).map(|m| b(m, "enabled"))).unwrap_or(false);
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                let core = this.core();
                this.state_call(window, cx, async move { json_of(mn::set_enabled(&core, gid, key, enabled).await) });
            }),
        );
    }

    pub fn update_one(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        self.update_keys(vec![key], window, cx);
    }

    pub fn update_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.state.as_ref().map(|st| arr(st, "mods").iter().filter(|m| b(m, "updateAvailable")).map(|m| s(m, "key")).collect()).unwrap_or_default();
        self.update_keys(keys, window, cx);
    }

    fn update_keys(&mut self, keys: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                let core = this.core();
                this.state_call(window, cx, async move { json_of(mn::update_mods(&core, gid, keys).await) });
            }),
        );
    }

    pub fn remove_dialog(&mut self, key: String, _window: &mut Window, cx: &mut Context<Self>) {
        let title = self.state.as_ref().and_then(|st| arr(st, "mods").iter().find(|m| s(m, "key") == key).map(|m| s(m, "title"))).unwrap_or_else(|| key.clone());
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Uninstall mod".into(),
            body: vec![format!("Uninstall **{title}**?")],
            small: None,
            ok: "Uninstall".into(),
            on_ok: Some(Box::new(move |this, _, window, cx| {
                let gid = this.gid();
                this.ready_then(
                    gid.clone(),
                    window,
                    cx,
                    Box::new(move |this, _, window, cx| {
                        let core = this.core();
                        this.exec(window, cx, async move { json_of(mn::uninstall(&core, gid, key).await) }, |this, st: Value, _, cx| {
                            this.mark_installed(&st);
                            this.set_state(st, cx);
                        });
                    }),
                );
            })),
        }));
        cx.notify();
    }

    pub fn check_updates_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (core, gid) = (self.core(), self.gid());
        self.exec(window, cx, async move { json_of(mn::check_updates(&core, gid).await) }, |this, st, _, cx| {
            this.set_state(st, cx);
            this.toast("chk", "Checked for updates", "", "", None, 2000);
        });
    }

    pub fn ensure_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                let core = this.core();
                this.state_call(window, cx, async move { json_of(mn::ensure_base(&core, gid).await) });
            }),
        );
    }

    pub fn repair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                this.batch = Some(("missing mods".into(), HashSet::new()));
                let core = this.core();
                this.exec_or(
                    window,
                    cx,
                    async move { json_of(mn::repair_dependencies(&core, gid).await) },
                    |this, st: Value, _, cx| {
                        this.dismiss("batch");
                        this.batch = None;
                        let left = !arr(&st, "missingDeps").is_empty();
                        this.toast("rep", if left { "Some mods are still missing" } else { "Repaired: every mod has what it needs" }, "", if left { "error" } else { "ok" }, None, 4000);
                        this.set_state(st, cx);
                    },
                    |this, e, _, _| {
                        this.dismiss("batch");
                        this.batch = None;
                        this.toast_error(&e);
                    },
                );
            }),
        );
    }

    /// Loader for your own games + keep mods updated; once per game per session.
    fn auto_maintain(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(st) = self.state.as_ref() else { return };
        if self.auto_done.contains(&id) || !b(&st["game"], "installed") {
            return;
        }
        self.auto_done.insert(id.clone());
        let wants_loader = s(&st["game"], "source") == "own" || !arr(st, "mods").is_empty();
        if b(&self.settings, "autoInstallLoader") && wants_loader && !arr(st, "baseMissing").is_empty() {
            let core = self.core();
            let gid = id.clone();
            self.exec_or(
                window,
                cx,
                async move { json_of(mn::ensure_base(&core, gid).await) },
                move |this, st, _, cx| {
                    this.set_state(st, cx);
                    this.check_and_update(id, cx);
                },
                |this, e, _, cx| {
                    if !e.starts_with("GAME_RUNNING|") {
                        this.toast_error(&e);
                    }
                    if let Some(id) = this.game_id.clone() {
                        this.check_and_update(id, cx);
                    }
                },
            );
        } else {
            self.check_and_update(id, cx);
        }
    }

    /// Looks for newer versions of this game's mods; installs them if "Auto-update mods" is on.
    /// Quietly waits while the game is running (mod files are locked then).
    pub fn check_and_update(&mut self, id: String, cx: &mut Context<Self>) {
        if self.checking || !self.state.as_ref().map(|st| b(&st["game"], "installed")).unwrap_or(false) {
            return;
        }
        self.checking = true;
        let core = self.core();
        let auto = b(&self.settings, "autoUpdateMods");
        cx.spawn(async move |this, cx| {
            let core2 = core.clone();
            let gid = id.clone();
            let st = bridge::run(async move { json_of(mn::check_updates(&core2, gid).await) }).await;
            let outdated: Vec<String> = match &st {
                Ok(st) => arr(st, "mods").iter().filter(|m| b(m, "updateAvailable")).map(|m| s(m, "key")).collect(),
                Err(_) => vec![],
            };
            let go = this
                .update(cx, |this, cx| {
                    match st {
                        Ok(st) => this.set_state(st, cx),
                        Err(e) if !e.starts_with("GAME_RUNNING|") => this.toast_error(&e),
                        _ => {}
                    }
                    auto && !outdated.is_empty()
                })
                .unwrap_or(false);
            if go {
                let core3 = core.clone();
                let gid = id.clone();
                let running = bridge::run(async move { mn::game_running(&core3, gid) }).await;
                if matches!(running, Ok(None)) {
                    let _ = this.update(cx, |this, cx| {
                        this.batch = Some(("updates".into(), HashSet::new()));
                        cx.notify();
                    });
                    let n = outdated.len();
                    let gid = id.clone();
                    let r = bridge::run(async move { json_of(mn::update_mods(&core, gid, outdated).await) }).await;
                    let _ = this.update(cx, |this, cx| {
                        this.dismiss("batch");
                        this.batch = None;
                        match r {
                            Ok(st) => {
                                this.set_state(st, cx);
                                this.toast(&format!("upd{id}"), format!("Updated {}", plural(n, "mod")), "", "ok", None, 4000);
                            }
                            Err(e) if !e.starts_with("GAME_RUNNING|") => this.toast_error(&e),
                            _ => {}
                        }
                        cx.notify();
                    });
                }
            }
            let _ = this.update(cx, |this, _| this.checking = false);
        })
        .detach();
    }

    /// New and updated mods from GitHub without restarting.
    pub fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        self.last_catalog_check = std::time::Instant::now();
        let core = self.core();
        cx.spawn(async move |this, cx| {
            let Ok(r) = bridge::run(async move { mn::refresh_catalog(&core).await }).await else { return };
            if !b(&r, "changed") {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.games = arr(&r, "games").to_vec();
                let names = |k: &str| arr(&r, k).iter().map(|p| format!("{} {}", s(p, "name"), s(p, "version"))).collect::<Vec<_>>();
                let (added, updated) = (names("added"), names("updated"));
                let list = |l: &Vec<String>| format!("{}{}", l.iter().take(3).cloned().collect::<Vec<_>>().join(", "), if l.len() > 3 { format!(" +{}", l.len() - 3) } else { String::new() });
                if !added.is_empty() || !updated.is_empty() {
                    let title = if added.is_empty() { "Catalog updated".to_string() } else { format!("New in the catalog: {}", list(&added)) };
                    let text = if updated.is_empty() { String::new() } else { format!("Updated: {}", list(&updated)) };
                    this.toast("catalog", title, text, "ok", None, 6000);
                }
                this.filters.retain(|k, _| !k.ends_with("|own"));
                if this.browse.as_ref().map(|b| b.source == "own").unwrap_or(false) {
                    this.browse = None;
                    this.pending_tab_shown = true;
                }
                match this.view {
                    View::Library => this.rebuild_library(cx),
                    View::Mine => this.show_mine(cx),
                    View::Game => {
                        if let Some(id) = this.game_id.clone() {
                            let core = this.core();
                            let gid = id.clone();
                            cx.spawn(async move |this, cx| {
                                let st = bridge::run(async move { json_of(mn::game_state(&core, gid).await) }).await;
                                let _ = this.update(cx, |this, cx| {
                                    if let Ok(st) = st {
                                        this.set_state(st, cx);
                                    }
                                    this.check_and_update(id, cx);
                                });
                            })
                            .detach();
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- play ----------

    pub fn launch(&mut self, vanilla: bool, cx: &mut Context<Self>) {
        self.menu = None;
        match mn::launch(&self.core, self.gid(), vanilla) {
            Ok(()) => self.toast("play", if vanilla { "Starting without mods…" } else { "Starting with mods…" }, "", "", None, 3000),
            Err(e) => self.toast_error(&e),
        }
        cx.notify();
    }

    pub fn set_steam_mode(&mut self, vanilla: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let (core, gid) = (self.core(), self.gid());
        self.exec(window, cx, async move { json_of(mn::set_steam_mode(&core, gid, vanilla).await) }, move |this, st, _, cx| {
            this.set_state(st, cx);
            this.toast("sm", if vanilla { "Steam now starts the normal game" } else { "Steam now starts the game with mods" }, if vanilla { "Use Play in ModNest to play with mods." } else { "" }, "ok", None, 3500);
        });
    }

    pub fn open_folder(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        if let Err(e) = mn::open_folder(&self.core, self.gid()) {
            self.toast_error(&e);
        }
        cx.notify();
    }

    pub fn install_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                let Some(path) = rfd::FileDialog::new().set_title("Pick a mod file").add_filter("Mod files", &["zip", "dll", "pak", "7z"]).add_filter("All files", &["*"]).pick_file() else { return };
                let core = this.core();
                this.exec(window, cx, async move { json_of(mn::install_file(&core, gid, path).await) }, |this, st: Value, _, cx| {
                    if !st.is_null() {
                        this.set_state(st, cx);
                        this.toast("file", "Mod installed from file", "", "ok", None, 3000);
                    }
                });
            }),
        );
    }

    pub fn set_game_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(path) = rfd::FileDialog::new().set_title("Pick the game's folder").pick_folder() else { return };
        let (core, gid) = (self.core(), self.gid());
        self.exec(window, cx, async move { json_of(mn::set_game_folder(&core, gid, path).await) }, |this, st: Value, _, cx| {
            if !st.is_null() {
                this.set_state(st, cx);
            }
        });
    }

    // ---------- profiles ----------

    /// Switches profile with one combined message instead of a popup per mod.
    pub fn switch_profile(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.batch = Some((name.clone(), HashSet::new()));
        let (core, gid) = (self.core(), self.gid());
        let n2 = name.clone();
        self.exec_or(
            window,
            cx,
            async move { json_of(mn::apply_profile(&core, gid, n2).await) },
            move |this, r: Value, _, cx| {
                this.dismiss("batch");
                let installed = this.batch.take().map(|b| b.1.len()).unwrap_or(0);
                if b(&r, "changed") {
                    this.toast("sw", format!("Now using “{name}”"), if installed > 0 { format!("{} installed", plural(installed, "mod")) } else { String::new() }, "ok", None, 2500);
                }
                let failed: Vec<String> = arr(&r, "failed").iter().filter_map(|x| x.as_str().map(String::from)).collect();
                if !failed.is_empty() {
                    this.toast("swf", format!("{} couldn't be installed", plural(failed.len(), "mod")), failed.join(" · "), "error", None, 12000);
                }
                this.browse = None;
                this.pending_tab_shown = true;
                this.set_state(r["state"].clone(), cx);
            },
            |this, e, _, _| {
                this.dismiss("batch");
                this.batch = None;
                this.toast_error(&e);
            },
        );
    }

    pub fn switch_profile_ui(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let gid = self.gid();
        self.ready_then(gid, window, cx, Box::new(move |this, _, window, cx| this.switch_profile(name, window, cx)));
    }

    pub fn new_profile_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.ask(
            "New profile",
            "Name",
            "",
            "Create",
            window,
            cx,
            Box::new(|this, name, window, cx| {
                let gid = this.gid();
                this.ready_then(
                    gid.clone(),
                    window,
                    cx,
                    Box::new(move |this, _, window, cx| {
                        let core = this.core();
                        this.exec(window, cx, async move { json_of(mn::new_profile(&core, gid, name).await) }, |this, st, window, cx| {
                            this.set_state(st, cx);
                            this.set_tab(Tab::Browse, window, cx);
                        });
                    }),
                );
            }),
        );
    }

    pub fn save_as_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(
            "Copy to new profile",
            "Name for the copy",
            "",
            "Save",
            window,
            cx,
            Box::new(|this, name, window, cx| {
                let (core, gid) = (this.core(), this.gid());
                this.state_call(window, cx, async move { json_of(mn::save_profile(&core, gid, name).await) });
            }),
        );
    }

    pub fn rename_dialog(&mut self, from: String, window: &mut Window, cx: &mut Context<Self>) {
        let f2 = from.clone();
        self.ask(
            "Rename profile",
            "New name",
            &from,
            "Rename",
            window,
            cx,
            Box::new(move |this, to, window, cx| {
                if to == f2 {
                    return;
                }
                let (core, gid) = (this.core(), this.gid());
                this.state_call(window, cx, async move { json_of(mn::rename_profile(&core, gid, f2, to).await) });
            }),
        );
    }

    pub fn delete_profile_dialog(&mut self, name: String, _window: &mut Window, cx: &mut Context<Self>) {
        let active = self.state.as_ref().and_then(|st| arr(st, "profiles").iter().find(|p| s(p, "name") == name).map(|p| b(p, "active"))).unwrap_or(false);
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Delete profile".into(),
            body: vec![format!("Delete “{name}”? {}", if active { "Its mods stay installed until you switch." } else { "Nothing in the game changes." })],
            small: None,
            ok: "Delete".into(),
            on_ok: Some(Box::new(move |this, _, window, cx| {
                let (core, gid) = (this.core(), this.gid());
                this.state_call(window, cx, async move { json_of(mn::delete_profile(&core, gid, name).await) });
            })),
        }));
        cx.notify();
    }

    pub fn share_dialog(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let code = match mn::export_profile(&self.core, self.gid(), name.clone()) {
            Ok(c) => c,
            Err(e) => {
                self.toast_error(&e);
                return cx.notify();
            }
        };
        cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
        let is_ts = self.state.as_ref().map(|st| s(&st["game"], "source") != "own").unwrap_or(false);
        let field = cx.new(|cx| gpui_component::input::InputState::new(window, cx).multi_line(true).default_value(code.clone()));
        self.modal = Some(Modal::Share { name, code, is_ts, field });
        cx.notify();
    }

    pub fn share_thunderstore(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.toast("ts", "Getting a Thunderstore code…", "", "", Some(-1.0), 0);
        let (core, gid) = (self.core(), self.gid());
        self.exec_or(
            window,
            cx,
            async move { mn::share_thunderstore(&core, gid, name).await },
            |this, short: String, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(short.clone()));
                this.toast("ts", format!("Thunderstore code: {short}"), "Copied. Works in r2modman and Thunderstore Mod Manager.", "ok", None, 10000);
            },
            |this, e, _, _| {
                this.dismiss("ts");
                this.toast_error(&e);
            },
        );
    }

    pub fn import_code_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let ts = self.state.as_ref().map(|st| s(&st["game"], "source") != "own").unwrap_or(false);
        let label = format!("Paste a ModNest code{}", if ts { " or a Thunderstore / r2modman code" } else { "" });
        self.ask("Import a profile", &label, "", "Import", window, cx, Box::new(|this, code, window, cx| this.import_code(code, window, cx)));
    }

    pub fn import_pasted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.code_in.read(cx).value().trim().to_string();
        if !code.is_empty() {
            self.import_code(code, window, cx);
        }
    }

    fn import_code(&mut self, code: String, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| {
                this.toast("imp", "Importing profile…", "", "", Some(-1.0), 0);
                let core = this.core();
                let g2 = gid.clone();
                this.exec_or(
                    window,
                    cx,
                    async move { mn::import_code(&core, code, g2).await },
                    move |this, r: Value, window, cx| {
                        if s(&r, "gameId") != gid {
                            this.toast("imp", format!("Imported “{}”", s(&r, "name")), "Added to the game it belongs to.", "ok", None, 5000);
                            return;
                        }
                        this.toast("imp", format!("Imported “{}”", s(&r, "name")), "", "ok", None, 2500);
                        this.code_in.update(cx, |s, cx| s.set_value("", window, cx));
                        this.tab = Tab::Installed;
                        this.switch_profile(s(&r, "name"), window, cx);
                    },
                    |this, e, _, _| {
                        this.dismiss("imp");
                        this.toast_error(&e);
                    },
                );
            }),
        );
    }

    pub fn import_manager(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let gid = self.gid();
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| match mn::import_manager_profile(&this.core, gid, name) {
                Ok(r) => {
                    this.tab = Tab::Installed;
                    this.switch_profile(s(&r, "name"), window, cx);
                }
                Err(e) => this.toast_error(&e),
            }),
        );
    }

    pub fn import_manager_any(&mut self, gid: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ready_then(
            gid.clone(),
            window,
            cx,
            Box::new(move |this, _, window, cx| match mn::import_manager_profile(&this.core, gid, name) {
                Ok(r) => {
                    let pname = s(&r, "name");
                    this.open_game_then(
                        s(&r, "gameId"),
                        Tab::Installed,
                        window,
                        cx,
                        Some(Box::new(move |this, _, window, cx| {
                            this.switch_profile(pname.clone(), window, cx);
                            this.toast("imp", format!("Imported “{pname}”"), "", "ok", None, 3000);
                        })),
                    );
                }
                Err(e) => this.toast_error(&e),
            }),
        );
    }

    pub fn open_profile_from_mine(&mut self, gid: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_game_then(
            gid,
            Tab::Profiles,
            window,
            cx,
            Some(Box::new(move |this, _, _, cx| {
                let Some(st) = this.state.as_ref() else { return };
                let Some(p) = arr(st, "profiles").iter().find(|p| s(p, "name") == name) else { return };
                if b(p, "active") {
                    return;
                }
                let game = s(&st["game"], "name");
                this.modal = Some(Modal::Confirm(Confirm {
                    title: "Switch profile".into(),
                    body: vec![format!("Switch {game} to “{name}” now?")],
                    small: None,
                    ok: "Switch".into(),
                    on_ok: Some(Box::new(move |this, _, window, cx| this.switch_profile_ui(name, window, cx))),
                }));
                cx.notify();
            })),
        );
    }

    // ---------- settings & games ----------

    pub fn save_settings(&mut self, loader: bool, steam: bool, update: bool, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let key_changed = key != s(&self.settings, "curseforgeKey");
        if let Some(o) = self.settings.as_object_mut() {
            o.insert("autoInstallLoader".into(), loader.into());
            o.insert("steamVanilla".into(), steam.into());
            o.insert("autoUpdateMods".into(), update.into());
        }
        let settings: mn::store::Settings = match serde_json::from_value(self.settings.clone()) {
            Ok(s) => s,
            Err(e) => return self.toast_error(&e.to_string()),
        };
        if let Err(e) = mn::save_settings(&self.core, settings) {
            return self.toast_error(&e);
        }
        if key_changed {
            self.toast("cf", if key.is_empty() { "Removing CurseForge…" } else { "Checking your CurseForge key…" }, "", "", Some(-1.0), 0);
            let core = self.core();
            let k2 = key.clone();
            self.exec_or(
                window,
                cx,
                async move { mn::set_curseforge_key(&core, k2).await },
                move |this, r: Value, window, cx| {
                    this.settings["curseforgeKey"] = key.clone().into();
                    this.games = arr(&r, "games").to_vec();
                    this.filters.clear();
                    this.browse = None;
                    this.toast("cf", if key.is_empty() { "CurseForge removed".to_string() } else { format!("CurseForge connected: {} games", n(&r, "curseforgeGames")) }, "", "ok", None, 4000);
                    this.after_settings(window, cx);
                },
                |this, e, _, _| this.toast("cf", "CurseForge key not saved", e, "error", None, 9000),
            );
        } else {
            self.after_settings(window, cx);
        }
    }

    fn after_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            View::Game => {
                let (core, gid) = (self.core(), self.gid());
                self.exec(window, cx, async move { json_of(mn::game_state(&core, gid).await) }, |this, st, window, cx| {
                    this.set_state(st, cx);
                    this.on_tab_shown(window, cx);
                });
            }
            View::Library => self.rebuild_library(cx),
            View::Mine => {}
        }
        cx.notify();
    }

    pub fn clear_cache(&mut self, cx: &mut Context<Self>) {
        match mn::clear_cache(&self.core) {
            Ok(n) => self.toast("cc", format!("Cleared {:.1} MB", n as f64 / 1048576.0), "", "ok", None, 3000),
            Err(e) => self.toast_error(&e),
        }
        cx.notify();
    }

    pub fn add_game(&mut self, game: Value, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new().set_title("Pick the folder with the game's .exe").pick_folder() else { return };
        let ng: mn::NewGame = match serde_json::from_value(game) {
            Ok(g) => g,
            Err(e) => return self.toast_error(&e.to_string()),
        };
        let core = self.core();
        self.exec(window, cx, async move { json_of(mn::add_custom_game(&core, ng, path).await) }, |this, v: Value, window, cx| {
            if v.is_null() {
                return;
            }
            this.games.push(v.clone());
            this.toast("ng", format!("Added {}", s(&v, "name")), format!("Mod loader: {}", s(&v, "loader")), "ok", None, 4000);
            this.open_game(s(&v, "id"), Tab::Installed, window, cx);
        });
    }

    pub fn save_game_settings(&mut self, folder: String, window: &mut Window, cx: &mut Context<Self>) {
        match mn::set_mods_folder(&self.core, self.gid(), folder) {
            Ok(st) => {
                self.set_state(serde_json::to_value(st).unwrap_or(Value::Null), cx);
                self.toast("gs", "Saved", "", "ok", None, 2000);
            }
            Err(e) => self.toast_error(&e),
        }
        let _ = window;
        cx.notify();
    }

    pub fn remove_game_dialog(&mut self, cx: &mut Context<Self>) {
        let name = self.state.as_ref().map(|st| s(&st["game"], "name")).unwrap_or_default();
        self.modal = Some(Modal::Confirm(Confirm {
            title: "Remove game".into(),
            body: vec![format!("Remove **{name}** from ModNest? Game files aren't touched.")],
            small: None,
            ok: "Remove".into(),
            on_ok: Some(Box::new(|this, _, _, cx| {
                let gid = this.gid();
                match mn::remove_custom_game(&this.core, gid.clone()) {
                    Ok(()) => {
                        this.games.retain(|g| s(g, "id") != gid);
                        this.show_library(cx);
                    }
                    Err(e) => this.toast_error(&e),
                }
            })),
        }));
        cx.notify();
    }
}
