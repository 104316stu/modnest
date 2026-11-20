//! Game page: banner, tabs, Installed, Browse/Modpacks, Profiles (matches .banner, .tabs, .row, .card … in styles.css).
use crate::app::{Menu, ModNest, Tab};
use crate::library::{search_box, seg, spinner};
use crate::theme as t;
use crate::widgets::*;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::input::Input;
use serde_json::Value;

/// What Browse / Modpacks is showing.
#[derive(Clone)]
pub struct Browse {
    pub tab: Tab,
    pub source: String,
    pub q: String,
    pub ordering: String,
    pub category: String,
    pub page: u32,
    pub items: Vec<Value>,
    pub total: u64,
    pub has_more: bool,
    pub loading: bool,
    pub loaded: bool,
    pub error: String,
    pub generation: u64,
}

pub const SOURCE_NAME: &[(&str, &str)] = &[("own", "ModNest"), ("ts", "Thunderstore"), ("cf", "CurseForge")];
pub fn source_name(s: &str) -> &'static str {
    SOURCE_NAME.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or("")
}

pub fn render(me: &mut ModNest, window: &mut Window, cx: &mut Context<ModNest>) -> impl IntoElement {
    let Some(st) = me.state.clone() else {
        return div().size_full().flex().items_center().justify_center().child(spinner(26.)).into_any_element();
    };
    let g = st["game"].clone();
    let mods = arr(&st, "mods");
    let updates = mods.iter().filter(|m| b(m, "updateAvailable")).count();
    let cur = me.tab;
    let tab = |id: &str, which: Tab, label: &str, count: Option<usize>, amber: bool, cx: &mut Context<ModNest>| {
        let active = cur == which;
        div()
            .id(SharedString::from(format!("tab-{id}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .pt(px(11.))
            .pb(px(10.))
            .mb(px(-1.))
            .border_b_2()
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .when(active, |d| d.border_color(t::accent()).text_color(t::strong()))
            .when(!active, |d| d.border_color(transparent_black()).text_color(t::muted()).hover(|s| s.text_color(t::text())))
            .child(label.to_string())
            .when_some(count, |d, n| {
                d.child(div().px(px(7.)).rounded_full().text_size(px(11.)).bg(if amber { t::amber_lo() } else { t::surface_3() }).text_color(if amber { t::amber() } else { t::text() }).child(n.to_string()))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.set_tab(which, window, cx)))
    };
    let body: AnyElement = match me.tab {
        Tab::Installed => installed(me, &st, window, cx).into_any_element(),
        Tab::Profiles => profiles_tab(me, &st, cx).into_any_element(),
        Tab::Browse | Tab::Modpacks => browse(me, &st, cx).into_any_element(),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(banner(me, &st, &g, updates, f32::from(window.viewport_size().width) - t::RAIL_W - 1.0, cx))
        .child(
            div()
                .flex()
                .flex_none()
                .gap(px(4.))
                .px(px(24.))
                .border_b_1()
                .border_color(t::line_soft())
                .child(tab("installed", Tab::Installed, "Installed", Some(mods.len()), false, cx))
                .child(tab("browse", Tab::Browse, "Browse", None, false, cx))
                .child(tab("modpacks", Tab::Modpacks, "Modpacks", None, false, cx))
                .child(tab("profiles", Tab::Profiles, "Profiles", Some(arr(&st, "profiles").len()), false, cx)),
        )
        .child(crate::library::scroll_area(div().id("tab-body").size_full().overflow_y_scroll().track_scroll(&me.tab_scroll).on_scroll_wheel(cx.listener(|this, _, _, cx| this.maybe_load_more(cx))).flex().flex_col().child(div().flex_none().w_full().flex().flex_col().child(body)), &me.tab_scroll, me.overflow))
        .into_any_element()
}

// ---------- banner (.banner) ----------

fn banner(me: &ModNest, st: &Value, g: &Value, updates: usize, width: f32, cx: &mut Context<ModNest>) -> impl IntoElement {
    let installed = b(g, "installed");
    let can_play = installed && (g.get("appId").map(|v| !v.is_null()).unwrap_or(false) || so(&g["custom"], "exe").is_some());
    let hero = so(g, "hero").or_else(|| so(g, "cover")).unwrap_or_default();
    let capsule = so(g, "capsule").or_else(|| so(g, "cover")).unwrap_or_default();
    let base_missing = !arr(st, "baseMissing").is_empty();
    let chips = div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .child(pill(s(g, "loader"), Tone::Plain))
        .children(arr(g, "sources").iter().filter_map(|x| x.as_str()).map(|x| pill(source_name(x), Tone::Line)))
        .child(if installed { if base_missing { pill("Loader not set up", Tone::Amber) } else { pill("Ready", Tone::Green) } } else { pill("Game not found", Tone::Red) })
        .when(updates > 0, |d| d.child(pill(plural(updates, "update"), Tone::Amber)))
        .when(b(st, "steamSwitchable"), |d| d.child(pill(if b(st, "steamVanilla") { "Steam starts it without mods" } else { "Steam starts it with mods" }, Tone::Line)));
    let profile_btn = div()
        .relative()
        .child(
            button("profile-menu", Btn::Glass, false)
                .max_w(px(230.))
                .child(icon("layers", 15., t::strong()))
                .child(div().min_w_0().overflow_hidden().whitespace_nowrap().child(so(g, "activeProfile").unwrap_or_else(|| "No profile".into())))
                .child(icon("caret", 16., t::strong()))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.toggle_menu(Menu::Profile, cx) })),
        )
        .when(me.menu == Some(Menu::Profile), |d| d.child(deferred(profile_menu(st, cx)).with_priority(2)));
    let play = div()
        .relative()
        .flex()
        .child(
            div()
                .id("play")
                .flex()
                .items_center()
                .gap(px(7.))
                .h(px(40.))
                .px(px(22.))
                .rounded_l(px(8.))
                .bg(t::green())
                .text_color(t::play_text())
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(15.))
                .when(can_play, |d| d.cursor_pointer().hover(|s| s.bg(t::green_hover())))
                .when(!can_play, |d| d.opacity(0.45))
                .child(icon("play", 16., t::play_text()))
                .child("Play")
                .when(can_play, |d| d.on_click(cx.listener(|this, _, _, cx| this.launch(false, cx)))),
        )
        .child(
            div()
                .id("play-more")
                .flex()
                .items_center()
                .justify_center()
                .h(px(40.))
                .w(px(34.))
                .rounded_r(px(8.))
                .bg(t::green())
                .border_l_1()
                .border_color(rgba(0x00000033))
                .cursor_pointer()
                .hover(|s| s.bg(t::green_hover()))
                .child(icon("caret", 18., t::play_text()))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.toggle_menu(Menu::Play, cx) })),
        )
        .when(me.menu == Some(Menu::Play), |d| d.child(deferred(play_menu(st, g, cx)).with_priority(2)));
    div()
        .relative()
        .flex_none()
        .h(px(t::BANNER_H))
        .bg(rgb(0x0c0e12))
        .overflow_hidden()
        .child(fitted(&hero, width, t::BANNER_H, if so(g, "hero").is_some() { 1920. / 620. } else { 460. / 215. }, 0.3).absolute().inset_0())
        // two gradients from .banner::after
        .child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(rgba(0x111318f2), 0.), linear_color_stop(rgba(0x1113188c), 0.55))))
        .child(div().absolute().inset_0().bg(linear_gradient(90., linear_color_stop(rgba(0x1113188c), 0.55), linear_color_stop(rgba(0x11131833), 1.))).opacity(1.))
        .child(div().absolute().inset_0().bg(linear_gradient(180., linear_color_stop(rgba(0x11131800), 0.55), linear_color_stop(t::bg(), 1.))))
        .child(
            div()
                .absolute()
                .left(px(28.))
                .right(px(28.))
                .bottom(px(18.))
                .flex()
                .items_end()
                .gap(px(18.))
                .child(picture(&capsule).w(px(76.)).h(px(114.)).flex_none().rounded(px(8.)).object_fit(ObjectFit::Cover).bg(t::surface_2()))
                .child(div().flex().flex_col().min_w_0().child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(white()).line_height(relative(1.1)).mb(px(6.)).child(s(g, "name"))).child(chips))
                .child(div().flex_1())
                .child(div().flex().items_center().gap(px(8.)).child(profile_btn).child(play)),
        )
}

