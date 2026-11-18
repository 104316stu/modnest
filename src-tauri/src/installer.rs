//! Downloads mod zips and puts their files in the right place inside the game folder.
//! Every file written is recorded so a mod can be disabled or removed cleanly.
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

pub const DISABLED: &str = ".disabled";
const SKIP_META: &[&str] = &["manifest.json", "icon.png", "readme.md", "changelog.md", "license", "license.md", "license.txt"];

/// How a package's files are laid out in the game folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Loader pack (BepInEx / MelonLoader): copied into the game root.
    Root,
    /// One BepInEx plugin folder: BepInEx/plugins/<folder>/
    Plugin,
    /// ChainedForge content pack: BepInEx/ChainedForgePacks/<folder>/
    CfPack,
    /// Thunderstore BepInEx package (plugins/, patchers/, config/ … routed like r2modman).
    Thunderstore,
    /// Thunderstore MelonLoader package (Mods/, Plugins/, UserLibs/, UserData/).
    Melon,
    /// A modpack from our own catalog: no files, just a list of dependencies.
    Modpack,
}

impl Layout {
    pub fn parse(s: &str) -> Layout {
        match s {
            "root" => Layout::Root,
            "cf-pack" => Layout::CfPack,
            "thunderstore" => Layout::Thunderstore,
            "melon" => Layout::Melon,
            "modpack" => Layout::Modpack,
            _ => Layout::Plugin,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Root => "root",
            Layout::Plugin => "plugin",
            Layout::CfPack => "cf-pack",
            Layout::Thunderstore => "thunderstore",
            Layout::Melon => "melon",
            Layout::Modpack => "modpack",
        }
    }
}

pub async fn download(url: &str, mut on_progress: impl FnMut(f64)) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder().user_agent("ModNest/0.1 (mod manager)").build()?;
    let res = client.get(url).send().await?;
    if !res.status().is_success() {
        bail!("Download failed ({}) for {}", res.status(), url);
    }
    let total = res.content_length().unwrap_or(0);
    let mut buf = Vec::with_capacity(total as usize);
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        on_progress(if total > 0 { buf.len() as f64 / total as f64 } else { -1.0 });
    }
    Ok(buf)
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches('/').to_string()
}

fn last(n: &str) -> String {
    n.rsplit('/').next().unwrap_or(n).to_ascii_lowercase()
}

/// Folder inside the zip that holds a marker file (loader packs are often wrapped in a folder).
fn prefix_of(files: &[String], marker: &str) -> Option<String> {
    let hit = files.iter().filter(|n| last(n) == marker).min_by_key(|n| n.matches('/').count())?;
    Some(match hit.rfind('/') {
        Some(i) => hit[..=i].to_string(),
        None => String::new(),
    })
}

