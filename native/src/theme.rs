//! ModNest's look, copied value-for-value from ui/styles.css so the native app matches the WebView one.
use gpui::{hsla, rgb, rgba, Hsla, Rgba};

pub const FONT: &str = "Segoe UI Variable Text";
pub const FONT_FALLBACK: &str = "Segoe UI";

// :root colours
pub fn bg() -> Rgba { rgb(0x111318) }
pub fn rail() -> Rgba { rgb(0x0b0d11) }
pub fn surface() -> Rgba { rgb(0x181b22) }
pub fn surface_2() -> Rgba { rgb(0x1f232c) }
pub fn surface_3() -> Rgba { rgb(0x272c37) }
pub fn line() -> Rgba { rgb(0x2a2f3a) }
pub fn line_soft() -> Rgba { rgb(0x20242d) }
pub fn text() -> Rgba { rgb(0xdfe3eb) }
pub fn strong() -> Rgba { rgb(0xf4f6fa) }
pub fn muted() -> Rgba { rgb(0x8b93a5) }
pub fn placeholder() -> Rgba { rgb(0x5f6779) }
pub fn accent() -> Rgba { rgb(0x4f8cff) }
pub fn accent_hover() -> Rgba { rgb(0x3f7cf0) }
pub fn accent_hi() -> Rgba { rgb(0x7db4ff) }
/// rgba(79, 140, 255, .14)
pub fn accent_lo() -> Rgba { rgba(0x4f8cff24) }
pub fn amber() -> Rgba { rgb(0xf5a524) }
pub fn green() -> Rgba { rgb(0x3ccf8e) }
pub fn green_hover() -> Rgba { rgb(0x35bd80) }
pub fn play_text() -> Rgba { rgb(0x06281a) }
pub fn red() -> Rgba { rgb(0xef5466) }
/// row hover background (#1b1e26)
pub fn row_hover() -> Rgba { rgb(0x1b1e26) }

// translucent variants used by pills / notes
pub fn green_lo() -> Rgba { rgba(0x3ccf8e24) } // .14
pub fn amber_lo() -> Rgba { rgba(0xf5a52426) } // .15
pub fn amber_note() -> Rgba { rgba(0xf5a52414) } // .08
pub fn amber_border() -> Rgba { rgba(0xf5a52459) } // .35
pub fn red_lo() -> Rgba { rgba(0xef546626) } // .15
pub fn accent_border() -> Rgba { rgba(0x4f8cff59) } // .35
pub fn glass() -> Rgba { rgba(0x0a0c108c) } // rgba(10,12,16,.55)
pub fn glass_border() -> Rgba { rgba(0xffffff24) } // rgba(255,255,255,.14)
pub fn badge_bg() -> Rgba { rgba(0x080a0ed1) } // rgba(8,10,14,.82)
pub fn backdrop() -> Rgba { rgba(0x050609b3) } // rgba(5,6,9,.7)
pub fn shadow() -> Hsla { hsla(0., 0., 0., 0.45) }

// sizes (px), from styles.css
pub const RAIL_W: f32 = 68.0;
pub const RAIL_BTN: f32 = 44.0;
pub const BANNER_H: f32 = 168.0;
pub const RADIUS: f32 = 10.0;
pub const BTN_H: f32 = 34.0;
pub const BTN_SM_H: f32 = 28.0;
pub const ROW_MIN_H: f32 = 52.0;
pub const ICON: f32 = 40.0;
pub const PAD_X: f32 = 24.0;
pub const LIB_PAD_X: f32 = 32.0;
pub const TEXT: f32 = 14.0;
pub const SMALL: f32 = 12.0;
/// ::-webkit-scrollbar { width: 10px }
pub const SCROLLBAR: f32 = 10.0;
/// ::-webkit-scrollbar-thumb background
pub fn scroll_thumb() -> Rgba { rgb(0x2a2f3a) }
pub const W650: gpui::FontWeight = gpui::FontWeight(650.0);