/// .menu-pop: 6px under its button (`top` = button height + 6), 230px wide.
fn menu_pop(top: f32) -> Div {
    div()
        .absolute()
        .top(px(top))
        .right_0()
        .w(px(230.))
        .bg(t::surface_2())
        .border_1()
        .border_color(t::line())
        .rounded(px(10.))
        .p(px(5.))
        .shadow(vec![BoxShadow { color: t::shadow().opacity(1.), offset: point(px(0.), px(14.)), blur_radius: px(34.), spread_radius: px(0.) }])
        .occlude()
}

fn menu_item(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).flex().items_center().gap(px(8.)).px(px(10.)).py(px(7.)).rounded(px(7.)).text_color(t::text()).cursor_pointer().hover(|s| s.bg(t::surface_3()).text_color(t::strong()))
}

fn menu_label(text: &str) -> Div {
    div().text_size(px(11.)).font_weight(FontWeight::BOLD).text_color(t::muted()).px(px(10.)).pt(px(6.)).pb(px(3.)).child(text.to_uppercase())
}

fn menu_hr() -> Div {
    div().h(px(1.)).bg(t::line()).my(px(5.))
}

fn tick(on: bool) -> AnyElement {
    if on { icon("check", 14., t::accent_hi()).into_any_element() } else { div().w(px(14.)).flex_none().into_any_element() }
}

fn profile_menu(st: &Value, cx: &mut Context<ModNest>) -> impl IntoElement {
    menu_pop(t::BTN_H + 6.)
        .child(menu_label("Profiles"))
        .children(arr(st, "profiles").iter().map(|p| {
            let name = s(p, "name");
            let n2 = name.clone();
            menu_item(SharedString::from(format!("pm-{name}")))
                .child(tick(b(p, "active")))
                .child(div().flex_1().min_w_0().child(name.clone()))
                .child(div().text_color(t::muted()).text_size(px(12.)).child(n(p, "count").to_string()))
                .on_click(cx.listener(move |this, _, window, cx| this.switch_profile_ui(n2.clone(), window, cx)))
        }))
        .child(menu_hr())
        .child(menu_item("pm-new").child("New empty profile…").on_click(cx.listener(|this, _, window, cx| this.new_profile_dialog(window, cx))))
        .child(menu_item("pm-import").child("Import a code…").on_click(cx.listener(|this, _, window, cx| this.import_code_dialog(window, cx))))
        .child(menu_item("pm-manage").child("Manage profiles").on_click(cx.listener(|this, _, window, cx| this.set_tab(Tab::Profiles, window, cx))))
}

