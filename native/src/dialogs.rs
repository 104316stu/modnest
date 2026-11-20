//! Dialogs (.modal), toasts (.toast) and the update bar (.app-update), matching styles.css.
use crate::actions::Then;
use crate::app::{ModNest, Toast};
use crate::library::spinner;
use crate::theme as t;
use crate::widgets::*;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use serde_json::{json, Value};

pub struct Confirm {
    pub title: String,
    /// Paragraphs; **text** is bold.
    pub body: Vec<String>,
    /// A smaller, muted last paragraph.
    pub small: Option<String>,
    pub ok: String,
    pub on_ok: Option<Then<()>>,
}

pub enum Modal {
    Confirm(Confirm),
    Ask { title: String, label: String, ok: String, input: Entity<InputState>, on_ok: Option<Then<String>>, _sub: Subscription },
    Share { name: String, code: String, is_ts: bool, field: Entity<InputState> },
    Settings { loader: bool, steam: bool, update: bool, cf: Entity<InputState> },
    AddGame { name: Entity<InputState>, loader: &'static str, loader_open: bool, folder: Entity<InputState>, ts: Entity<InputState> },
    GameSettings { folder: Entity<InputState> },
}

const LOADERS: &[(&str, &str)] = &[
    ("auto", "Work it out for me"),
    ("bepinex5", "BepInEx 5 (most Unity games)"),
    ("bepinex6-il2cpp", "BepInEx 6 IL2CPP (newer Unity games)"),
    ("melonloader", "MelonLoader"),
    ("none", "None: copy mod files into a folder"),
];

impl ModNest {
    /// A one-line question (like ask() in app.js). Enter = OK.
    pub fn ask(&mut self, title: &str, label: &str, value: &str, ok: &str, window: &mut Window, cx: &mut Context<Self>, on_ok: Then<String>) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
        let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.modal_ok(window, cx);
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.modal = Some(Modal::Ask { title: title.into(), label: label.into(), ok: ok.into(), input, on_ok: Some(on_ok), _sub: sub });
        cx.notify();
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let key = s(&self.settings, "curseforgeKey");
        let cf = cx.new(|cx| InputState::new(window, cx).placeholder("Paste your key").default_value(key));
        self.modal = Some(Modal::Settings { loader: b(&self.settings, "autoInstallLoader"), steam: b(&self.settings, "steamVanilla"), update: b(&self.settings, "autoUpdateMods"), cf });
        cx.notify();
    }

    pub fn add_game_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("My Game"));
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder("leave empty for the game folder"));
        let ts = cx.new(|cx| InputState::new(window, cx).placeholder("Type a Thunderstore game name"));
        name.update(cx, |s, cx| s.focus(window, cx));
        self.modal = Some(Modal::AddGame { name, loader: "auto", loader_open: false, folder, ts });
        cx.notify();
    }

    pub fn game_settings_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let current = self.state.as_ref().map(|st| s(&st["game"], "modsFolder")).unwrap_or_default();
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder("the game folder itself").default_value(current));
        self.modal = Some(Modal::GameSettings { folder });
        cx.notify();
    }

    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        cx.notify();
    }

    /// The dialog's main button.
    pub fn modal_ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.modal.take() else { return };
        cx.notify();
        match m {
            Modal::Confirm(mut c) => {
                if let Some(f) = c.on_ok.take() {
                    f(self, (), window, cx);
                }
            }
            Modal::Ask { input, mut on_ok, .. } => {
                let v = input.read(cx).value().trim().to_string();
                if let (false, Some(f)) = (v.is_empty(), on_ok.take()) {
                    f(self, v, window, cx);
                }
            }
            Modal::Share { .. } => {}
            Modal::Settings { loader, steam, update, cf } => {
                let key = cf.read(cx).value().trim().to_string();
                self.save_settings(loader, steam, update, key, window, cx);
            }
            Modal::AddGame { name, loader, folder, ts, .. } => {
                let ts_name = ts.read(cx).value().trim().to_lowercase();
                let ts_id = self.games.iter().filter(|g| arr(g, "sources").iter().any(|x| x == "ts")).find(|g| s(g, "name").to_lowercase() == ts_name).and_then(|g| so(g, "tsId"));
                let game = json!({ "name": name.read(cx).value().to_string(), "loader": loader, "modsFolder": folder.read(cx).value().to_string(), "thunderstore": ts_id, "curseforge": null });
                self.add_game(game, window, cx);
            }
            Modal::GameSettings { folder } => {
                let f = folder.read(cx).value().to_string();
                self.save_game_settings(f, window, cx);
            }
        }
    }
}

