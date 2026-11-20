//! Rail, Library and My stuff (matches .rail, .lib-head, .cover-grid, .cover-card … in styles.css).
use crate::app::{LibLine, ModNest, View};
use crate::theme as t;
use crate::widgets::*;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::input::Input;
use serde_json::Value;

pub fn loading() -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .text_color(t::muted())
        .child(spinner(26.))
        .child("Looking for your Steam games…")
}

/// Spinner (.spinner): a ring with a blue top segment. Drawn as a partial border.
pub fn spinner(size: f32) -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(icon("ring", size, t::line()).absolute().top_0().left_0())
        .child(
            icon("arc", size, t::accent())
                .absolute()
                .top_0()
                .left_0()
                .with_animation("spin", Animation::new(std::time::Duration::from_millis(800)).repeat(), |s, f| s.with_transformation(Transformation::rotate(percentage(f)))),
        )
}

// ---------- rail (.rail) ----------

pub fn rail(me: &ModNest, cx: &mut Context<ModNest>) -> impl IntoElement {
    let mine: Vec<&Value> = me.games.iter().filter(|g| b(g, "installed") && (s(g, "source") == "own" || n(g, "modCount") > 0)).collect();
    let btn = |id: SharedString, active: bool, content: AnyElement| {
        let inner = div()
            .id(id)
            .relative()
            .size(px(t::RAIL_BTN))
            .flex_none()
            .rounded(px(if active { 9. } else { 12. }))
            .overflow_hidden()
            .bg(t::surface_2())
            .flex()
            .items_center()
            .justify_center()
            .text_color(t::muted())
            .cursor_pointer()
            .hover(|s| s.rounded(px(9.)).text_color(t::strong()))
            .child(content);
        // .rail-btn.active: box-shadow 0 0 0 2px rail, 0 0 0 4px accent (drawn outside, takes no space).
        div()
            .relative()
            .flex_none()
            .size(px(t::RAIL_BTN))
            .when(active, |d| {
                d.child(div().absolute().top(px(-4.)).left(px(-4.)).size(px(t::RAIL_BTN + 8.)).rounded(px(13.)).bg(t::accent()))
                    .child(div().absolute().top(px(-2.)).left(px(-2.)).size(px(t::RAIL_BTN + 4.)).rounded(px(11.)).bg(t::rail()))
            })
            .child(inner)
    };
    div()
        .w(px(t::RAIL_W))
        .flex_none()
        .h_full()
        .bg(t::rail())
        .border_r_1()
        .border_color(t::line_soft())
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .py(px(12.))
        .child(img(std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, crate::assets::LOGO_PNG.to_vec()))).size(px(40.)).rounded(px(11.)))
        .child(div().w(px(28.)).h(px(2.)).rounded(px(2.)).bg(t::line()).my(px(2.)))
        .child(btn("rail-lib".into(), me.view == View::Library, 
            div()
                .id("rail-lib-in")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(icon("library", 20., t::muted()))
                .on_click(cx.listener(|this, _, _, cx| this.show_library(cx)))
                .into_any_element(),
        ))
        .child(btn("rail-mine".into(), me.view == View::Mine, 
            div()
                .id("rail-mine-in")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(icon("person", 20., t::muted()))
                .on_click(cx.listener(|this, _, _, cx| this.show_mine(cx)))
                .into_any_element(),
        ))
        .child(
            div()
                .id("rail-games")
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .py(px(2.))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(mine.into_iter().map(|g| {
                    let id = s(g, "id");
                    let active = me.view == View::Game && me.game_id.as_deref() == Some(id.as_str());
                    let art = so(g, "capsule").or_else(|| so(g, "cover")).unwrap_or_default();
                    let gid = id.clone();
                    btn(SharedString::from(format!("rail-{id}")), active, 
                        div()
                            .id(SharedString::from(format!("rail-in-{id}")))
                            .size_full()
                            .child(fitted(&art, t::RAIL_BTN, t::RAIL_BTN, if so(g, "capsule").is_some() { 2. / 3. } else { 460. / 215. }, 0.15).child(corners(if active { 9. } else { 12. }, t::RAIL_BTN, t::rail())))
                            .on_click(cx.listener(move |this, _, window, cx| this.open_game(gid.clone(), crate::app::Tab::Installed, window, cx)))
                            .into_any_element(),
                    )
                })),
        )
        .child(btn("rail-set".into(), false, 
            div()
                .id("rail-set-in")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(icon("gear", 20., t::muted()))
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
                .into_any_element(),
        ))
}