fn play_menu(st: &Value, g: &Value, cx: &mut Context<ModNest>) -> impl IntoElement {
    let switchable = b(st, "steamSwitchable");
    let vanilla = b(st, "steamVanilla");
    let installed = b(g, "installed");
    let custom = !g["custom"].is_null();
    menu_pop(46.)
        .child(menu_item("pl-vanilla").child(icon("play", 14., t::text())).child("Play without mods").on_click(cx.listener(|this, _, _, cx| this.launch(true, cx))))
        .when(switchable, |d| {
            d.child(menu_hr())
                .child(menu_label("Starting from Steam"))
                .child(menu_item("pl-sv1").child(tick(vanilla)).child(div().flex_1().min_w_0().child("Normal game (mods only via ModNest)")).on_click(cx.listener(|this, _, window, cx| this.set_steam_mode(true, window, cx))))
                .child(menu_item("pl-sv0").child(tick(!vanilla)).child("With mods").on_click(cx.listener(|this, _, window, cx| this.set_steam_mode(false, window, cx))))
        })
        .child(menu_hr())
        .child(menu_item("pl-file").child("Install mod from file…").on_click(cx.listener(|this, _, window, cx| this.install_file(window, cx))))
        .child(menu_item("pl-folder").child("Open game folder").on_click(cx.listener(|this, _, _, cx| this.open_folder(cx))))
        .when(!installed && !custom, |d| d.child(menu_item("pl-set").child("Set game folder…").on_click(cx.listener(|this, _, window, cx| this.set_game_folder(window, cx)))))
        .child(menu_item("pl-settings").child("Game settings…").on_click(cx.listener(|this, _, window, cx| this.game_settings_dialog(window, cx))))
}

// ---------- notes (warning bars above lists) ----------

pub fn notes(me: &ModNest, st: &Value, cx: &mut Context<ModNest>) -> Option<AnyElement> {
    let g = &st["game"];
    let installed = b(g, "installed");
    let mods = arr(st, "mods");
    let active = arr(st, "profiles").iter().find(|p| b(p, "active"));
    let gid = s(g, "id");
    if let Some(all) = &me.mgr_all {
        let offers: Vec<&Value> = all.iter().filter(|p| s(p, "gameId") == gid && n(p, "count") > 0 && !arr(st, "profiles").iter().any(|x| s(x, "name") == s(p, "name") && n(x, "count") > 0)).collect();
        if installed && !offers.is_empty() && active.map(|a| n(a, "count") == 0).unwrap_or(true) {
            return Some(
                div()
                    .flex()
                    .flex_col()
                    .children(offers.into_iter().take(3).map(|p| {
                        let name = s(p, "name");
                        let n2 = name.clone();
                        note(Tone::Blue)
                            .child(div().child(format!("Found “{}” ({}) in Thunderstore Mod Manager.", name, plural(n(p, "count") as usize, "mod"))))
                            .child(button(SharedString::from(format!("use-{name}")), Btn::Primary, true).child("Use it").on_click(cx.listener(move |this, _, window, cx| this.import_manager(n2.clone(), window, cx))))
                    }))
                    .into_any_element(),
            );
        }
    }
    let md = arr(st, "missingDeps");
    if installed && !md.is_empty() {
        let names: Vec<String> = md.iter().filter_map(|k| k.as_str()).map(|k| k.split_once(':').map(|x| x.1).unwrap_or(k).split_once('-').map(|x| x.1.to_string()).unwrap_or_else(|| k.to_string()).replace('_', " ")).collect();
        let more = if names.len() > 5 { format!(" +{} more", names.len() - 5) } else { String::new() };
        return Some(
            note(Tone::Amber)
                .child(div().child(format!(
                    "Missing {}: {}{more}",
                    if md.len() == 1 { "dependency" } else { "dependencies" },
                    names.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                )))
                .child(button("repair", Btn::Primary, true).child("Repair").on_click(cx.listener(|this, _, window, cx| this.repair(window, cx))))
                .into_any_element(),
        );
    }
    if !installed {
        return Some(
            note(Tone::Amber)
                .child(div().child(format!("{} wasn't found in Steam.", s(g, "name"))))
                .child(button("set-folder", Btn::Normal, true).child("Set game folder").on_click(cx.listener(|this, _, window, cx| this.set_game_folder(window, cx))))
                .into_any_element(),
        );
    }
    if !arr(st, "baseMissing").is_empty() {
        return Some(
            note(Tone::Blue)
                .child(div().child(format!("{} is set up automatically with your first mod.", s(g, "loader"))))
                .child(button("base", Btn::Normal, true).child("Set it up now").on_click(cx.listener(|this, _, window, cx| this.ensure_base(window, cx))))
                .into_any_element(),
        );
    }
    let updates = mods.iter().filter(|m| b(m, "updateAvailable")).count();
    if updates > 0 {
        return Some(
            note(Tone::Amber)
                .child(div().child(format!("{} can be updated.", plural(updates, "mod"))))
                .child(button("update-all", Btn::Primary, true).child("Update all").on_click(cx.listener(|this, _, window, cx| this.update_all(window, cx))))
                .into_any_element(),
        );
    }
    None
}

