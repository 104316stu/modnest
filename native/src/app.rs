//! The ModNest window: state (a port of the WebView UI's `ui` object) and the top-level layout.
use crate::bridge::{self, Event};
use crate::theme as t;
use crate::widgets::*;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use modnest_core as mn;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum View {
    Library,
    Game,
    Mine,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Installed,
    Browse,
    Modpacks,
    Profiles,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Menu {
    Profile,
    Play,
    Sort,
    Category,
}

/// One row of the virtualized library list.
#[derive(Clone)]
pub enum LibLine {
    Head,
    Group(&'static str, Option<String>),
    /// cards, ends with the add card, last row of its group
    Covers(Vec<usize>, bool, bool),
    Note(&'static str),
    Spacer,
}

pub struct Toast {
    pub id: String,
    pub title: String,
    pub text: String,
    pub kind: &'static str,
    pub fraction: Option<f64>,
    pub until: Option<std::time::Instant>,
}

pub struct ModNest {
    pub core: Arc<mn::Core>,
    pub version: String,
    pub games: Vec<Value>,
    pub settings: Value,
    pub loaded: bool,
    pub view: View,
    pub lib_all: bool,
    pub lib_filter: Entity<InputState>,
    pub lib_list: ListState,
    pub lib_lines: Vec<LibLine>,
    pub lib_cols: usize,
    pub game_id: Option<String>,
    pub state: Option<Value>,
    pub tab: Tab,
    pub inst_filter: Entity<InputState>,
    pub browse_q: Entity<InputState>,
    pub code_in: Entity<InputState>,
    pub layout_grid: bool,
    pub open: HashSet<String>,
    pub browse: Option<crate::game::Browse>,
    pub filters: HashMap<String, Value>,
    pub busy: HashSet<String>,
    pub auto_done: HashSet<String>,
    pub menu: Option<Menu>,
    pub source_for: HashMap<String, String>,
    pub toasts: Vec<Toast>,
    pub modal: Option<crate::dialogs::Modal>,
    pub batch: Option<(String, HashSet<String>)>,
    pub mine: Option<(Value, Vec<Value>)>,
    pub mgr_all: Option<Vec<Value>>,
    pub checking: bool,
    pub app_update: Option<Value>,
    pub last_catalog_check: std::time::Instant,
    pub pending_tab_shown: bool,
    pub updating: bool,
    pub placeholder_n: usize,
    /// width of the main area (window minus rail and scrollbar gutter)
    pub main_w: f32,
    /// whether the current view's scroll area is taller than its viewport (last frame)
    pub overflow: bool,
    pub mine_scroll: ScrollHandle,
    pub root_focus: FocusHandle,
    /// the library filter the user picked last time (None = decide from what is installed)
    pub lib_pref: Option<bool>,
    pub tab_scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl ModNest {
    pub fn new(engine: bridge::Engine, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lib_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search games"));
        let inst_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter installed mods"));
        let browse_q = cx.new(|cx| InputState::new(window, cx).placeholder("Search mods"));
        let code_in = cx.new(|cx| InputState::new(window, cx).multi_line(true).placeholder("Paste a code"));
        let mut subs = vec![];
        subs.push(cx.subscribe(&lib_filter, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.rebuild_library(cx);
                cx.notify();
            }
        }));
        subs.push(cx.subscribe(&inst_filter, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        }));
        subs.push(cx.subscribe(&browse_q, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.search_changed(cx);
            }
        }));
        let mut me = ModNest {
            core: engine.core.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            games: vec![],
            settings: Value::Null,
            loaded: false,
            view: View::Library,
            lib_all: false,
            lib_filter,
            lib_list: ListState::new(0, ListAlignment::Top, px(1200.)),
            lib_lines: vec![],
            lib_cols: 0,
            game_id: None,
            state: None,
            tab: Tab::Installed,
            inst_filter,
            browse_q,
            code_in,
            layout_grid: false,
            open: HashSet::new(),
            browse: None,
            filters: HashMap::new(),
            busy: HashSet::new(),
            auto_done: HashSet::new(),
            menu: None,
            source_for: HashMap::new(),
            toasts: vec![],
            modal: None,
            batch: None,
            mine: None,
            mgr_all: None,
            checking: false,
            app_update: None,
            last_catalog_check: std::time::Instant::now(),
            pending_tab_shown: false,
            updating: false,
            placeholder_n: usize::MAX,
            main_w: 1200.,
            overflow: false,
            mine_scroll: ScrollHandle::new(),
            root_focus: cx.focus_handle(),
            lib_pref: None,
            tab_scroll: ScrollHandle::new(),
            _subs: subs,
        };
        let prefs = Self::load_prefs();
        me.layout_grid = b(&prefs, "layoutGrid");
        me.lib_pref = prefs.get("libAll").and_then(|v| v.as_bool());
        me.root_focus.focus(window);
        me.start(engine.events, cx);
        me
    }

    fn start(&mut self, events: async_channel::Receiver<Event>, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let version = self.version.clone();
        cx.spawn(async move |this, cx| {
            let r = bridge::run(async move { mn::init(&core, version).await }).await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(v) => {
                        this.games = arr(&v, "games").to_vec();
                        this.settings = v.get("settings").cloned().unwrap_or(Value::Null);
                        this.lib_all = this.lib_pref.unwrap_or(!this.games.iter().any(|g| b(g, "installed")));
                    }
                    Err(e) => this.toast_error(&e),
                }
                this.loaded = true;
                this.rebuild_library(cx);
                cx.notify();
            });
        })
        .detach();
        // Progress events from the engine.
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |this, cx| this.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // Expire toasts, poll the catalog every 5 minutes, check the open game every 15.
        cx.spawn(async move |this, cx| {
            let mut ticks: u64 = 0;
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                ticks += 1;
                if this
                    .update(cx, |this, cx| {
                        let now = std::time::Instant::now();
                        let before = this.toasts.len();
                        this.toasts.retain(|t| t.until.map(|u| u > now).unwrap_or(true));
                        if before != this.toasts.len() {
                            cx.notify();
                        }
                        if ticks % 600 == 0 {
                            this.refresh_catalog(cx);
                        }
                        if ticks % 1800 == 0 && this.view == View::Game {
                            if let Some(id) = this.game_id.clone() {
                                this.check_and_update(id, cx);
                            }
                        }
                        if ticks == 4 || ticks % 43200 == 0 {
                            this.check_app_update(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    // ---------- toasts ----------

    pub fn toast(&mut self, id: &str, title: impl Into<String>, text: impl Into<String>, kind: &'static str, fraction: Option<f64>, ttl_ms: u64) {
        let until = (ttl_ms > 0).then(|| std::time::Instant::now() + std::time::Duration::from_millis(ttl_ms));
        let t = Toast { id: id.to_string(), title: title.into(), text: text.into(), kind, fraction, until };
        match self.toasts.iter_mut().find(|x| x.id == id) {
            Some(x) => *x = t,
            None => self.toasts.push(t),
        }
    }

    pub fn dismiss(&mut self, id: &str) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn toast_error(&mut self, e: &str) {
        let id = format!("err{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        if let Some(rest) = e.strip_prefix("GAME_RUNNING|") {
            let msg = rest.splitn(2, '|').nth(1).unwrap_or(rest).to_string();
            self.toast(&id, "Close the game first", msg, "error", None, 8000);
        } else {
            self.toast(&id, "Something went wrong", e, "error", None, 10000);
        }
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        if ev.name == "app-update-progress" {
            let f = ev.payload.as_f64().unwrap_or(-1.0);
            self.toast("selfupd", "Downloading ModNest update", "", "", Some(f), 0);
            cx.notify();
            return;
        }
        if ev.name != "progress" {
            return;
        }
        let p = &ev.payload;
        let (stage, title, key) = (s(p, "stage"), s(p, "title"), s(p, "key"));
        let fraction = p.get("fraction").and_then(|f| f.as_f64()).unwrap_or(-1.0);
        if let Some((name, done)) = self.batch.as_mut() {
            if stage == "done" {
                done.insert(title.clone());
            }
            let n = done.len();
            let name = name.clone();
            let text = if n > 0 { format!("{} installed", plural(n, "mod")) } else { format!("Installing {title}") };
            self.toast("batch", format!("Setting up “{name}”…"), text, "", Some(-1.0), 0);
        } else {
            let id = format!("p:{key}");
            match stage.as_str() {
                "done" => self.toast(&id, title, "Installed", "ok", None, 2200),
                "resolve" => self.toast(&id, "Getting mod info…", "", "", Some(-1.0), 0),
                "install" => self.toast(&id, format!("Installing {title}"), "", "", Some(-1.0), 0),
                _ => self.toast(&id, format!("Downloading {title}"), "", "", Some(fraction), 0),
            }
        }
        cx.notify();
    }

    // ---------- library ----------

    pub fn rebuild_library(&mut self, cx: &mut Context<Self>) {
        let f = self.lib_filter.read(cx).value().to_lowercase();
        let list: Vec<usize> = self
            .games
            .iter()
            .enumerate()
            .filter(|(_, g)| f.is_empty() || s(g, "name").to_lowercase().contains(&f))
            .filter(|(_, g)| self.lib_all || b(g, "installed"))
            .map(|(i, _)| i)
            .collect();
        let by = |src: &str| list.iter().copied().filter(|&i| s(&self.games[i], "source") == src).collect::<Vec<_>>();
        let (own, custom, ts, cf) = (by("own"), by("custom"), by("thunderstore"), by("curseforge"));
        let cols = self.lib_cols.max(1);
        let mut lines = vec![];
        let group = |title: &'static str, pill: Option<String>, items: Vec<usize>, add_card: bool, lines: &mut Vec<LibLine>| {
            if items.is_empty() && !add_card {
                return;
            }
            lines.push(LibLine::Group(title, pill));
            let mut cells: Vec<usize> = items;
            let total = cells.len() + add_card as usize;
            let rows = total.div_ceil(cols);
            for r in 0..rows {
                let end = ((r + 1) * cols).min(cells.len());
                let start = (r * cols).min(end);
                let row: Vec<usize> = cells[start..end].to_vec();
                let last = r == rows - 1;
                lines.push(LibLine::Covers(row, add_card && last, last));
            }
            cells.clear();
        };
        group("Your mods", Some("ModNest catalog".into()), own, false, &mut lines);
        group("Added by you", None, custom, f.is_empty(), &mut lines);
        let tn = ts.len();
        group("Thunderstore", Some(format!("{tn} games")), ts, false, &mut lines);
        let cn = cf.len();
        group("CurseForge", Some(format!("{cn} games")), cf, false, &mut lines);
        if list.is_empty() {
            lines.push(LibLine::Note("empty"));
        }
        if self.lib_all && cn == 0 && s(&self.settings, "curseforgeKey").is_empty() {
            lines.push(LibLine::Note("cf"));
        }
        lines.push(LibLine::Spacer);
        self.lib_lines = lines;
        self.lib_list.reset(self.lib_lines.len());
    }

    fn prefs_path() -> std::path::PathBuf {
        bridge::data_dir().join("ui-native.json")
    }

    fn load_prefs() -> Value {
        std::fs::read(Self::prefs_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
    }

    /// Remembers small view choices between launches.
    pub fn save_prefs(&self) {
        let v = serde_json::json!({ "layoutGrid": self.layout_grid, "libAll": self.lib_pref });
        let _ = std::fs::write(Self::prefs_path(), v.to_string());
    }

    /// Esc closes a dialog or menu; Ctrl+F jumps to the search box on the page.
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.key == "escape" {
            if self.modal.is_some() {
                self.modal = None;
            } else {
                self.menu = None;
            }
            self.root_focus.focus(window);
            cx.notify();
        } else if k.modifiers.control && k.key == "f" && self.modal.is_none() {
            let input = match self.view {
                View::Library => Some(self.lib_filter.clone()),
                View::Game if matches!(self.tab, Tab::Browse | Tab::Modpacks) => Some(self.browse_q.clone()),
                View::Game if self.tab == Tab::Installed => Some(self.inst_filter.clone()),
                _ => None,
            };
            if let Some(i) = input {
                i.update(cx, |s, cx| s.focus(window, cx));
                cx.stop_propagation();
            }
        }
    }

    pub fn show_library(&mut self, cx: &mut Context<Self>) {
        self.view = View::Library;
        self.state = None;
        self.menu = None;
        self.rebuild_library(cx);
        cx.notify();
    }
}

impl Render for ModNest {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Library columns follow the window width like CSS grid auto-fill minmax(138px, 1fr).
        // minus the 10px scrollbar gutter (::-webkit-scrollbar width)
        let overflow = match self.view {
            View::Library => self.lib_list.max_offset_for_scrollbar().height > px(0.5),
            View::Mine => self.mine_scroll.max_offset().height > px(0.5),
            View::Game => self.tab_scroll.max_offset().height > px(0.5),
        };
        if overflow != self.overflow {
            self.overflow = overflow;
            // sizes depend on it; draw again with the right gutter
            window.request_animation_frame();
        }
        if self.view == View::Game {
            // also catches scrollbar drags and lists too short to scroll
            self.maybe_load_more(cx);
        }
        let main_w = f32::from(window.viewport_size().width) - t::RAIL_W - 1.0 - if overflow { t::SCROLLBAR } else { 0. };
        self.main_w = main_w;
        let cols = (((main_w - 2.0 * t::LIB_PAD_X + 16.0) / (138.0 + 16.0)).floor() as usize).max(1);
        if cols != self.lib_cols {
            self.lib_cols = cols;
            self.rebuild_library(cx);
        }
        if self.games.len() != self.placeholder_n {
            self.placeholder_n = self.games.len();
            let text = format!("Search {} games", self.placeholder_n);
            self.lib_filter.update(cx, |s, cx| s.set_placeholder(text, window, cx));
        }
        if self.pending_tab_shown {
            self.pending_tab_shown = false;
            self.on_tab_shown(window, cx);
        }
        let main: AnyElement = if !self.loaded {
            crate::library::loading().into_any_element()
        } else {
            match self.view {
                View::Library => crate::library::render(self, main_w, cx).into_any_element(),
                View::Mine => crate::library::render_mine(self, cx).into_any_element(),
                View::Game => crate::game::render(self, window, cx).into_any_element(),
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t::bg())
            .text_color(t::text())
            .font_family(t::FONT)
            .text_size(px(t::TEXT))
            .line_height(relative(1.45))
            .track_focus(&self.root_focus)
            .on_key_down(cx.listener(Self::on_key))
            .when_some(self.app_update.clone(), |d, u| d.child(crate::dialogs::update_bar(&u, self.updating, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.menu.is_some() {
                    this.menu = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(crate::library::rail(self, cx))
                    .child(div().flex().flex_col().flex_1().min_w_0().min_h_0().child(main)),
            )
            .child(crate::dialogs::toasts(self, cx))
            .when(self.modal.is_some(), |d| d.child(crate::dialogs::modal(self, window, cx)))
    }
}
