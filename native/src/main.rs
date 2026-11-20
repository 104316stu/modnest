// ModNest with a native GPUI interface. Same engine (modnest-core) and data folder as the WebView app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod bridge;
mod dialogs;
mod game;
mod library;
mod theme;
mod updater;
mod widgets;

use gpui::*;

fn main() {
    let engine = bridge::Engine::start(bridge::data_dir());
    Application::new().with_assets(assets::Assets).with_http_client(bridge::Http::new()).run(move |cx: &mut App| {
        gpui_component::init(cx);
        {
            let th = gpui_component::Theme::global_mut(cx);
            th.font_family = theme::FONT.into();
            th.font_size = px(theme::TEXT);
            th.foreground = theme::strong().into();
            th.caret = theme::strong().into();
            th.selection = theme::accent_border().into();
            th.muted_foreground = theme::placeholder().into();
            th.background = theme::bg().into();
            th.scrollbar_show = gpui_component::scroll::ScrollbarShow::Always;
            th.scrollbar = gpui::transparent_black();
            th.scrollbar_thumb = theme::scroll_thumb().into();
            th.scrollbar_thumb_hover = gpui::rgb(0x3a404c).into();
        }
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(900.), px(600.))),
            titlebar: Some(TitlebarOptions { title: Some("ModNest".into()), ..Default::default() }),
            app_id: Some("app.modnest.desktop".into()),
            ..Default::default()
        };
        cx.open_window(opts, |window, cx| {
            let view = cx.new(|cx| app::ModNest::new(engine, window, cx));
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        })
        .expect("open window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