// ---------- rows (.row) ----------

pub fn row_shell(open: bool) -> Div {
    div().bg(t::surface()).rounded(px(9.)).border_1().border_color(if open { t::line() } else { transparent_black().into() }).hover(|s| s.border_color(t::line()).bg(t::row_hover()))
}

fn toolbar() -> Div {
    div().flex().flex_wrap().gap(px(8.)).items_center().px(px(24.)).pt(px(14.)).pb(px(8.))
}

fn installed(me: &mut ModNest, st: &Value, _window: &mut Window, cx: &mut Context<ModNest>) -> impl IntoElement {
    let g = &st["game"];
    let mods = arr(st, "mods");
    let f = me.inst_filter.read(cx).value().to_lowercase();
    let list: Vec<&Value> = mods.iter().filter(|m| f.is_empty() || s(m, "title").to_lowercase().contains(&f) || s(m, "author").to_lowercase().contains(&f)).collect();
    let title_of = |k: &str| -> String {
        mods.iter().find(|m| s(m, "key") == k).map(|m| s(m, "title")).unwrap_or_else(|| k.split_once(':').map(|x| x.1).unwrap_or(k).split_once('-').map(|x| x.1.to_string()).unwrap_or_else(|| k.to_string()).replace('_', " "))
    };
    let game_installed = b(g, "installed");
    div()
        .flex()
        .flex_col()
        .child(toolbar().child(search_box(&me.inst_filter)).child(button("check", Btn::Normal, false).child("Check for updates").on_click(cx.listener(|this, _, window, cx| this.check_updates_click(window, cx)))))
        .when_some(notes(me, st, cx), |d, n| d.child(n))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(24.))
                .pt(px(6.))
                .pb(px(28.))
                .children(list.iter().map(|m| {
                    let key = s(m, "key");
                    let open = me.open.contains(&key);
                    let (required, auto, enabled) = (b(m, "required"), b(m, "auto"), b(m, "enabled"));
                    let version = s(m, "version");
                    let desc = if required { "Mod loader · always installed".to_string() } else if auto { "Needed by another mod".into() } else if b(m, "fromFile") { "Installed from a file".into() } else if b(m, "adopted") { "Found in the game folder".into() } else if version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) { format!("Version {version}") } else { version.clone() };
                    let deps: Vec<String> = arr(m, "dependencies").iter().filter_map(|d| d.as_str()).map(|d| title_of(d)).collect();
                    let (k1, k2, k3, k4, k5) = (key.clone(), key.clone(), key.clone(), key.clone(), key.clone());
                    row_shell(open)
                        .when(!enabled, |d| d.opacity(0.5))
                        .child(
                            div()
                                .id(SharedString::from(format!("row-{key}")))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .px(px(10.))
                                .py(px(6.))
                                .min_h(px(t::ROW_MIN_H))
                                .cursor_pointer()
                                .child(mod_icon(m))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_w_0()
                                        .child(div().flex().items_baseline().gap(px(6.)).min_w_0().child(div().font_weight(t::W650).text_color(t::strong()).min_w_0().overflow_hidden().whitespace_nowrap().child(s(m, "title"))).when(!s(m, "author").is_empty(), |d| d.child(div().text_color(t::muted()).text_size(px(12.)).whitespace_nowrap().child(format!("by {}", s(m, "author"))))))
                                        .child(div().text_color(t::muted()).text_size(px(12.5)).ellipsis().child(desc)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .gap(px(4.))
                                        .when(required, |d| d.child(pill("Base", Tone::Blue)))
                                        .when(auto, |d| d.child(pill("Dependency", Tone::Line)))
                                        .when(b(m, "updateAvailable"), |d| d.child(pill(format!("→ {}", s(m, "latest")), Tone::Amber))),
                                )
                                .child(switch(SharedString::from(format!("sw-{key}")), enabled, required || !game_installed).when(!required && game_installed, |d| d.on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.toggle_mod(k1.clone(), window, cx)
                                }))))
                                .child(icon("chev", 20., t::muted()).when(open, |d| d.with_transformation(Transformation::rotate(radians(std::f32::consts::PI)))))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.open.remove(&k2) {
                                        this.open.insert(k2.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(open, |d| {
                            d.child(
                                div()
                                    .pl(px(62.))
                                    .pr(px(14.))
                                    .pt(px(2.))
                                    .pb(px(12.))
                                    .when(!deps.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(t::muted()).mb(px(10.)).child(format!("Needs {}", deps.join(", ")))))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap(px(6.))
                                            .when(b(m, "updateAvailable"), |d| d.child(button(SharedString::from(format!("up-{key}")), Btn::Primary, true).child(format!("Update to {}", s(m, "latest"))).on_click(cx.listener(move |this, _, window, cx| this.update_one(k3.clone(), window, cx)))))
                                            .when(b(m, "adopted") && !required, |d| {
                                                let k = k4.clone();
                                                d.child(button(SharedString::from(format!("re-{key}")), Btn::Normal, true).child("Reinstall latest").on_click(cx.listener(move |this, _, window, cx| this.update_one(k.clone(), window, cx))))
                                            })
                                            .when(!required, |d| d.child(button(SharedString::from(format!("rm-{key}")), Btn::Danger, true).child("Uninstall").on_click(cx.listener(move |this, _, window, cx| this.remove_dialog(k5.clone(), window, cx)))))
                                            .when(key.starts_with("ts:"), |d| {
                                                let url = format!("https://thunderstore.io/package/{}/", key[3..].replacen('-', "/", 1));
                                                d.child(button(SharedString::from(format!("pg-{key}")), Btn::Ghost, true).child("Mod page ↗").on_click(cx.listener(move |this, _, _, cx| this.open_url(url.clone(), cx))))
                                            }),
                                    ),
                            )
                        })
                }))
                .when(list.is_empty(), |d| {
                    d.child(
                        div()
                            .py(px(48.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(14.))
                            .text_color(t::muted())
                            .child(if mods.is_empty() { "No mods yet." } else { "Nothing matches that filter." })
                            .child(button("go-browse", Btn::Primary, false).child("Browse mods").on_click(cx.listener(|this, _, window, cx| this.set_tab(Tab::Browse, window, cx)))),
                    )
                }),
        )
}

