//! Icons and the logo, built into the program (same SVGs the WebView UI uses).
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        fn icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $name, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $name, ".svg"))),)*
                _ => None,
            }
        }
        const ICON_NAMES: &[&str] = &[$(concat!("icons/", $name, ".svg")),*];
    };
}
icons!("arc", "caret", "check", "chev", "dl", "gear", "grid", "layers", "library", "like", "list", "person", "play", "ring", "search");

pub const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(icon(path).map(Cow::Borrowed))
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICON_NAMES.iter().filter(|p| p.starts_with(path)).map(|p| SharedString::from(*p)).collect())
    }
}

/// Path of a built-in icon, e.g. icon_path("search") -> "icons/search.svg".
pub fn icon_path(name: &str) -> SharedString {
    SharedString::from(format!("icons/{name}.svg"))
}