// ---------- rendering ----------

/// Text with **bold** parts.
fn rich(text: &str) -> StyledText {
    let mut plain = String::new();
    let mut ranges = vec![];
    for (i, part) in text.split("**").enumerate() {
        if i % 2 == 1 {
            ranges.push((plain.len()..plain.len() + part.len(), HighlightStyle { font_weight: Some(FontWeight::BOLD), color: Some(t::strong().into()), ..Default::default() }));
        }
        plain.push_str(part);
    }
    StyledText::new(plain).with_highlights(ranges)
}

/// .input: 34px, the text input brings its own 12px padding.
/// Text with **bold** parts and one [[link]] that runs `on` when clicked.
fn rich_link(id: &'static str, text: &str, cx: &mut Context<ModNest>, on: fn(&mut ModNest, &mut Context<ModNest>)) -> InteractiveText {
    let mut plain = String::new();
    let mut ranges = vec![];
    let mut link = 0..0;
    let mut rest = text;
    while !rest.is_empty() {
        let next = [rest.find("**"), rest.find("[[")].into_iter().flatten().min();
        let Some(i) = next else {
            plain.push_str(rest);
            break;
        };
        plain.push_str(&rest[..i]);
        let bold = rest[i..].starts_with("**");
        let after = &rest[i + 2..];
        let end = after.find(if bold { "**" } else { "]]" }).unwrap_or(after.len());
        let start = plain.len();
        plain.push_str(&after[..end]);
        let r = start..plain.len();
        if bold {
            ranges.push((r, HighlightStyle { font_weight: Some(FontWeight::BOLD), color: Some(t::strong().into()), ..Default::default() }));
        } else {
            link = r.clone();
            ranges.push((r, HighlightStyle { color: Some(t::accent_hi().into()), underline: Some(UnderlineStyle { thickness: px(1.), color: Some(t::accent_hi().into()), wavy: false }), ..Default::default() }));
        }
        rest = &after[(end + 2).min(after.len())..];
    }
    let me = cx.entity().downgrade();
    InteractiveText::new(id, StyledText::new(plain).with_highlights(ranges)).on_click(vec![link], move |_, _, cx| {
        let _ = me.update(cx, |this, cx| on(this, cx));
    })
}

fn field_box(input: &Entity<InputState>) -> Div {
    div().h(px(34.)).text_size(px(14.)).rounded(px(8.)).border_1().border_color(t::line()).bg(t::surface()).text_color(t::strong()).flex().items_center().child(Input::new(input).appearance(false).w_full())
}

fn field(label: impl IntoElement, input: impl IntoElement) -> Div {
    div().flex().flex_col().gap(px(6.)).child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t::strong()).child(label)).child(input)
}

fn label_with(main: &str, note: &str) -> Div {
    div().flex().items_baseline().gap(px(4.)).child(main.to_string()).child(div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(t::muted()).child(note.to_string()))
}

/// Inline code (.modal code).
fn code(s: &str) -> Div {
    div().px(px(4.)).rounded(px(4.)).bg(t::surface_3()).font_family("Consolas").text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(t::text()).child(s.to_string())
}

/// A checkbox row (.modal label.check).
fn check(id: &'static str, on: bool, title: &str, text: &str, cx: &mut Context<ModNest>, flip: fn(&mut Modal)) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .gap(px(10.))
        .items_start()
        .mb(px(12.))
        .cursor_pointer()
        // a dark-theme checkbox with accent-color (margin 4px 3px 3px 4px)
        .child(
            div()
                .mt(px(4.))
                .ml(px(4.))
                .mr(px(3.))
                .size(px(13.))
                .flex_none()
                .rounded(px(2.))
                .flex()
                .items_center()
                .justify_center()
                .when(on, |d| d.bg(t::accent()).child(icon("check", 11., rgb(0x111318))))
                .when(!on, |d| d.bg(rgb(0x3b3b3b)).border_1().border_color(rgb(0x858585))),
        )
        .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::BOLD).text_color(t::strong()).child(title.to_string())).child(div().text_size(px(12.)).text_color(t::muted()).child(text.to_string())))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(m) = this.modal.as_mut() {
                flip(m);
            }
            cx.notify();
        }))
}