// ---------- Browse / Modpacks ----------

fn browse(me: &mut ModNest, st: &Value, cx: &mut Context<ModNest>) -> impl IntoElement {
    let g = &st["game"];
    let sources: Vec<String> = arr(g, "sources").iter().filter_map(|x| x.as_str().map(String::from)).collect();
    let packs = me.tab == Tab::Modpacks;
    let Some(bw) = me.browse.clone() else {
        return div()
            .px(px(24.))
            .py(px(48.))
            .flex()
            .justify_center()
            .text_color(t::muted())
            .child(format!("No online mods for {}. Install from a file in the Play menu.", s(g, "name")))
            .into_any_element();
    };
    let source = bw.source.clone();
    let cats = me.filters.get(&format!("{}|{}", s(g, "id"), source)).map(|f| arr(f, "categories").to_vec()).unwrap_or_default();
    let needs_key = source == "cf" && s(&me.settings, "curseforgeKey").is_empty();
    let sorts: Vec<(&str, &str)> = if source == "own" { vec![] } else { vec![("most-downloaded", "Most downloaded"), ("top-rated", if source == "cf" { "Top rated" } else { "Most liked" }), ("last-updated", "Recently updated"), ("newest", "Newest")] };
    let sort_label = sorts.iter().find(|(k, _)| *k == bw.ordering).map(|(_, v)| *v).unwrap_or("Most downloaded");
    let cat_label = cats.iter().find(|c| s(c, "id") == bw.category).map(|c| s(c, "name")).unwrap_or_else(|| "All categories".into());
    let noun = if packs { "modpacks" } else { "mods" };
    let grid = me.layout_grid;
    let src_opts: Vec<(&str, &str)> = sources.iter().map(|x| (x.as_str(), source_name(x))).collect();
    let select = |id: &'static str, label: String, menu: Menu, cx: &mut Context<ModNest>| {
        div()
            .relative()
            .child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t::line())
                    .bg(t::surface())
                    .text_color(t::strong())
                    .cursor_pointer()
                    .child(label)
                    .child(icon("caret", 16., t::muted()))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.toggle_menu(menu, cx) })),
            )
            .when(me.menu == Some(menu), |d| {
                let items: Vec<(String, String)> = if menu == Menu::Sort { sorts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() } else { std::iter::once((String::new(), "All categories".to_string())).chain(cats.iter().map(|c| (s(c, "id"), s(c, "name")))).collect() };
                let current = if menu == Menu::Sort { bw.ordering.clone() } else { bw.category.clone() };
                d.child(deferred(
                    menu_pop(t::BTN_H + 6.).left_0().right_auto().max_h(px(360.)).id(SharedString::from(format!("{id}-pop"))).overflow_y_scroll().children(items.into_iter().map(|(k, v)| {
                        let k2 = k.clone();
                        menu_item(SharedString::from(format!("{id}-{k}"))).child(tick(k == current)).child(v).on_click(cx.listener(move |this, _, window, cx| this.pick_select(menu, k2.clone(), window, cx)))
                    })),
                ).with_priority(2))
            })
    };
    div()
        .flex()
        .flex_col()
        .child(
            toolbar()
                .when(sources.len() > 1, |d| d.child(seg("src", &src_opts, &source, cx, |this, v, cx| this.pick_source(v.to_string(), cx))))
                .child(search_box(&me.browse_q))
                .when(!sorts.is_empty(), |d| d.child(select("sort", sort_label.to_string(), Menu::Sort, cx)))
                .when(!cats.is_empty() && !packs, |d| d.child(select("cat", cat_label, Menu::Category, cx)))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .border_1()
                        .border_color(t::line())
                        .rounded(px(8.))
                        .overflow_hidden()
                        .child(div().id("lay-list").w(px(34.)).h(px(32.)).flex().items_center().justify_center().cursor_pointer().bg(if grid { t::surface() } else { t::surface_3() }).child(icon("list", 16., if grid { t::muted() } else { t::strong() })).on_click(cx.listener(|this, _, _, cx| { this.layout_grid = false; this.save_prefs(); cx.notify(); })))
                        .child(div().id("lay-grid").w(px(34.)).h(px(32.)).flex().items_center().justify_center().cursor_pointer().bg(if grid { t::surface_3() } else { t::surface() }).child(icon("grid", 16., if grid { t::strong() } else { t::muted() })).on_click(cx.listener(|this, _, _, cx| { this.layout_grid = true; this.save_prefs(); cx.notify(); }))),
                )
                .when(bw.total > 0, |d| d.child(div().text_color(t::muted()).text_size(px(12.)).ml(px(4.)).child(format!("{} {noun}", fmt_num(bw.total))))),
        )
        .child(if needs_key {
            note(Tone::Blue).child("CurseForge needs an API key.").child(button("cfkey", Btn::Primary, true).child("Add key").on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))).into_any_element()
        } else if packs {
            note(Tone::Blue).child(format!("Modpacks install as a new profile. “{}” stays as it is.", so(g, "activeProfile").unwrap_or_else(|| "Default".into()))).into_any_element()
        } else {
            notes(me, st, cx).unwrap_or_else(|| div().into_any_element())
        })
        .child(if grid {
            let inner = me.main_w - 48.;
            let n = grid_cols(inner, 180., 12.);
            let w = (inner - 12. * (n as f32 - 1.)) / n as f32;
            div().grid().grid_cols(n).gap(px(12.)).px(px(24.)).pt(px(6.)).pb(px(28.)).children(bw.items.iter().map(|m| browse_card(me, m, packs, w, cx))).into_any_element()
        } else {
            div().flex().flex_col().gap(px(4.)).px(px(24.)).pt(px(6.)).pb(px(28.)).children(bw.items.iter().map(|m| browse_row(me, m, packs, cx))).into_any_element()
        })
        .when(bw.loading, |d| d.child(div().flex().justify_center().pb(px(28.)).pt(px(4.)).child(spinner(26.))))
        // more pages load by themselves as you scroll (maybe_load_more); the button is only a fallback after an error
        .when(!bw.loading && bw.has_more && !bw.error.is_empty(), |d| d.child(div().flex().justify_center().pb(px(28.)).pt(px(4.)).child(button("more", Btn::Normal, false).child("Try again").on_click(cx.listener(|this, _, window, cx| {
            if let Some(bw) = this.browse.as_mut() {
                bw.error.clear();
            }
            this.load_more(window, cx)
        })))))
        .when(!bw.loading && bw.loaded && bw.items.is_empty() && !needs_key, |d| d.child(div().px(px(24.)).py(px(48.)).flex().justify_center().text_color(t::muted()).child(if bw.error.is_empty() { format!("No {noun} found on {}.", source_name(&source)) } else { bw.error.clone() })))
        .into_any_element()
}

