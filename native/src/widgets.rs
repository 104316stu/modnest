//! Small reusable pieces, each matching a CSS class from ui/styles.css.
use crate::assets::icon_path;
use crate::theme as t;
use gpui::prelude::FluentBuilder;
use gpui::*;
use serde_json::Value;

// ---------- JSON helpers (the engine answers in the same JSON shape the WebView UI used) ----------

pub fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}
pub fn so(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(String::from)
}
pub fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(false)
}
pub fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64))).unwrap_or(0)
}
pub fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k).and_then(|x| x.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

pub fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

pub fn fmt_num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// "today", "3d ago", "2mo ago"… like the WebView's ago().
pub fn ago(iso: &str) -> String {
    let Some(date) = iso.get(..10) else { return String::new() };
    let parts: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return String::new();
    }
    let days_from_civil = |y: i64, m: i64, d: i64| {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86400).unwrap_or(0);
    let d = now - (days_from_civil(parts[0], parts[1], parts[2]) - days_from_civil(1970, 1, 1));
    match d {
        i64::MIN..=0 => "today".into(),
        1 => "yesterday".into(),
        2..=30 => format!("{d}d ago"),
        31..=364 => format!("{}mo ago", d / 30),
        _ => format!("{}y ago", d / 365),
    }
}

// ---------- icons & images ----------

pub fn icon(name: &str, size: f32, color: impl Into<Hsla>) -> Svg {
    svg().path(icon_path(name)).size(px(size)).flex_none().text_color(color)
}

/// An image from a web URL, or Steam's local art (http://art.localhost/…) read straight from disk.
pub fn picture(url: &str) -> Img {
    match modnest_core::art_file(url) {
        Some(path) => img(path),
        None => img(SharedString::from(url.to_string())),
    }
}

/// An image filling a box like CSS `object-fit: cover` with `object-position: 50% y`
/// (`ratio` = the image's width / height, which we know from the kind of Steam art).
pub fn fitted(url: &str, w: f32, h: f32, ratio: f32, y: f32) -> Div {
    let (iw, ih) = if w / h > ratio { (w, w / ratio) } else { (h * ratio, h) };
    div().relative().w(px(w)).h(px(h)).overflow_hidden().child(picture(url).absolute().w(px(iw)).h(px(ih)).left(px((w - iw) / 2.)).top(px((h - ih) * y)))
}

/// Rounds the corners of a clipped picture by painting the outside of a rounded square in `bg`
/// (GPUI clips children to rectangles only). A border whose inner edge has radius `r`.
pub fn corners(r: f32, size: f32, bg: impl Into<Hsla>) -> Div {
    let b = r + 2.;
    div().absolute().top(px(-b)).left(px(-b)).size(px(size + 2. * b)).rounded(px(r + b)).border(px(b)).border_color(bg)
}

/// Columns of a CSS grid `repeat(auto-fill, minmax(min, 1fr))` in `width` with `gap`.
pub fn grid_cols(width: f32, min: f32, gap: f32) -> u16 {
    (((width + gap) / (min + gap)).floor() as u16).max(1)
}

// ---------- pills (.pill) ----------

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Plain,
    Blue,
    Green,
    Amber,
    Red,
    Line,
}

pub fn pill(text: impl Into<SharedString>, tone: Tone) -> Div {
    let (bg, fg, border) = match tone {
        Tone::Plain => (t::surface_3().into(), t::text().into(), None),
        Tone::Blue => (t::accent_lo().into(), t::accent_hi().into(), None),
        Tone::Green => (t::green_lo().into(), t::green().into(), None),
        Tone::Amber => (t::amber_lo().into(), t::amber().into(), None),
        Tone::Red => (t::red_lo().into(), t::red().into(), None),
        Tone::Line => (Hsla::transparent_black(), t::muted().into(), Some(t::line())),
    };
    let fg: Hsla = fg;
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.))
        .px(px(8.))
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .when_some(border, |d, c| d.border_1().border_color(c))
        .child(text.into())
}

// ---------- buttons (.btn) ----------

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Normal,
    Primary,
    Ghost,
    Danger,
    Glass,
}