pub fn modal(me: &mut ModNest, _window: &mut Window, cx: &mut Context<ModNest>) -> impl IntoElement {
    let footer = |extra: Option<AnyElement>, ok: &str, cx: &mut Context<ModNest>| {
        div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .px(px(18.))
            .pt(px(12.))
            .pb(px(16.))
            .when_some(extra, |d, e| d.child(e))
            .child(button("m-cancel", Btn::Normal, false).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_modal(cx))))
            .child(button("m-ok", Btn::Primary, false).child(ok.to_string()).on_click(cx.listener(|this, _, window, cx| this.modal_ok(window, cx))))
    };
    let header = |title: String| div().px(px(18.)).pt(px(16.)).pb(px(4.)).text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(t::strong()).child(title);
    let section = || div().px(px(18.)).pt(px(10.)).pb(px(14.)).flex().flex_col();
    let content: AnyElement = match me.modal.as_ref() {
        None => div().into_any_element(),
        Some(Modal::Confirm(c)) => div()
            .child(header(c.title.clone()))
            .child(section().gap(px(14.)).pt(px(24.)).pb(px(28.)).children(c.body.iter().map(|p| div().child(rich(p)))).when_some(c.small.clone(), |d, sm| d.child(div().text_size(px(12.)).text_color(t::muted()).child(sm))))
            .child(footer(None, &c.ok, cx))
            .into_any_element(),
        Some(Modal::Ask { title, label, ok, input, .. }) => div()
            .child(header(title.clone()))
            .child(section().child(div().flex().flex_col().gap(px(6.)).child(div().text_size(px(12.)).text_color(t::muted()).child(label.clone())).child(field_box(input))))
            .child(footer(None, ok, cx))
            .into_any_element(),
        Some(Modal::Share { name, code, is_ts, field }) => {
            let (n1, c1, is_ts) = (name.clone(), code.clone(), *is_ts);
            div()
                .child(header(format!("Share “{name}”")))
                .child(
                    section()
                        .child(div().text_size(px(12.)).text_color(t::muted()).mb(px(12.)).child(rich("Copied. Your friend pastes it under **Import a code** in the profile menu.")))
                        .child(div().h(px(92.)).rounded(px(8.)).border_1().border_color(t::line()).bg(t::surface()).px(px(12.)).py(px(8.)).font_family("Consolas").text_size(px(12.)).text_color(t::strong()).child(Input::new(field).appearance(false).h_full()))
                        .when(is_ts, |d| d.child(div().mt(px(12.)).text_size(px(12.)).text_color(t::muted()).child("For r2modman or Thunderstore Mod Manager, use a Thunderstore code."))),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .px(px(18.))
                        .pt(px(12.))
                        .pb(px(16.))
                        .when(is_ts, |d| {
                            d.child(button("m-ts", Btn::Normal, false).child("Get Thunderstore code").on_click(cx.listener(move |this, _, window, cx| {
                                this.modal = None;
                                this.share_thunderstore(n1.clone(), window, cx);
                            })))
                        })
                        .child(button("m-copy", Btn::Normal, false).child("Copy again").on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(c1.clone()));
                            this.modal = None;
                            this.toast("cp", "Code copied", "", "ok", None, 2000);
                            cx.notify();
                        })))
                        .child(button("m-done", Btn::Primary, false).child("Done").on_click(cx.listener(|this, _, _, cx| this.close_modal(cx)))),
                )
                .into_any_element()
        }
        Some(Modal::Settings { loader, steam, update, cf }) => {
            let version = me.version.clone();
            div()
                .child(header("Settings".into()))
                .child(
                    section()
                        .child(check("set-loader", *loader, "Install mod loaders automatically", "Installs BepInEx and other loaders when needed.", cx, |m| {
                            if let Modal::Settings { loader, .. } = m {
                                *loader = !*loader
                            }
                        }))
                        .child(check("set-steam", *steam, "Play with mods only from ModNest", "Steam starts the game without mods. BepInEx games only.", cx, |m| {
                            if let Modal::Settings { steam, .. } = m {
                                *steam = !*steam
                            }
                        }))
                        .child(check("set-update", *update, "Auto-update mods", "Updates mods when you open a game.", cx, |m| {
                            if let Modal::Settings { update, .. } = m {
                                *update = !*update
                            }
                        }))
                        .child(
                            div()
                                .mt(px(14.))
                                .mb(px(6.))
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(div().flex().items_baseline().gap(px(4.)).text_size(px(13.)).child(div().font_weight(FontWeight::BOLD).text_color(t::strong()).child("CurseForge API key")).child(div().text_size(px(12.)).text_color(t::muted()).child("· needed to browse CurseForge")))
                                .child(field_box(cf))
                                .child(div().text_size(px(12.)).text_color(t::muted()).child(rich_link("cf-link", "Get a free key at [[console.curseforge.com]] → **API keys**.", cx, |this, cx| this.open_url("https://console.curseforge.com/".into(), cx)))),
                        )
                        .child(div().mt(px(13.)).text_size(px(12.)).text_color(t::muted()).child(rich_link("clear-cache", "[[Clear download cache]]", cx, |this, cx| this.clear_cache(cx))))
                        .child(div().mt(px(13.)).text_size(px(12.)).text_color(t::muted()).child(format!("ModNest v{version}"))),
                )
                .child(footer(None, "Save", cx))
                .into_any_element()
        }
        Some(Modal::AddGame { name, loader, loader_open, folder, ts }) => {
            let current = LOADERS.iter().find(|(k, _)| k == loader).map(|(_, v)| *v).unwrap_or("");
            let open = *loader_open;
            div()
                .child(header("Add a game".into()))
                .child(
                    section()
                        .gap(px(10.))
                        .child(div().text_size(px(12.)).text_color(t::muted()).child(rich("Then pick the folder with the game's .exe.")))
                        .child(field("Name", field_box(name)))
                        .child(field(
                            "Mod loader",
                            div()
                                .relative()
                                .child(
                                    div()
                                        .id("ng-loader")
                                        .h(px(34.))
                                        .px(px(12.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(if open { t::accent() } else { t::line() })
                                        .bg(t::surface())
                                        .text_color(t::strong())
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .cursor_pointer()
                                        .child(current)
                                        .child(icon("caret", 16., t::muted()))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(Modal::AddGame { loader_open, .. }) = this.modal.as_mut() {
                                                *loader_open = !*loader_open;
                                            }
                                            cx.notify();
                                        })),
                                )
                                .when(open, |d| {
                                    d.child(deferred(
                                        div().absolute().top(px(38.)).left_0().right_0().bg(t::surface_2()).border_1().border_color(t::line()).rounded(px(8.)).p(px(4.)).occlude().children(LOADERS.iter().map(|(k, v)| {
                                            let k: &'static str = k;
                                            div()
                                                .id(k)
                                                .px(px(10.))
                                                .py(px(6.))
                                                .rounded(px(6.))
                                                .cursor_pointer()
                                                .hover(|s| s.bg(t::surface_3()))
                                                .child(*v)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(Modal::AddGame { loader, loader_open, .. }) = this.modal.as_mut() {
                                                        *loader = k;
                                                        *loader_open = false;
                                                    }
                                                    cx.notify();
                                                }))
                                        })),
                                    ))
                                }),
                        ))
                        .child(field(
                            div().flex().flex_wrap().items_baseline().gap(px(4.)).child("Mods folder").child(
                                div().flex().items_baseline().gap(px(4.)).text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(t::muted()).child("(only for “None”, e.g.").child(code("Mods")).child("or").child(code("Content/Paks/~mods")).child(")"),
                            ),
                            field_box(folder),
                        ))
                        .child(field(label_with("Get mods from Thunderstore", "(optional)"), field_box(ts))),
                )
                .child(footer(None, "Pick folder…", cx))
                .into_any_element()
        }
        Some(Modal::GameSettings { folder }) => {
            let g = me.state.as_ref().map(|st| st["game"].clone()).unwrap_or(Value::Null);
            let loader = s(&g, "loader");
            let plain = !loader.starts_with("BepInEx") && loader != "MelonLoader";
            let sources: Vec<&str> = arr(&g, "sources").iter().filter_map(|x| x.as_str()).map(crate::game::source_name).collect();
            let custom = !g["custom"].is_null();
            let line = |k: &str, v: String| div().flex().gap(px(4.)).child(div().font_weight(FontWeight::BOLD).text_color(t::strong()).child(k.to_string())).child(div().text_color(t::muted()).child(v));
            div()
                .child(header(format!("{} settings", s(&g, "name"))))
                .child(
                    section()
                        .child(div().text_size(px(12.)).mb(px(12.)).child(line("Folder:", so(&g, "path").unwrap_or_else(|| "not found".into()))).child(line("Mod loader:", loader.clone())).child(line("Mods from:", if sources.is_empty() { "files only".into() } else { sources.join(", ") })))
                        .child(field(label_with("Mods folder", &format!("· where plain mod files go{}", if plain { "" } else { " (not used with a mod loader)" })), field_box(folder))),
                )
                .child(footer(
                    custom.then(|| {
                        div()
                            .flex_1()
                            .child(button("m-remove", Btn::Danger, false).child("Remove from ModNest").on_click(cx.listener(|this, _, _, cx| {
                                this.modal = None;
                                this.remove_game_dialog(cx);
                            })))
                            .into_any_element()
                    }),
                    "Save",
                    cx,
                ))
                .into_any_element()
        }
    };
    div()
        .id("modal-backdrop")
        .absolute()
        .inset_0()
        .bg(t::backdrop())
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .child(
            div()
                .id("modal")
                .w(px(500.))
                .max_w(relative(0.92))
                .bg(t::surface())
                .text_color(t::text())
                .border_1()
                .border_color(t::line())
                .rounded(px(14.))
                .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.6), offset: point(px(0.), px(20.)), blur_radius: px(60.), spread_radius: px(0.) }])
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_modal(cx);
                    }
                }))
                .child(content),
        )
}