// ---------- library ----------

/// A scrolling area with a WebView-style scrollbar in a 10px gutter on the right.
/// Like overflow-y: auto, the gutter is only there while the content is taller than the area.
pub fn scroll_area<H: gpui_component::scroll::ScrollbarHandle + Clone>(content: impl IntoElement, handle: &H, overflow: bool) -> Div {
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .child(div().flex_1().min_w_0().h_full().child(content))
        .when(overflow, |d| {
            d.child(div().w(px(t::SCROLLBAR)).flex_none())
                .child(div().absolute().top_0().bottom_0().right(px(-3.)).w(px(16.)).child(gpui_component::scroll::Scrollbar::vertical(handle)))
        })
}

pub fn seg(id: &str, options: &[(&str, &str)], active: &str, cx: &mut Context<ModNest>, on: fn(&mut ModNest, &str, &mut Context<ModNest>)) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .p(px(3.))
        .rounded(px(9.))
        .bg(t::surface())
        .border_1()
        .border_color(t::line())
        .children(options.iter().map(|(value, label)| {
            let is = *value == active;
            let v = value.to_string();
            div()
                .id(SharedString::from(format!("{id}-{value}")))
                .px(px(12.))
                .py(px(5.))
                .rounded(px(6.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(13.))
                .cursor_pointer()
                .when(is, |d| d.bg(t::surface_3()).text_color(t::strong()))
                .when(!is, |d| d.text_color(t::muted()))
                .child(label.to_string())
                .on_click(cx.listener(move |this, _, _, cx| on(this, &v, cx)))
        }))
}

/// Search box (.search): 34px input with a magnifier at left.
pub fn search_box(input: &Entity<gpui_component::input::InputState>) -> Div {
    div()
        .relative()
        .flex()
        .flex_1()
        .min_w(px(200.))
        .h(px(34.))
        .rounded(px(8.))
        .border_1()
        .border_color(t::line())
        .bg(t::surface())
        .items_center()
        .pl(px(22.))
        .pr(px(12.))
        .child(div().absolute().left(px(11.)).top(px(9.)).child(icon("search", 14., t::muted())))
        .child(Input::new(input).appearance(false).text_color(t::strong()))
}

pub fn render(me: &mut ModNest, main_w: f32, cx: &mut Context<ModNest>) -> impl IntoElement {
    let cols = me.lib_cols.max(1);
    let card_w = ((main_w - 2.0 * t::LIB_PAD_X - 16.0 * (cols as f32 - 1.0)) / cols as f32).floor();
    let rows = list(me.lib_list.clone(), cx.processor(move |this: &mut ModNest, ix: usize, _window, cx| {
        let Some(line) = this.lib_lines.get(ix).cloned() else { return div().into_any_element() };
        let row: AnyElement = match line {
            LibLine::Head => div().into_any_element(),
            LibLine::Group(title, p) => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pt(px(18.))
                .pb(px(10.))
                .px(px(t::LIB_PAD_X))
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(t::muted())
                .child(title)
                .when_some(p, |d, p| d.child(pill(p, Tone::Line)))
                .into_any_element(),
            LibLine::Covers(items, add, last) => div()
                .flex()
                .gap(px(16.))
                .px(px(t::LIB_PAD_X))
                .pb(px(if last { 10. } else { 16. }))
                .children(items.iter().map(|&i| cover_card(this, i, card_w, cx).into_any_element()))
                .when(add, |d| d.child(add_card(card_w, cx)))
                .into_any_element(),
            LibLine::Note("empty") => div()
                .px(px(24.))
                .py(px(48.))
                .flex()
                .justify_center()
                .text_color(t::muted())
                .child(format!("No games found{}.", if this.lib_all { "" } else { ". Try “All supported”" }))
                .into_any_element(),
            LibLine::Note(_) => note(Tone::Blue)
                .mx(px(32.))
                .my(px(10.))
                .child("Add a CurseForge key to see CurseForge games.")
                .child(button("cf-key", Btn::Normal, true).child("Add key").on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))))
                .into_any_element(),
            LibLine::Spacer => div().h(px(24.)).into_any_element(),
        };
        div().w_full().child(row).into_any_element()
    }))
    .size_full();
    div().size_full().flex().flex_col().child(lib_head(me, cx)).child(scroll_area(rows, &me.lib_list, me.overflow))
}

