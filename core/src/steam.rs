//! Finds Steam libraries and installed games by reading Steam's own files.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SteamGame {
    #[allow(dead_code)]
    pub name: String,
    pub path: PathBuf,
}

pub fn steam_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        if let Ok(key) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
            if let Ok(p) = key.get_value::<String, _>("SteamPath") {
                candidates.push(PathBuf::from(p.replace('/', "\\")));
            }
        }
    }
    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    candidates.push(PathBuf::from(r"C:\Program Files\Steam"));
    candidates.into_iter().find(|p| p.join("steamapps").exists())
}

/// Pulls every `"key" "value"` pair with the given key out of a Valve KeyValues text file.
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix(&needle)?.trim();
            let v = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(v.replace("\\\\", "\\"))
        })
        .collect()
}

fn library_folders() -> Vec<PathBuf> {
    let Some(root) = steam_root() else { return vec![] };
    let mut libs = vec![root.clone()];
    if let Ok(text) = fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for p in vdf_values(&text, "path") {
            let p = PathBuf::from(p);
            if !libs.iter().any(|l| l.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy())) {
                libs.push(p);
            }
        }
    }
    libs.into_iter().filter(|p| p.join("steamapps").exists()).collect()
}

fn read_manifest(file: &Path) -> Option<(u32, String, String)> {
    let text = fs::read_to_string(file).ok()?;
    let first = |k: &str| vdf_values(&text, k).into_iter().next();
    Some((first("appid")?.parse().ok()?, first("name").unwrap_or_default(), first("installdir")?))
}

/// Steam app id -> installed game.
pub fn installed_games() -> HashMap<u32, SteamGame> {
    let mut out = HashMap::new();
    for lib in library_folders() {
        let dir = lib.join("steamapps");
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            if let Some((id, game_name, install_dir)) = read_manifest(&e.path()) {
                let path = dir.join("common").join(install_dir);
                if path.exists() {
                    out.insert(id, SteamGame { name: game_name, path });
                }
            }
        }
    }
    out
}

/// Steam's own cached artwork for a game (what the Steam library shows).
/// kind: "capsule" (tall cover), "hero" (wide banner), "header", "logo".
pub fn library_art(app_id: u32, kind: &str) -> Option<PathBuf> {
    let file = match kind {
        "capsule" => "library_capsule.jpg",
        "hero" => "library_hero.jpg",
        "header" => "library_header.jpg",
        "logo" => "logo.png",
        _ => return None,
    };
    let dir = steam_root()?.join("appcache").join("librarycache");
    // Older Steam: librarycache/<id>_library_600x900.jpg
    let old = match kind {
        "capsule" => Some(format!("{app_id}_library_600x900.jpg")),
        "hero" => Some(format!("{app_id}_library_hero.jpg")),
        "header" => Some(format!("{app_id}_header.jpg")),
        "logo" => Some(format!("{app_id}_logo.png")),
        _ => None,
    };
    if let Some(o) = old.map(|o| dir.join(o)).filter(|p| p.exists()) {
        return Some(o);
    }
    // Newer Steam: librarycache/<id>/<hash>/library_capsule.jpg (or directly in <id>/)
    let game_dir = dir.join(app_id.to_string());
    if game_dir.join(file).exists() {
        return Some(game_dir.join(file));
    }
    fs::read_dir(&game_dir).ok()?.flatten().map(|e| e.path().join(file)).find(|p| p.exists())
}