// ---------- toasts ----------

fn bar(fraction: f64) -> Div {
    let track = div().h(px(4.)).mt(px(7.)).rounded_full().bg(t::surface_3()).overflow_hidden().relative();
    if fraction < 0.0 {
        // indeterminate: a 35% block sliding across
        track.child(div().absolute().top_0().bottom_0().w(relative(0.35)).bg(t::accent()).rounded_full().with_animation("slide", Animation::new(std::time::Duration::from_millis(1100)).repeat().with_easing(ease_in_out), |d, f| d.left(relative(-0.35 + 1.35 * f))))
    } else {
        track.child(div().h_full().w(relative(fraction.clamp(0.0, 1.0) as f32)).bg(t::accent()))
    }
}

fn toast_view(t_: &Toast, cx: &mut Context<ModNest>) -> impl IntoElement {
    let id = t_.id.clone();
    let border: Hsla = match t_.kind {
        "error" => rgba(0xef546680).into(),
        "ok" => rgba(0x3ccf8e73).into(),
        _ => t::line().into(),
    };
    let key = id.clone();
    div()
        .id(SharedString::from(format!("toast-{id}")))
        .w_full()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.dismiss(&key);
            cx.notify();
        }))
        .bg(t::surface_2())
        .border_1()
        .border_color(border)
        .rounded(px(10.))
        .px(px(12.))
        .py(px(9.))
        .text_size(px(13.))
        .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.45), offset: point(px(0.), px(10.)), blur_radius: px(28.), spread_radius: px(0.) }])
        .child(div().font_weight(t::W650).text_color(t::strong()).child(t_.title.clone()))
        .when(!t_.text.is_empty(), |d| d.child(div().text_color(t::muted()).text_size(px(12.)).child(t_.text.clone())))
        .when_some(t_.fraction, |d, f| d.child(bar(f)))
}

pub fn toasts(me: &ModNest, cx: &mut Context<ModNest>) -> impl IntoElement {
    let _ = spinner;
    div().absolute().right(px(16.)).bottom(px(16.)).w(px(320.)).flex().flex_col().gap(px(8.)).children(me.toasts.iter().map(|t_| toast_view(t_, cx)))
}

// ---------- update bar ----------

pub fn update_bar(u: &Value, updating: bool, cx: &mut Context<ModNest>) -> impl IntoElement {
    div()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(14.))
        .bg(t::accent())
        .text_color(white())
        .font_weight(FontWeight::SEMIBOLD)
        .child(format!("ModNest {} is ready to install.", s(u, "version")))
        .child(
            button("app-update-btn", Btn::Normal, true)
                .when(updating, |d| d.opacity(0.6))
                .child(if updating { "Updating…" } else { "Update and restart" })
                .on_click(cx.listener(|this, _, _, cx| this.install_app_update(cx))),
        )
}