fn get_button(me: &ModNest, m: &Value, packs: bool, cx: &mut Context<ModNest>) -> AnyElement {
    let key = s(m, "key");
    let busy = me.busy.contains(&key);
    let can = me.state.as_ref().map(|st| b(&st["game"], "installed")).unwrap_or(false) && !busy;
    if b(m, "installed") {
        return pill("Installed", Tone::Green).into_any_element();
    }
    if m.get("downloadable").and_then(|x| x.as_bool()) == Some(false) {
        let url = s(m, "pageUrl");
        return button(SharedString::from(format!("cfp-{key}")), Btn::Normal, true).child("On CurseForge ↗").on_click(cx.listener(move |this, _, _, cx| this.open_url(url.clone(), cx))).into_any_element();
    }
    let title = s(m, "title");
    let k = key.clone();
    button(SharedString::from(format!("get-{key}")), Btn::Primary, true)
        .when(!can, |d| d.opacity(0.45))
        .when(busy, |d| d.child(spinner(13.)))
        .when(!busy && packs, |d| d.child("Install as profile"))
        .when(!busy && !packs, |d| d.child(icon("dl", 14., white())).child("Get"))
        .when(can, |d| d.on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            if packs { this.install_pack(k.clone(), title.clone(), window, cx) } else { this.install(k.clone(), window, cx) }
        })))
        .into_any_element()
}