/// Decide where each zip entry goes: (entry name, destination relative to the game root).
pub fn plan(layout: Layout, folder: &str, names: &[String]) -> Vec<(String, String)> {
    let files: Vec<String> = names.iter().map(|n| norm(n)).filter(|n| !n.ends_with('/') && !n.is_empty()).collect();
    let mut out = Vec::new();
    let has_root_bepinex = files.iter().any(|n| n.split('/').next().unwrap_or("").eq_ignore_ascii_case("bepinex") && n.contains('/'));

    match layout {
        Layout::Modpack => {}
        Layout::Root => {
            let prefix = ["winhttp.dll", "version.dll", "doorstop_config.ini"]
                .iter()
                .find_map(|m| prefix_of(&files, m))
                .unwrap_or_default();
            for n in &files {
                if let Some(rel) = n.strip_prefix(prefix.as_str()) {
                    if !rel.contains('/') && SKIP_META.contains(&rel.to_ascii_lowercase().as_str()) {
                        continue;
                    }
                    out.push((n.clone(), rel.to_string()));
                }
            }
        }
        Layout::CfPack => {
            // Strip a single wrapping folder if the zip has one (pack.json one level down).
            let prefix = prefix_of(&files, "pack.json").unwrap_or_default();
            for n in &files {
                let rel = n.strip_prefix(prefix.as_str()).unwrap_or(n);
                out.push((n.clone(), format!("BepInEx/ChainedForgePacks/{folder}/{rel}")));
            }
        }
        Layout::Plugin if has_root_bepinex => {
            for n in &files {
                out.push((n.clone(), n.clone()));
            }
        }
        Layout::Plugin => {
            for n in &files {
                out.push((n.clone(), format!("BepInEx/plugins/{folder}/{n}")));
            }
        }
        Layout::Melon => {
            for n in &files {
                let parts: Vec<&str> = n.split('/').collect();
                let top = parts[0].to_ascii_lowercase();
                if parts.len() > 1 && ["mods", "plugins", "userlibs", "userdata"].contains(&top.as_str()) {
                    out.push((n.clone(), n.clone()));
                } else if parts.len() == 1 && top.ends_with(".dll") {
                    out.push((n.clone(), format!("Mods/{n}")));
                } else if parts.len() == 1 && SKIP_META.contains(&top.as_str()) {
                    continue;
                } else {
                    out.push((n.clone(), format!("UserData/{folder}/{n}")));
                }
            }
        }
        Layout::Thunderstore => {
            for n in &files {
                let mut parts: Vec<&str> = n.split('/').collect();
                if parts.len() > 1 && parts[0].eq_ignore_ascii_case("bepinex") {
                    parts.remove(0);
                }
                let top = parts[0].to_ascii_lowercase();
                let rest = parts[1..].join("/");
                let dest = match top.as_str() {
                    "plugins" | "patchers" | "monomod" | "core" if parts.len() > 1 => format!("BepInEx/{top}/{folder}/{rest}"),
                    "config" if parts.len() > 1 => format!("BepInEx/config/{rest}"),
                    _ => format!("BepInEx/plugins/{folder}/{}", parts.join("/")),
                };
                out.push((n.clone(), dest));
            }
        }
    }
    out
}

/// Join a relative path onto the game root, refusing anything that escapes it.
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("Blocked unsafe path in mod: {rel}");
    }
    Ok(root.join(rel_path))
}

/// Writes the files. Returns the relative paths written.
pub fn extract(game_root: &Path, layout: Layout, folder: &str, zip_bytes: &[u8]) -> Result<Vec<String>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(zip_bytes)).context("The download is not a valid zip file")?;
    let names: Vec<String> = zip.file_names().map(String::from).collect();
    let mapping = plan(layout, folder, &names);
    if mapping.is_empty() {
        bail!("The mod zip is empty.");
    }
    let mut written = Vec::new();
    for (entry, dest) in mapping {
        let full = safe_join(game_root, &dest)?;
        // find the original (un-normalised) entry name
        let idx = names.iter().position(|n| norm(n) == entry).ok_or_else(|| anyhow!("missing {entry}"))?;
        let mut f = zip.by_index(idx)?;
        let mut data = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut data)?;
        if let Some(dir) = full.parent() {
            fs::create_dir_all(dir)?;
        }
        // A disabled copy left over from before would shadow the new file; remove it.
        let _ = fs::remove_file(with_suffix(&full));
        fs::write(&full, data).with_context(|| format!("Could not write {dest}. Is the game running?"))?;
        written.push(dest);
    }
    Ok(written)
}

fn with_suffix(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(DISABLED);
    PathBuf::from(s)
}

fn remove_empty_dirs(root: &Path, rel: &str) {
    let Ok(full) = safe_join(root, rel) else { return };
    let mut dir = full.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        if fs::remove_dir(&d).is_err() {
            break; // not empty
        }
        dir = d.parent().map(Path::to_path_buf);
    }
}

pub fn remove_files(root: &Path, files: &[String]) -> Result<()> {
    for rel in files {
        let full = safe_join(root, rel)?;
        for p in [full.clone(), with_suffix(&full)] {
            if p.exists() {
                fs::remove_file(&p).with_context(|| format!("Could not delete {rel}. Is the game running?"))?;
            }
        }
    }
    for rel in files {
        remove_empty_dirs(root, rel);
    }
    Ok(())
}