fn lib_head(me: &ModNest, cx: &mut Context<ModNest>) -> impl IntoElement {
    let total = me.games.len();
    div()
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(16.))
                .pt(px(26.))
                .pb(px(14.))
                .px(px(t::LIB_PAD_X))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(t::strong()).line_height(relative(1.45)).child("Library"))
                        .child(div().mt(px(2.)).text_color(t::muted()).child(if me.lib_all { format!("{total} games") } else { "Installed games".into() })),
                )
                .child(div().flex_1())
                .child(seg("scope", &[("mine", "Installed"), ("all", "All supported")], if me.lib_all { "all" } else { "mine" }, cx, |this, v, cx| {
                    this.lib_all = v == "all";
                    this.lib_pref = Some(this.lib_all);
                    this.save_prefs();
                    this.rebuild_library(cx);
                    cx.notify();
                })),
        )
        .child(div().flex().px(px(t::LIB_PAD_X)).pb(px(6.)).child(search_box(&me.lib_filter)))
}

fn cover_card(me: &ModNest, i: usize, w: f32, cx: &mut Context<ModNest>) -> impl IntoElement {
    let g = &me.games[i];
    let id = s(g, "id");
    let installed = b(g, "installed");
    let art = so(g, "capsule").or_else(|| so(g, "cover")).unwrap_or_default();
    let h = (w * 1.5).round();
    let mods = n(g, "modCount") as usize;
    let gid = id.clone();
    div()
        .id(SharedString::from(format!("cover-{id}")))
        .relative()
        .flex_none()
        .w(px(w))
        .h(px(h))
        .rounded(px(t::RADIUS))
        .overflow_hidden()
        .bg(t::surface_2())
        .cursor_pointer()
        // .cover-card:hover: lift 3px, shadow and a 2px accent ring
        .hover(|s| {
            s.top(px(-3.)).shadow(vec![
                BoxShadow { color: hsla(0., 0., 0., 0.45), offset: point(px(0.), px(10.)), blur_radius: px(24.), spread_radius: px(0.) },
                BoxShadow { color: t::accent().into(), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(2.) },
            ])
        })
        // fallback name (visible if there's no art)
        .child(div().absolute().inset_0().flex().items_center().justify_center().p(px(12.)).text_center().font_weight(FontWeight::BOLD).text_color(t::muted()).child(s(g, "name")))
        .child(picture(&art).absolute().inset_0().size_full().rounded(px(t::RADIUS)).object_fit(ObjectFit::Cover).when(!installed, |i| i.grayscale(true)))
        .when(!installed, |d| d.child(div().absolute().inset_0().rounded(px(t::RADIUS)).bg(rgba(0x00000073))))
        // .shade: transparent 50% -> rgba(5,6,9,.92)
        .child(div().absolute().inset_0().rounded(px(t::RADIUS)).bg(linear_gradient(180., linear_color_stop(rgba(0x05060900), 0.5), linear_color_stop(rgba(0x050609eb), 1.))))
        .when(mods > 0, |d| d.child(div().absolute().top(px(8.)).right(px(8.)).child(pill(plural(mods, "mod"), Tone::Plain).bg(t::badge_bg()).text_color(white()))))
        .child(
            div()
                .absolute()
                .left(px(10.))
                .right(px(10.))
                .bottom(px(9.))
                .child(div().text_color(white()).font_weight(FontWeight::BOLD).text_size(px(13.)).line_height(relative(1.2)).child(s(g, "name")))
                .child(div().mt(px(2.)).text_size(px(11.5)).text_color(rgba(0xffffffb3)).child(if installed { s(g, "loader") } else { "Not installed".into() })),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.open_game(gid.clone(), crate::app::Tab::Installed, window, cx)))
}