fn browse_row(me: &ModNest, m: &Value, packs: bool, cx: &mut Context<ModNest>) -> AnyElement {
    let key = s(m, "key");
    let ok = format!("b:{key}");
    let open = me.open.contains(&ok);
    let k2 = ok.clone();
    let page = so(m, "pageUrl").or_else(|| so(m, "website"));
    row_shell(open)
        .child(
            div()
                .id(SharedString::from(format!("br-{key}")))
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(10.))
                .py(px(6.))
                .min_h(px(t::ROW_MIN_H))
                .cursor_pointer()
                .child(mod_icon(m))
                .child(div().flex().flex_col().flex_1().min_w_0().child(div().flex().items_baseline().gap(px(6.)).min_w_0().child(div().font_weight(t::W650).text_color(t::strong()).min_w_0().overflow_hidden().whitespace_nowrap().child(s(m, "title"))).child(div().text_color(t::muted()).text_size(px(12.)).whitespace_nowrap().child(format!("by {}", s(m, "author"))))).child(div().text_color(t::muted()).text_size(px(12.5)).ellipsis().child(s(m, "description"))))
                .child(div().flex().flex_none().gap(px(4.)).when(b(m, "required"), |d| d.child(pill("Base", Tone::Blue))).when(b(m, "deprecated"), |d| d.child(pill("Deprecated", Tone::Red))))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(14.))
                        .min_w(px(120.))
                        .justify_end()
                        .text_color(t::muted())
                        .text_size(px(12.))
                        .when(m.get("downloads").map(|x| !x.is_null()).unwrap_or(false), |d| d.child(div().flex().items_center().gap(px(4.)).child(icon("dl", 13., t::muted())).child(fmt_num(n(m, "downloads")))))
                        .when(m.get("rating").map(|x| !x.is_null()).unwrap_or(false), |d| d.child(div().flex().items_center().gap(px(4.)).child(icon("like", 13., t::muted())).child(fmt_num(n(m, "rating")))))
                        .child(if !s(m, "updated").is_empty() { ago(&s(m, "updated")) } else if !s(m, "version").is_empty() { format!("v{}", s(m, "version")) } else { String::new() }),
                )
                .child(div().flex_none().child(get_button(me, m, packs, cx)))
                .child(icon("chev", 20., t::muted()).when(open, |d| d.with_transformation(Transformation::rotate(radians(std::f32::consts::PI)))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.open.remove(&k2) {
                        this.open.insert(k2.clone());
                    }
                    cx.notify();
                })),
        )
        .when(open, |d| {
            d.child(
                div()
                    .pl(px(62.))
                    .pr(px(14.))
                    .pt(px(2.))
                    .pb(px(12.))
                    .child(div().mb(px(10.)).mt(px(4.)).text_color(t::text()).child(s(m, "description")))
                    .child(div().flex().flex_wrap().gap(px(6.)).children(arr(m, "categories").iter().filter_map(|c| c.as_str()).filter(|c| !c.is_empty()).map(|c| pill(c.to_string(), Tone::Line))).when_some(page, |d, url| {
                        d.child(button(SharedString::from(format!("bp-{key}")), Btn::Ghost, true).child("Mod page ↗").on_click(cx.listener(move |this, _, _, cx| this.open_url(url.clone(), cx))))
                    })),
            )
        })
        .into_any_element()
}

fn browse_card(me: &ModNest, m: &Value, packs: bool, w: f32, cx: &mut Context<ModNest>) -> AnyElement {
    let key = s(m, "key");
    // .thumb: square, full card width
    let thumb: AnyElement = match so(m, "icon") {
        Some(url) => picture(&url).w(px(w - 2.)).h(px(w - 2.)).object_fit(ObjectFit::Cover).bg(t::surface_3()).into_any_element(),
        None => letter_icon(&s(m, "title"), w - 2., 0., 42.).font_weight(FontWeight::EXTRA_BOLD).into_any_element(),
    };
    let has_dl = m.get("downloads").map(|x| !x.is_null()).unwrap_or(false);
    div()
        .id(SharedString::from(format!("bc-{key}")))
        .relative()
        .min_w_0()
        .bg(t::surface())
        .border_1()
        .border_color(t::line_soft())
        .rounded(px(t::RADIUS))
        .overflow_hidden()
        .flex()
        .flex_col()
        .hover(|s| s.border_color(t::line()).top(px(-2.)))
        .child(thumb)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .px(px(11.))
                .pt(px(9.))
                .pb(px(11.))
                .child(div().w_full().font_weight(t::W650).text_color(t::strong()).ellipsis().child(s(m, "title")))
                .child(div().mt(px(4.)).text_color(t::muted()).text_size(px(12.)).child(format!("by {}", s(m, "author"))))
                // -webkit-line-clamp: 2; min-height: 2.9em
                .child(div().mt(px(4.)).w_full().min_h(px(34.8)).text_color(t::muted()).text_size(px(12.)).overflow_hidden().text_ellipsis().line_clamp(2).child(s(m, "description")))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .mt_auto()
                        .pt(px(10.))
                        .child(div().flex().items_center().gap(px(4.)).text_color(t::muted()).text_size(px(12.)).when(has_dl, |d| d.child(icon("dl", 12., t::muted())).child(fmt_num(n(m, "downloads")))).when(!has_dl && !s(m, "version").is_empty(), |d| d.child(format!("v{}", s(m, "version")))))
                        .child(get_button(me, m, packs, cx)),
                ),
        )
        .into_any_element()
}

// ---------- Profiles tab ----------

/// A profile card (.pcard) without its buttons.
pub fn profile_card(p: &Value) -> Div {
    let active = b(p, "active");
    let mods: Vec<String> = arr(p, "mods").iter().filter_map(|m| m.as_str().map(String::from)).collect();
    div()
        .min_w_0()
        .bg(t::surface())
        .border_1()
        .relative()
        .border_color(if active { rgba(0x4f8cff8c) } else { t::line_soft() })
        // .pcard.active also has an inset 1px ring at .25
        .when(active, |d| d.child(div().absolute().inset_0().rounded(px(t::RADIUS - 1.)).border_1().border_color(rgba(0x4f8cff40))))
        .rounded(px(t::RADIUS))
        .px(px(14.))
        .py(px(12.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().overflow_hidden().child(div().w_full().font_weight(FontWeight::BOLD).text_color(t::strong()).ellipsis().child(s(p, "name"))))
                .when(active, |d| d.child(pill("In use", Tone::Blue)))
                .when(!p["modpack"].is_null(), |d| d.child(pill("Modpack", Tone::Line))),
        )
        .child(div().w_full().text_color(t::muted()).text_size(px(12.)).min_h(px(33.6)).overflow_hidden().text_ellipsis().line_clamp(2).child(if mods.is_empty() { "Empty".to_string() } else { mods.join(" · ") }))
}