/// A button like .btn / .btn.primary / .btn.ghost … `small` = .btn.sm
pub fn button(id: impl Into<ElementId>, kind: Btn, small: bool) -> Stateful<Div> {
    let h = if small { t::BTN_SM_H } else { t::BTN_H };
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .h(px(h))
        .px(px(if small { 10. } else { 14. }))
        .rounded(px(if small { 7. } else { 8. }))
        .border_1()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(if small { 12. } else { 13. }))
        .whitespace_nowrap()
        .cursor_pointer();
    match kind {
        Btn::Normal => base.bg(t::surface_2()).border_color(t::line()).text_color(t::strong()).hover(|s| s.bg(t::surface_3())),
        Btn::Primary => base.bg(t::accent()).border_color(transparent_black()).text_color(white()).hover(|s| s.bg(t::accent_hover())),
        Btn::Ghost => base.bg(transparent_black()).border_color(transparent_black()).text_color(t::muted()).hover(|s| s.bg(t::surface_2()).text_color(t::strong())),
        Btn::Danger => base.bg(t::surface_2()).border_color(t::line()).text_color(t::strong()).hover(|s| s.bg(transparent_black()).border_color(t::red()).text_color(t::red())),
        Btn::Glass => base.bg(t::glass()).border_color(t::glass_border()).text_color(t::strong()).hover(|s| s.bg(rgba(0x0a0c10bf))),
    }
}

// ---------- notes (.note) ----------

pub fn note(tone: Tone) -> Div {
    let (bg, border): (Hsla, Hsla) = match tone {
        Tone::Blue => (t::accent_lo().into(), t::accent_border().into()),
        Tone::Amber => (t::amber_note().into(), t::amber_border().into()),
        _ => (t::surface().into(), t::line().into()),
    };
    div()
        .mx(px(24.))
        .mt(px(6.))
        .mb(px(4.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(9.))
        .flex()
        .flex_wrap()
        .gap(px(12.))
        .items_center()
        .justify_between()
        .text_size(px(13.))
        .bg(bg)
        .border_1()
        .border_color(border)
}

// ---------- toggle switch (.switch) ----------

pub fn switch(id: impl Into<ElementId>, on: bool, disabled: bool) -> Stateful<Div> {
    let track: Hsla = if on { t::accent().into() } else { t::surface_3().into() };
    div()
        .id(id)
        .flex_none()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .bg(track)
        .when(disabled, |d| d.opacity(0.45))
        .when(!disabled, |d| d.cursor_pointer())
        .relative()
        .child(div().absolute().top(px(3.)).left(px(if on { 19. } else { 3. })).size(px(14.)).rounded_full().bg(white()))
}

/// The little letter square used when a mod has no icon (.ico without image).
pub fn letter_icon(title: &str, size: f32, radius: f32, text: f32) -> Div {
    div()
        .flex_none()
        .size(px(size))
        .rounded(px(radius))
        .bg(t::surface_3())
        .flex()
        .items_center()
        .justify_center()
        .text_color(t::muted())
        .font_weight(FontWeight::BOLD)
        .text_size(px(text))
        .child(title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into()))
}

/// Mod icon (.ico): 40×40, radius 8, or a letter square if there's no image.
pub fn mod_icon(v: &Value) -> AnyElement {
    match so(v, "icon") {
        Some(url) => picture(&url).size(px(t::ICON)).flex_none().rounded(px(8.)).object_fit(ObjectFit::Cover).bg(t::surface_3()).into_any_element(),
        None => letter_icon(&s(v, "title"), t::ICON, 8., 16.).into_any_element(),
    }
}

/// One line with "…" at the end when it doesn't fit (like CSS text-overflow: ellipsis).
/// GPUI's own truncate() keeps the first, unconstrained measurement of no-wrap text,
/// so this wraps-and-clamps to one line instead, which is measured again at the real width.
pub trait Ellipsis: Styled + Sized {
    fn ellipsis(self) -> Self {
        self.overflow_hidden().text_ellipsis().line_clamp(1)
    }
}
impl<T: Styled> Ellipsis for T {}