fn add_card(w: f32, cx: &mut Context<ModNest>) -> impl IntoElement {
    div()
        .id("add-game")
        .relative()
        .flex_none()
        .w(px(w))
        .h(px((w * 1.5).round()))
        .rounded(px(t::RADIUS))
        .border_2()
        .border_dashed()
        .border_color(t::line())
        .cursor_pointer()
        .hover(|s| s.border_color(t::accent()))
        .child(div().absolute().top_0().left_0().right_0().bottom(px(w * 0.45)).flex().items_center().justify_center().text_size(px(46.)).font_weight(FontWeight::LIGHT).text_color(t::muted()).child("+"))
        .child(
            div()
                .absolute()
                .left(px(10.))
                .right(px(10.))
                .bottom(px(9.))
                .child(div().text_color(t::strong()).font_weight(FontWeight::BOLD).text_size(px(13.)).child("Add a game"))
                .child(div().mt(px(2.)).text_size(px(11.5)).text_color(t::muted()).child("Any game, any folder")),
        )
        .on_click(cx.listener(|this, _, window, cx| this.add_game_dialog(window, cx)))
}

// ---------- My stuff ----------

pub fn render_mine(me: &mut ModNest, cx: &mut Context<ModNest>) -> impl IntoElement {
    let head = div()
        .flex()
        .flex_none()
        .pt(px(26.))
        .pb(px(14.))
        .px(px(t::LIB_PAD_X))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).text_color(t::strong()).line_height(relative(1.45)).child("My stuff"))
                .child(div().mt(px(2.)).text_color(t::muted()).child("Profiles, modpacks and published mods.")),
        );
    let Some((data, mgr)) = me.mine.clone() else {
        return div().size_full().flex().flex_col().child(head).child(div().flex_1().flex().items_center().justify_center().child(spinner(26.))).into_any_element();
    };
    let cols = grid_cols(me.main_w - 2. * t::LIB_PAD_X, 250., 12.);
    let group_title = |title: &str| div().flex().items_center().gap(px(8.)).pt(px(18.)).pb(px(10.)).px(px(t::LIB_PAD_X)).text_size(px(13.)).font_weight(FontWeight::BOLD).text_color(t::muted()).child(title.to_string());
    let profiles: Vec<AnyElement> = arr(&data, "profiles")
        .iter()
        .map(|g| {
            let gid = s(g, "gameId");
            let gid2 = gid.clone();
            div()
                .mb(px(14.))
                .child(
                    div()
                        .id(SharedString::from(format!("mine-{gid}")))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .py(px(4.))
                        .cursor_pointer()
                        .child(picture(&s(g, "capsule")).w(px(30.)).h(px(45.)).rounded(px(5.)).object_fit(ObjectFit::Cover).bg(t::surface_2()))
                        .child(div().text_color(t::strong()).font_weight(FontWeight::BOLD).text_size(px(15.)).child(s(g, "gameName")))
                        .child(div().text_color(t::muted()).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(plural(arr(g, "profiles").len(), "profile")))
                        .on_click(cx.listener(move |this, _, window, cx| this.open_game(gid2.clone(), crate::app::Tab::Installed, window, cx))),
                )
                .child(
                    div().grid().grid_cols(cols).gap(px(12.)).pt(px(8.)).pb(px(4.)).children(arr(g, "profiles").iter().map(|p| {
                        let name = s(p, "name");
                        let (gid3, name2) = (gid.clone(), name.clone());
                        crate::game::profile_card(p)
                            .child(div().flex().gap(px(6.)).child(button(SharedString::from(format!("op-{gid}-{name}")), Btn::Normal, true).child("Open").on_click(cx.listener(move |this, _, window, cx| this.open_profile_from_mine(gid3.clone(), name2.clone(), window, cx)))))
                            .into_any_element()
                    })),
                )
                .into_any_element()
        })
        .collect();
    let published: Vec<AnyElement> = arr(&data, "published")
        .iter()
        .filter(|g| !arr(g, "packages").is_empty())
        .map(|g| {
            div()
                .mb(px(14.))
                .child(div().flex().items_center().gap(px(10.)).py(px(4.)).child(div().text_color(t::strong()).font_weight(FontWeight::BOLD).text_size(px(15.)).child(s(g, "gameName"))).child(div().text_color(t::muted()).text_size(px(12.)).child(plural(arr(g, "packages").len(), "item"))))
                .child(div().flex().flex_col().gap(px(4.)).py(px(6.)).children(arr(g, "packages").iter().map(|p| {
                    let mut iv = p.clone();
                    iv["title"] = Value::String(s(p, "name"));
                    crate::game::row_shell(false)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .px(px(10.))
                                .py(px(6.))
                                .min_h(px(t::ROW_MIN_H))
                                .child(mod_icon(&iv))
                                .child(div().flex().flex_col().flex_1().min_w_0().child(div().flex().items_baseline().gap(px(6.)).child(div().font_weight(t::W650).text_color(t::strong()).min_w_0().overflow_hidden().whitespace_nowrap().child(s(p, "name"))).child(div().text_color(t::muted()).text_size(px(12.)).child(format!("v{}", s(p, "version"))))).child(div().text_color(t::muted()).text_size(px(12.5)).ellipsis().child(s(p, "description"))))
                                .child(pill(s(p, "kind"), if s(p, "kind") == "Modpack" { Tone::Blue } else { Tone::Line })),
                        )
                        .into_any_element()
                })))
                .into_any_element()
        })
        .collect();
    let repo = s(&data, "repo");
    let (r1, r2) = (repo.clone(), format!("{repo}/blob/main/CONTRIBUTING.md"));
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(head)
        .child(scroll_area(
            div()
                .id("mine-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&me.mine_scroll)
                .pb(px(30.))
                .child(group_title("Your profiles"))
                .child(div().px(px(t::LIB_PAD_X)).children(profiles).when(arr(&data, "profiles").is_empty(), |d| d.child(div().py(px(48.)).flex().justify_center().text_color(t::muted()).child("No profiles yet."))))
                .when(!mgr.is_empty(), |d| {
                    d.child(group_title("In Thunderstore Mod Manager on this PC")).child(div().px(px(t::LIB_PAD_X)).children(mgr.iter().map(|p| {
                        let (gid, name) = (s(p, "gameId"), s(p, "name"));
                        let mods: Vec<String> = arr(p, "mods").iter().filter_map(|m| m.as_str().map(String::from)).take(10).collect();
                        crate::game::row_shell(false)
                            .mb(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .px(px(10.))
                                    .py(px(6.))
                                    .min_h(px(t::ROW_MIN_H))
                                    .child(div().flex().flex_col().flex_1().min_w_0().child(div().flex().items_baseline().gap(px(6.)).child(div().font_weight(FontWeight::SEMIBOLD).text_color(t::strong()).child(name.clone())).child(div().text_color(t::muted()).text_size(px(12.)).child(s(p, "gameName")))).child(div().text_color(t::muted()).text_size(px(12.5)).ellipsis().child(mods.join(" · "))))
                                    .child(div().text_color(t::muted()).text_size(px(12.)).child(plural(n(p, "count") as usize, "mod")))
                                    .child(button(SharedString::from(format!("imp-{gid}-{name}")), Btn::Normal, true).child("Import").on_click(cx.listener(move |this, _, window, cx| this.import_manager_any(gid.clone(), name.clone(), window, cx)))),
                            )
                    })))
                })
                .child(
                    group_title("Published by you")
                        .child(pill("ModNest catalog", Tone::Line))
                        .child(div().id("gh").text_color(t::accent_hi()).font_weight(FontWeight::NORMAL).cursor_pointer().child("GitHub ↗").on_click(cx.listener(move |this, _, _, cx| this.open_url(r1.clone(), cx))))
                        .child(div().id("contrib").text_color(t::accent_hi()).font_weight(FontWeight::NORMAL).cursor_pointer().child("How others can add mods ↗").on_click(cx.listener(move |this, _, _, cx| this.open_url(r2.clone(), cx)))),
                )
                .child(div().px(px(t::LIB_PAD_X)).children(published)),
            &me.mine_scroll,
            me.overflow,
        ))
        .into_any_element()
}