fn profiles_tab(me: &mut ModNest, st: &Value, cx: &mut Context<ModNest>) -> impl IntoElement {
    let g = &st["game"];
    let is_ts = s(g, "source") != "own";
    let new_card = |id: &'static str, label: &'static str, cx: &mut Context<ModNest>, on: fn(&mut ModNest, &mut Window, &mut Context<ModNest>)| {
        div()
            .id(id)
            .min_h(px(120.))
            .rounded(px(t::RADIUS))
            .border_1()
            .border_dashed()
            .border_color(t::line())
            .flex()
            .items_center()
            .justify_center()
            .text_color(t::muted())
            .cursor_pointer()
            .hover(|s| s.border_color(t::accent()).text_color(t::accent_hi()))
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
    };
    let mgr: Vec<Value> = me.mgr_all.clone().unwrap_or_default().into_iter().filter(|p| s(p, "gameId") == s(g, "id")).collect();
    div()
        .flex()
        .flex_col()
        .child(div().px(px(24.)).pt(px(8.)).pb(px(4.)).child(div().mt(px(10.)).mb(px(2.)).text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(t::strong()).child("Profiles")).child(div().text_color(t::muted()).text_size(px(12.)).child("Each profile is a separate set of mods.")))
        .child(
            div()
                .grid()
                .grid_cols(grid_cols(me.main_w - 48., 250., 12.))
                .gap(px(12.))
                .px(px(24.))
                .pt(px(10.))
                .pb(px(6.))
                .children(arr(st, "profiles").iter().map(|p| {
                    let name = s(p, "name");
                    let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
                    profile_card(p).child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .when(!b(p, "active"), |d| d.child(button(SharedString::from(format!("use-{name}")), Btn::Primary, true).child("Use").on_click(cx.listener(move |this, _, window, cx| this.switch_profile_ui(n1.clone(), window, cx)))))
                            .child(button(SharedString::from(format!("share-{name}")), Btn::Normal, true).child("Share").on_click(cx.listener(move |this, _, window, cx| this.share_dialog(n2.clone(), window, cx))))
                            .child(button(SharedString::from(format!("ren-{name}")), Btn::Ghost, true).child("Rename").on_click(cx.listener(move |this, _, window, cx| this.rename_dialog(n3.clone(), window, cx))))
                            .child(button(SharedString::from(format!("del-{name}")), Btn::Ghost, true).child("Delete").on_click(cx.listener(move |this, _, window, cx| this.delete_profile_dialog(n4.clone(), window, cx)))),
                    )
                }))
                .child(new_card("new-empty", "+ New empty profile", cx, |this, window, cx| this.new_profile_dialog(window, cx)))
                .child(new_card("copy-current", "+ Copy current profile", cx, |this, window, cx| this.save_as_dialog(window, cx))),
        )
        .child(
            div()
                .mx(px(24.))
                .my(px(10.))
                .bg(t::surface())
                .border_1()
                .border_color(t::line_soft())
                .rounded(px(t::RADIUS))
                .px(px(16.))
                .py(px(14.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(t::strong()).mb(px(3.)).child("Import a profile"))
                .child(div().text_color(t::muted()).text_size(px(12.)).child(format!("Paste a ModNest code{}.", if is_ts { ", or a Thunderstore / r2modman profile code" } else { "" })))
                .child(div().mt(px(10.)).h(px(52.)).rounded(px(8.)).border_1().border_color(t::line()).bg(t::surface()).font_family("Consolas").text_size(px(12.)).child(Input::new(&me.code_in).appearance(false).h_full()))
                .child(div().flex().mt(px(10.)).child(button("import-pasted", Btn::Primary, false).child("Import").on_click(cx.listener(|this, _, window, cx| this.import_pasted(window, cx))))),
        )
        .when(!mgr.is_empty(), |d| {
            d.child(
                div()
                    .mx(px(24.))
                    .my(px(10.))
                    .bg(t::surface())
                    .border_1()
                    .border_color(t::line_soft())
                    .rounded(px(t::RADIUS))
                    .px(px(16.))
                    .py(px(14.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(t::strong()).mb(px(3.)).child("Found in Thunderstore Mod Manager"))
                    .child(div().text_color(t::muted()).text_size(px(12.)).child("From another mod manager on this PC."))
                    .children(mgr.iter().map(|p| {
                        let name = s(p, "name");
                        let n2 = name.clone();
                        div().flex().items_center().gap(px(8.)).mt(px(10.)).child(div().font_weight(FontWeight::BOLD).child(name.clone())).child(div().flex_1().text_color(t::muted()).text_size(px(12.)).child(plural(n(p, "count") as usize, "mod"))).child(button(SharedString::from(format!("imgr-{name}")), Btn::Normal, true).child("Import").on_click(cx.listener(move |this, _, window, cx| this.import_manager(n2.clone(), window, cx))))
                    })),
            )
        })
        .child(div().h(px(20.)))
}