pub fn set_enabled(root: &Path, files: &[String], enabled: bool) -> Result<()> {
    for rel in files {
        let on = safe_join(root, rel)?;
        let off = with_suffix(&on);
        let (from, to) = if enabled { (off, on) } else { (on, off) };
        if from.exists() {
            fs::rename(&from, &to).with_context(|| format!("Could not {} {rel}. Is the game running?", if enabled { "enable" } else { "disable" }))?;
        }
    }
    Ok(())
}

/// Every file under a folder (used when adopting a mod that was installed by hand).
pub fn list_folder(root: &Path, rel_dir: &str) -> Vec<String> {
    fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(full) = safe_join(root, rel) else { return };
        let Ok(rd) = fs::read_dir(full) else { return };
        for e in rd.flatten() {
            let r = format!("{rel}/{}", e.file_name().to_string_lossy());
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                walk(root, &r, out);
            } else {
                out.push(r.strip_suffix(DISABLED).unwrap_or(&r).to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &norm(rel_dir), &mut out);
    out
}

/// Reads manifest.json (Thunderstore format) from a zip, if there is one: (name, version, author, deps).
pub fn read_manifest(zip_bytes: &[u8]) -> Option<serde_json::Value> {
    let mut zip = zip::ZipArchive::new(Cursor::new(zip_bytes)).ok()?;
    let name = zip.file_names().find(|n| n.eq_ignore_ascii_case("manifest.json"))?.to_string();
    let mut s = String::new();
    zip.by_name(&name).ok()?.read_to_string(&mut s).ok()?;
    serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()
}

pub fn zip_has_file(zip_bytes: &[u8], file: &str) -> bool {
    zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map(|z| z.file_names().any(|n| last(n) == file))
        .unwrap_or(false)
}

/// Turns the mod loader on or off in the game's doorstop_config.ini (Doorstop 3 and 4).
/// Off means starting the game from Steam runs it without mods; ModNest's Play button
/// switches it back on for that run with a launch option. Returns None when there's no config.
pub fn set_loader_default(root: &Path, enabled: bool) -> Result<Option<bool>> {
    let file = root.join("doorstop_config.ini");
    let Ok(text) = fs::read_to_string(&file) else { return Ok(None) };
    let (new_text, changed) = edit_doorstop(&text, enabled);
    if changed {
        fs::write(&file, new_text).with_context(|| "Could not update doorstop_config.ini. Is the game running?")?;
    }
    Ok(Some(changed))
}

/// Pure text edit behind set_loader_default: sets `enabled` in the [General] (Doorstop 4)
/// or [UnityDoorstop] (Doorstop 3) section. Returns (text, whether anything changed).
pub fn edit_doorstop(text: &str, enabled: bool) -> (String, bool) {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let want = if enabled { "true" } else { "false" };
    let mut section = String::new();
    let mut changed = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') {
                section = t[1..t.len() - 1].trim().to_ascii_lowercase();
                return line.to_string();
            }
            let in_main = section == "general" || section == "unitydoorstop";
            if let Some((k, v)) = t.split_once('=') {
                if in_main && k.trim().eq_ignore_ascii_case("enabled") && !v.trim().eq_ignore_ascii_case(want) {
                    changed = true;
                    return format!("enabled = {want}");
                }
            }
            line.to_string()
        })
        .collect();
    let mut out = lines.join(nl);
    if text.ends_with('\n') {
        out.push_str(nl);
    }
    (out, changed)
}

/// Steam launch arguments. `mods` decides whether the loader runs this time.
pub fn launch_args(app_id: u32, melonloader: bool, mods: bool) -> Vec<String> {
    let mut args = vec!["-applaunch".to_string(), app_id.to_string()];
    if melonloader {
        if !mods {
            args.push("--no-mods".into());
        }
    } else {
        // Doorstop 4 and Doorstop 3 spell the switch differently; pass both.
        let v = if mods { "true" } else { "false" };
        args.extend(["--doorstop-enabled".into(), v.into(), "--doorstop-enable".into(), v.into()]);
    }
    args
}
