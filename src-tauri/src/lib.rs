//! ModNest: mod manager core. Everything the UI does goes through the commands below.
pub mod installer;
pub mod profiles;
mod registry;
mod steam;
pub mod store;
mod thunderstore;

use installer::Layout;
use registry::{OwnGame, Registry};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use store::{Installed, Profile, ProfileMod, Settings, Store};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

struct AppState {
    store: Mutex<Store>,
    registry: Mutex<Registry>,
    steam: Mutex<HashMap<u32, steam::SteamGame>>,
    /// Latest known version per "gameId|key", filled by check_updates.
    latest: Mutex<HashMap<String, String>>,
    /// Only one change to a game folder at a time.
    busy: tauri::async_runtime::Mutex<()>,
    data_dir: PathBuf,
}

// ---------- games ----------

#[derive(Clone)]
enum Kind {
    Own(OwnGame),
    Ts(&'static thunderstore::Community),
}

#[derive(Clone)]
struct Game {
    id: String,
    name: String,
    kind: Kind,
    app_ids: Vec<u32>,
}

impl Game {
    fn melon(&self) -> bool {
        matches!(self.kind, Kind::Ts(c) if c.melonloader)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameView {
    id: String,
    name: String,
    source: &'static str,
    installed: bool,
    path: Option<String>,
    loader: String,
    app_id: Option<u32>,
    mod_count: usize,
    /// Wide image (Steam header).
    cover: Option<String>,
    /// Tall cover for the game picker.
    capsule: Option<String>,
    /// Big banner for the game page.
    hero: Option<String>,
    active_profile: Option<String>,
}

fn all_games(reg: &Registry) -> Vec<Game> {
    let mut out: Vec<Game> = reg
        .games
        .iter()
        .map(|g| Game { id: g.id.clone(), name: g.name.clone(), app_ids: g.steam_app_ids.clone(), kind: Kind::Own(g.clone()) })
        .collect();
    for c in thunderstore::COMMUNITIES {
        out.push(Game { id: format!("ts-{}", c.id), name: c.name.to_string(), app_ids: c.steam_app_ids.to_vec(), kind: Kind::Ts(c) });
    }
    out
}

fn find_game(st: &AppState, id: &str) -> CmdResult<Game> {
    let reg = st.registry.lock().unwrap();
    all_games(&reg).into_iter().find(|g| g.id == id).ok_or_else(|| format!("Unknown game {id}"))
}

fn game_path(st: &AppState, g: &Game) -> Option<PathBuf> {
    if let Some(p) = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.custom_path.clone()) {
        if Path::new(&p).exists() {
            return Some(PathBuf::from(p));
        }
    }
    let steam = st.steam.lock().unwrap();
    g.app_ids.iter().find_map(|id| steam.get(id).map(|s| s.path.clone()))
}

fn require_path(st: &AppState, g: &Game) -> CmdResult<PathBuf> {
    game_path(st, g).ok_or_else(|| format!("{} isn't installed (or ModNest can't find it). Use \"Set game folder\".", g.name))
}

/// URL for art served from Steam's local cache (see the "art" protocol in run()).
fn art_url(app_id: u32, kind: &str) -> Option<String> {
    steam::library_art(app_id, kind).map(|_| format!("http://art.localhost/{app_id}/{kind}"))
}

fn view(st: &AppState, g: &Game) -> GameView {
    let path = game_path(st, g);
    let (mod_count, header, cover, active) = {
        let s = st.store.lock().unwrap();
        let rec = s.data.games.get(&g.id);
        (
            rec.map(|r| r.installed.len()).unwrap_or(0),
            g.app_ids.iter().find_map(|id| s.data.art.get(id).cloned()),
            s.data.covers.get(&g.id).cloned(),
            rec.and_then(|r| r.active_profile.clone()),
        )
    };
    let app_id = g.app_ids.first().copied();
    let header = header.or_else(|| app_id.map(|id| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/header.jpg")));
    let loader = match &g.kind {
        Kind::Own(o) => o.loader.clone(),
        Kind::Ts(c) => if c.melonloader { "MelonLoader".into() } else { "BepInEx".into() },
    };
    GameView {
        id: g.id.clone(),
        name: g.name.clone(),
        source: if matches!(g.kind, Kind::Own(_)) { "own" } else { "thunderstore" },
        installed: path.is_some(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        loader,
        app_id,
        mod_count,
        capsule: app_id.and_then(|id| art_url(id, "capsule")).or(cover).or_else(|| header.clone()),
        hero: app_id.and_then(|id| art_url(id, "hero")).or_else(|| header.clone()),
        cover: header,
        active_profile: active,
    }
}

/// Header images from Steam's store API and tall covers from Thunderstore, fetched once and remembered.
async fn fetch_missing_art(st: &AppState, games: &[Game]) {
    let (missing_art, missing_covers): (Vec<u32>, Vec<(String, String)>) = {
        let s = st.store.lock().unwrap();
        (
            games.iter().filter_map(|g| g.app_ids.first().copied()).filter(|id| !s.data.art.contains_key(id)).collect(),
            games
                .iter()
                .filter_map(|g| match g.kind {
                    Kind::Ts(c) if !s.data.covers.contains_key(&g.id) => Some((g.id.clone(), c.id.to_string())),
                    _ => None,
                })
                .collect(),
        )
    };
    if missing_art.is_empty() && missing_covers.is_empty() {
        return;
    }
    let Ok(client) = reqwest::Client::builder().user_agent("ModNest/0.1").timeout(std::time::Duration::from_secs(6)).build() else { return };
    let art_jobs = missing_art.into_iter().map(|id| {
        let client = client.clone();
        async move {
            let url = format!("https://store.steampowered.com/api/appdetails?appids={id}&filters=basic");
            let v: serde_json::Value = client.get(url).send().await.ok()?.json().await.ok()?;
            Some((id, v[id.to_string()]["data"]["header_image"].as_str()?.to_string()))
        }
    });
    let cover_jobs = missing_covers.into_iter().map(|(gid, cid)| {
        let client = client.clone();
        async move {
            let v: serde_json::Value = client.get(format!("https://thunderstore.io/api/cyberstorm/community/{cid}/")).send().await.ok()?.json().await.ok()?;
            Some((gid, v["cover_image_url"].as_str()?.to_string()))
        }
    });
    let (arts, covers) = futures_util::join!(futures_util::future::join_all(art_jobs), futures_util::future::join_all(cover_jobs));
    let mut s = st.store.lock().unwrap();
    s.data.art.extend(arts.into_iter().flatten());
    s.data.covers.extend(covers.into_iter().flatten());
    s.save();
}

// ---------- packages ----------

/// Everything needed to install one package.
#[derive(Clone, Debug)]
struct Pkg {
    key: String,
    title: String,
    author: String,
    version: String,
    download: String,
    sha256: Option<String>,
    layout: Layout,
    folder: String,
    deps: Vec<String>,
    icon: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct LocalMeta {
    title: String,
    author: String,
    version: String,
    layout: String,
    folder: String,
}

fn cache_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("cache")
}

fn cache_file(st: &AppState, key: &str, version: &str) -> PathBuf {
    cache_dir(st).join(format!("{}@{}.zip", safe_name(key), safe_name(version)))
}

async fn resolve(st: &AppState, g: &Game, key: &str) -> CmdResult<Pkg> {
    if let Some(id) = key.strip_prefix("own:") {
        let p = st.registry.lock().unwrap().package(id).cloned().ok_or_else(|| format!("Mod {id} is not in the catalog"))?;
        return Ok(Pkg {
            key: key.to_string(),
            title: p.name.clone(),
            author: p.author.clone(),
            version: p.version.clone(),
            download: p.download.clone(),
            sha256: p.sha256.clone(),
            layout: Layout::parse(&p.install_type),
            folder: p.install_folder.clone().unwrap_or_else(|| p.id.clone()),
            deps: p.dependencies.iter().map(|d| format!("own:{d}")).collect(),
            icon: p.icon.clone(),
        });
    }
    if let Some(full) = key.strip_prefix("ts:") {
        let d = thunderstore::detail(full).await.map_err(err)?;
        let layout = if thunderstore::is_loader(full) { Layout::Root } else if g.melon() { Layout::Melon } else { Layout::Thunderstore };
        return Ok(Pkg {
            key: key.to_string(),
            title: d.title,
            author: d.author,
            version: d.version,
            download: d.download,
            sha256: None,
            layout,
            folder: full.to_string(),
            deps: d.dependencies.iter().map(|d| format!("ts:{d}")).collect(),
            icon: d.icon,
        });
    }
    if key.starts_with("local:") {
        // Mods installed from a file can come back from the download cache.
        let meta_prefix = format!("{}@", safe_name(key));
        let newest = std::fs::read_dir(cache_dir(st))
            .ok()
            .and_then(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&meta_prefix) && e.file_name().to_string_lossy().ends_with(".json"))
                    .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            })
            .ok_or_else(|| format!("{} was installed from a file that isn't available any more.", key.trim_start_matches("local:")))?;
        let meta: LocalMeta = serde_json::from_slice(&std::fs::read(newest.path()).map_err(err)?).map_err(err)?;
        return Ok(Pkg {
            key: key.into(),
            title: meta.title,
            author: meta.author,
            version: meta.version,
            download: String::new(),
            sha256: None,
            layout: Layout::parse(&meta.layout),
            folder: meta.folder,
            deps: vec![],
            icon: None,
        });
    }
    Err(format!("Unknown mod {key}"))
}

/// Base (loader/framework) packages the game always needs.
async fn base_keys(st: &AppState, g: &Game) -> Vec<String> {
    match &g.kind {
        Kind::Own(o) => o.base_packages.iter().map(|p| format!("own:{p}")).collect(),
        Kind::Ts(c) => {
            if let Some(k) = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.base_key.clone()) {
                return vec![k];
            }
            match thunderstore::find_base(c.id).await {
                Ok(Some(full)) => {
                    let k = format!("ts:{full}");
                    let mut s = st.store.lock().unwrap();
                    s.game(&g.id).base_key = Some(k.clone());
                    s.save();
                    vec![k]
                }
                _ => vec![],
            }
        }
    }
}

fn cached_base_keys(st: &AppState, g: &Game) -> Vec<String> {
    match &g.kind {
        Kind::Own(o) => o.base_packages.iter().map(|p| format!("own:{p}")).collect(),
        Kind::Ts(_) => st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.base_key.clone()).into_iter().collect(),
    }
}

/// True when starting the game from Steam should run it without mods.
fn steam_vanilla(st: &AppState, g: &Game) -> bool {
    let s = st.store.lock().unwrap();
    s.data.games.get(&g.id).and_then(|r| r.steam_vanilla).unwrap_or(s.data.settings.steam_vanilla)
}

/// Keeps the game's doorstop_config.ini in step with the Steam setting (BepInEx games only).
fn apply_steam_mode(st: &AppState, g: &Game, root: &Path) -> CmdResult<()> {
    if g.melon() {
        return Ok(());
    }
    installer::set_loader_default(root, !steam_vanilla(st, g)).map(|_| ()).map_err(err)
}

/// Matches up the store with what is really in the game folder:
/// adopts mods installed by hand, forgets mods whose files are gone.
fn reconcile(st: &AppState, g: &Game, root: &Path) {
    let reg = st.registry.lock().unwrap().clone();
    let base = cached_base_keys(st, g);
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);

    rec.installed.retain(|_, m| {
        if m.files.is_empty() {
            return true;
        }
        m.files.iter().any(|f| {
            installer::safe_join(root, f).map(|p| p.exists() || PathBuf::from(format!("{}{}", p.display(), installer::DISABLED)).exists()).unwrap_or(false)
        })
    });

    let exists = |rel: &str| installer::safe_join(root, rel).map(|p| p.exists()).unwrap_or(false);
    let adopted = |key: String, title: String, author: String, layout: &str, files: Vec<String>, icon: Option<String>| Installed {
        key,
        title,
        author,
        version: "installed by hand".into(),
        layout: layout.into(),
        enabled: true,
        adopted: true,
        icon,
        files,
        dependencies: vec![],
        auto: false,
    };
    match &g.kind {
        Kind::Own(_) => {
            for p in reg.packages.iter().filter(|p| p.games.contains(&g.id) && !p.detect.is_empty()) {
                let key = format!("own:{}", p.id);
                let layout = Layout::parse(&p.install_type);
                if !p.detect.iter().all(|d| exists(d)) {
                    // A loader whose key files were wiped (e.g. Steam "verify files") must be reinstalled.
                    if layout == Layout::Root {
                        rec.installed.remove(&key);
                    }
                    continue;
                }
                if rec.installed.contains_key(&key) {
                    continue;
                }
                let folder = p.install_folder.clone().unwrap_or_else(|| p.id.clone());
                let files = match layout {
                    Layout::Plugin => installer::list_folder(root, &format!("BepInEx/plugins/{folder}")),
                    Layout::CfPack => installer::list_folder(root, &format!("BepInEx/ChainedForgePacks/{folder}")),
                    _ => vec![],
                };
                rec.installed.insert(key.clone(), adopted(key, p.name.clone(), p.author.clone(), layout.as_str(), files, p.icon.clone()));
            }
        }
        Kind::Ts(c) => {
            let present = if c.melonloader { exists("version.dll") && exists("MelonLoader") } else { exists("winhttp.dll") && exists("BepInEx/core") };
            for key in base {
                if !present {
                    rec.installed.remove(&key);
                } else if !rec.installed.contains_key(&key) {
                    let title = thunderstore::pretty(key.trim_start_matches("ts:").split_once('-').map(|x| x.1).unwrap_or(&key));
                    rec.installed.insert(key.clone(), adopted(key, title, String::new(), "root", vec![], None));
                }
            }
        }
    }
    s.save();
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModView {
    key: String,
    title: String,
    author: String,
    version: String,
    enabled: bool,
    required: bool,
    adopted: bool,
    auto: bool,
    icon: Option<String>,
    latest: Option<String>,
    update_available: bool,
    from_file: bool,
    dependencies: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    name: String,
    count: usize,
    active: bool,
    modpack: Option<String>,
    updated: u64,
    mods: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameState {
    game: GameView,
    mods: Vec<ModView>,
    base_missing: Vec<String>,
    profiles: Vec<ProfileView>,
    /// Starting from Steam runs the normal game (mods only via ModNest's Play).
    steam_vanilla: bool,
    /// Whether this game's loader can be switched that way (BepInEx with a doorstop config).
    steam_switchable: bool,
}

/// Rough version compare: true if `b` is newer than `a`.
fn newer(a: &str, b: &str) -> bool {
    let parts = |s: &str| s.split(|c: char| !c.is_ascii_alphanumeric()).map(String::from).collect::<Vec<_>>();
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).map(String::as_str).unwrap_or("0"), pb.get(i).map(String::as_str).unwrap_or("0"));
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
    }
    false
}

fn state_of(st: &AppState, g: &Game) -> GameState {
    let root = game_path(st, g);
    if let Some(root) = &root {
        reconcile(st, g, root);
        // Best effort: if the game is running the file is locked; it's fixed next time.
        let _ = apply_steam_mode(st, g, root);
    }
    let steam_switchable = !g.melon() && root.as_ref().map(|r| r.join("doorstop_config.ini").exists()).unwrap_or(false);
    let base = cached_base_keys(st, g);
    let latest = st.latest.lock().unwrap().clone();
    let rec = st.store.lock().unwrap().data.games.get(&g.id).cloned().unwrap_or_default();
    let mut mods: Vec<ModView> = rec
        .installed
        .values()
        .map(|m| {
            let l = latest.get(&format!("{}|{}", g.id, m.key)).cloned();
            ModView {
                key: m.key.clone(),
                title: m.title.clone(),
                author: m.author.clone(),
                version: m.version.clone(),
                enabled: m.enabled,
                required: base.contains(&m.key),
                adopted: m.adopted,
                auto: m.auto,
                icon: m.icon.clone(),
                update_available: l.as_deref().map(|l| !m.adopted && newer(&m.version, l)).unwrap_or(false),
                latest: l,
                from_file: m.key.starts_with("local:"),
                dependencies: m.dependencies.clone(),
            }
        })
        .collect();
    mods.sort_by(|a, b| b.required.cmp(&a.required).then(a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    let base_missing = base.into_iter().filter(|k| !rec.installed.contains_key(k)).collect();
    let mut profiles: Vec<ProfileView> = rec
        .profiles
        .values()
        .map(|p| ProfileView {
            name: p.name.clone(),
            count: p.mods.len(),
            active: rec.active_profile.as_deref() == Some(p.name.as_str()),
            modpack: p.modpack.clone(),
            updated: p.updated,
            mods: p.mods.iter().map(|m| m.title.clone()).collect(),
        })
        .collect();
    profiles.sort_by(|a, b| b.active.cmp(&a.active).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    GameState { game: view(st, g), mods, base_missing, profiles, steam_vanilla: steam_vanilla(st, g), steam_switchable }
}

fn progress(app: &AppHandle, game: &str, key: &str, title: &str, stage: &str, fraction: f64) {
    let _ = app.emit("progress", json!({ "gameId": game, "key": key, "title": title, "stage": stage, "fraction": fraction }));
}

/// Install `keys` plus anything they depend on that isn't installed yet.
/// `force` reinstalls the listed keys even if present (used for updates).
/// Keys in `explicit` are mods the user asked for; everything else pulled in is a dependency.
/// The caller must hold `st.busy`.
async fn install_many(app: &AppHandle, st: &AppState, g: &Game, keys: Vec<String>, force: bool, explicit: &HashSet<String>) -> CmdResult<()> {
    let root = require_path(st, g)?;

    let installed: HashSet<String> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.keys().cloned().collect()).unwrap_or_default();
    let forced: HashSet<String> = if force { keys.iter().cloned().collect() } else { HashSet::new() };
    let mut order: Vec<Pkg> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<(String, bool)> = keys.iter().rev().map(|k| (k.clone(), false)).collect();
    let mut resolved: HashMap<String, Pkg> = HashMap::new();
    while let Some((key, expanded)) = stack.pop() {
        if expanded {
            if let Some(p) = resolved.remove(&key) {
                order.push(p);
            }
            continue;
        }
        if seen.contains(&key) || (installed.contains(&key) && !forced.contains(&key)) {
            continue;
        }
        seen.insert(key.clone());
        progress(app, &g.id, &key, &key, "resolve", -1.0);
        let p = resolve(st, g, &key).await?;
        stack.push((key.clone(), true));
        for d in p.deps.iter().rev() {
            stack.push((d.clone(), false));
        }
        resolved.insert(key, p);
    }

    for p in order {
        // A game only ever gets one loader (dependencies sometimes name a different BepInExPack).
        if p.layout == Layout::Root && !forced.contains(&p.key) {
            let has_loader = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.values().any(|m| m.layout == "root")).unwrap_or(false);
            if has_loader {
                continue;
            }
        }
        let bytes: Vec<u8> = if p.layout == Layout::Modpack {
            vec![]
        } else {
            let cached = cache_file(st, &p.key, &p.version);
            if let Ok(b) = std::fs::read(&cached) {
                b
            } else if p.download.is_empty() {
                return Err(format!("{} has no download.", p.title));
            } else {
                progress(app, &g.id, &p.key, &p.title, "download", 0.0);
                let (app2, gid, key, title) = (app.clone(), g.id.clone(), p.key.clone(), p.title.clone());
                let mut last = 0.0;
                let b = installer::download(&p.download, move |f| {
                    if f < 0.0 || f - last >= 0.02 || f >= 1.0 {
                        last = f;
                        progress(&app2, &gid, &key, &title, "download", f);
                    }
                })
                .await
                .map_err(|e| format!("{}: {e}", p.title))?;
                if let Some(want) = &p.sha256 {
                    if !installer::sha256_hex(&b).eq_ignore_ascii_case(want) {
                        return Err(format!("{}: the download is corrupted (checksum mismatch). Try again.", p.title));
                    }
                }
                let _ = std::fs::create_dir_all(cache_dir(st));
                let _ = std::fs::write(&cached, &b);
                b
            }
        };
        progress(app, &g.id, &p.key, &p.title, "install", 1.0);

        let old = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(&p.key).cloned());
        let (root2, layout, folder) = (root.clone(), p.layout, p.folder.clone());
        let old_files = old.as_ref().map(|o| o.files.clone()).unwrap_or_default();
        let files = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            // Remove the old version's files first so renamed/deleted files don't linger.
            if !old_files.is_empty() && layout != Layout::Root {
                installer::remove_files(&root2, &old_files)?;
            }
            if layout == Layout::Modpack {
                return Ok(vec![]);
            }
            installer::extract(&root2, layout, &folder, &bytes)
        })
        .await
        .map_err(err)?
        .map_err(|e| format!("{}: {e}", p.title))?;

        let was_disabled = old.as_ref().map(|o| !o.enabled).unwrap_or(false);
        if was_disabled {
            installer::set_enabled(&root, &files, false).map_err(err)?;
        }
        let auto = if explicit.contains(&p.key) { false } else { old.as_ref().map(|o| o.auto).unwrap_or(true) };
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).installed.insert(
            p.key.clone(),
            Installed {
                key: p.key.clone(),
                title: p.title.clone(),
                author: p.author.clone(),
                version: p.version.clone(),
                layout: p.layout.as_str().into(),
                enabled: !was_disabled,
                adopted: false,
                icon: p.icon.clone(),
                files,
                dependencies: p.deps.clone(),
                auto,
            },
        );
        s.save();
        drop(s);
        st.latest.lock().unwrap().insert(format!("{}|{}", g.id, p.key), p.version.clone());
        progress(app, &g.id, &p.key, &p.title, "done", 1.0);
    }

    // Something you picked yourself is never removed as an "unused dependency".
    let mut s = st.store.lock().unwrap();
    for k in explicit {
        if let Some(m) = s.game(&g.id).installed.get_mut(k) {
            m.auto = false;
        }
    }
    s.save();
    Ok(())
}

/// Removes a mod, then any dependencies that nothing needs any more. Caller holds `st.busy`.
fn uninstall_inner(st: &AppState, g: &Game, root: &Path, key: &str) -> CmdResult<()> {
    let base: HashSet<String> = cached_base_keys(st, g).into_iter().collect();
    let mut queue = vec![key.to_string()];
    while let Some(k) = queue.pop() {
        let rec = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(&k).cloned());
        if let Some(m) = rec {
            installer::remove_files(root, &m.files).map_err(err)?;
            let mut s = st.store.lock().unwrap();
            s.game(&g.id).installed.remove(&k);
            s.save();
        }
        // Anything installed only as a dependency and no longer needed goes too.
        let s = st.store.lock().unwrap();
        if let Some(r) = s.data.games.get(&g.id) {
            let needed: HashSet<&String> = r.installed.values().flat_map(|m| m.dependencies.iter()).collect();
            for (dk, dm) in &r.installed {
                if dm.auto && !base.contains(dk) && !needed.contains(dk) && !queue.contains(dk) {
                    queue.push(dk.clone());
                }
            }
        }
    }
    Ok(())
}

fn set_enabled_inner(st: &AppState, g: &Game, root: &Path, key: &str, enabled: bool) -> CmdResult<()> {
    let files = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(key)).map(|m| m.files.clone()).unwrap_or_default();
    installer::set_enabled(root, &files, enabled).map_err(err)?;
    let mut s = st.store.lock().unwrap();
    if let Some(m) = s.game(&g.id).installed.get_mut(key) {
        m.enabled = enabled;
    }
    s.save();
    Ok(())
}

/// The mods you chose yourself (not the loader, not dependencies).
fn snapshot(st: &AppState, g: &Game) -> Vec<ProfileMod> {
    let base = cached_base_keys(st, g);
    let s = st.store.lock().unwrap();
    s.data
        .games
        .get(&g.id)
        .map(|r| {
            r.installed
                .values()
                .filter(|m| !m.auto && !base.contains(&m.key))
                .map(|m| ProfileMod { key: m.key.clone(), title: m.title.clone(), version: m.version.clone(), enabled: m.enabled })
                .collect()
        })
        .unwrap_or_default()
}

/// Keeps the active profile in step with what's installed.
fn sync_active(st: &AppState, g: &Game) {
    let mods = snapshot(st, g);
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);
    if let Some(name) = rec.active_profile.clone() {
        if let Some(p) = rec.profiles.get_mut(&name) {
            p.mods = mods;
            p.updated = now();
        }
    }
    s.save();
}

fn unique_name(st: &AppState, g: &Game, want: &str) -> String {
    let s = st.store.lock().unwrap();
    let taken = |n: &str| s.data.games.get(&g.id).map(|r| r.profiles.contains_key(n)).unwrap_or(false);
    let base = if want.trim().is_empty() { "Profile".to_string() } else { want.trim().chars().take(60).collect() };
    if !taken(&base) {
        return base;
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap()
}

fn put_profile(st: &AppState, g: &Game, p: Profile) {
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).profiles.insert(p.name.clone(), p);
    s.save();
}

/// Before switching away, make sure the current setup is saved somewhere.
fn keep_current_setup(st: &AppState, g: &Game) {
    let active = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.active_profile.clone());
    if active.is_some() {
        sync_active(st, g);
        return;
    }
    let mods = snapshot(st, g);
    if mods.is_empty() {
        return;
    }
    let name = unique_name(st, g, "Default");
    put_profile(st, g, Profile { name: name.clone(), mods, modpack: None, updated: now() });
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).active_profile = Some(name);
    s.save();
}

/// Makes the game folder match a saved profile. Caller holds `st.busy`. Returns mods that failed.
async fn apply_inner(app: &AppHandle, st: &AppState, g: &Game, name: &str) -> CmdResult<Vec<String>> {
    let root = require_path(st, g)?;
    let profile = st
        .store
        .lock()
        .unwrap()
        .data
        .games
        .get(&g.id)
        .and_then(|r| r.profiles.get(name).cloned())
        .ok_or_else(|| format!("No profile called {name}"))?;
    let target: HashSet<String> = profile.mods.iter().map(|m| m.key.clone()).collect();
    let base: HashSet<String> = cached_base_keys(st, g).into_iter().collect();

    // 1. Remove what this profile doesn't have.
    let current: Vec<(String, bool)> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.values().map(|m| (m.key.clone(), m.auto)).collect()).unwrap_or_default();
    for (k, auto) in current {
        if !auto && !base.contains(&k) && !target.contains(&k) {
            uninstall_inner(st, g, &root, &k)?;
        }
    }

    // 2. Install what it's missing (loader first). One at a time so one broken mod doesn't stop the rest.
    let explicit: HashSet<String> = target.clone();
    let loader = base_keys(st, g).await;
    install_many(app, st, g, loader.clone(), false, &loader.iter().cloned().collect()).await?;
    let mut failed = Vec::new();
    for m in &profile.mods {
        let one: HashSet<String> = [m.key.clone()].into();
        if let Err(e) = install_many(app, st, g, vec![m.key.clone()], false, &one).await {
            failed.push(format!("{}: {e}", if m.title.is_empty() { &m.key } else { &m.title }));
        }
    }
    let _ = explicit;

    // 3. On/off switches as saved.
    for m in &profile.mods {
        let installed = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.contains_key(&m.key)).unwrap_or(false);
        if installed {
            set_enabled_inner(st, g, &root, &m.key, m.enabled)?;
        }
    }
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).active_profile = Some(name.to_string());
    s.save();
    Ok(failed)
}

// ---------- commands: games & mods ----------

#[tauri::command]
async fn init(app: AppHandle, st: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let reg = Registry::load(&st.data_dir).await;
    *st.registry.lock().unwrap() = reg;
    let games = tauri::async_runtime::spawn_blocking(steam::installed_games).await.map_err(err)?;
    *st.steam.lock().unwrap() = games;
    let reg = st.registry.lock().unwrap().clone();
    let games = all_games(&reg);
    fetch_missing_art(&st, &games).await;
    let views: Vec<GameView> = games.iter().map(|g| view(&st, g)).collect();
    let settings = st.store.lock().unwrap().data.settings.clone();
    Ok(json!({ "games": views, "settings": settings, "version": app.package_info().version.to_string() }))
}

#[tauri::command]
async fn game_state(game_id: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    base_keys(&st, &g).await;
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn filters(game_id: String, st: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    match &g.kind {
        Kind::Ts(c) => {
            let (sections, categories) = thunderstore::filters(c.id).await.map_err(err)?;
            Ok(json!({ "sections": sections, "categories": categories }))
        }
        Kind::Own(_) => {
            let reg = st.registry.lock().unwrap().clone();
            let mut cats: Vec<String> = reg.packages.iter().filter(|p| p.games.contains(&g.id)).map(|p| p.category.clone()).filter(|c| !c.is_empty()).collect();
            cats.sort();
            cats.dedup();
            let sections = json!([{ "id": "mods", "name": "Mods", "slug": "mods" }, { "id": "modpacks", "name": "Modpacks", "slug": "modpacks" }]);
            let categories: Vec<_> = cats.iter().map(|c| json!({ "id": c, "name": c, "slug": c })).collect();
            Ok(json!({ "sections": sections, "categories": categories }))
        }
    }
}

#[tauri::command]
async fn browse(game_id: String, q: String, page: u32, ordering: String, section: Option<String>, categories: Vec<String>, st: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    let installed: HashSet<String> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.keys().cloned().collect()).unwrap_or_default();
    let base = cached_base_keys(&st, &g);
    match &g.kind {
        Kind::Own(_) => {
            let reg = st.registry.lock().unwrap().clone();
            let ql = q.to_lowercase();
            let items: Vec<_> = reg
                .packages
                .iter()
                .filter(|p| p.games.contains(&g.id))
                .filter(|p| ql.is_empty() || p.name.to_lowercase().contains(&ql) || p.description.to_lowercase().contains(&ql))
                .filter(|p| match section.as_deref() {
                    Some("modpacks") => p.install_type == "modpack",
                    Some("mods") => p.install_type != "modpack",
                    _ => true,
                })
                .filter(|p| categories.is_empty() || categories.contains(&p.category))
                .map(|p| {
                    let key = format!("own:{}", p.id);
                    json!({
                        "key": key, "title": p.name, "author": p.author, "description": p.description,
                        "icon": p.icon, "version": p.version, "categories": [p.category],
                        "installed": installed.contains(&key), "required": base.contains(&key), "website": p.website,
                        "modpack": p.install_type == "modpack",
                    })
                })
                .collect();
            Ok(json!({ "items": items, "total": items.len(), "hasMore": false }))
        }
        Kind::Ts(c) => {
            let (items, total, more) = thunderstore::list_filtered(c.id, &q, page.max(1), &ordering, section.as_deref(), &categories).await.map_err(err)?;
            let items: Vec<_> = items
                .into_iter()
                .map(|l| {
                    let mut v = serde_json::to_value(&l).unwrap();
                    v["installed"] = json!(installed.contains(&l.key));
                    v["required"] = json!(base.contains(&l.key));
                    v["modpack"] = json!(l.categories.iter().any(|c| c == "Modpacks"));
                    v["pageUrl"] = json!(format!("https://thunderstore.io/c/{}/p/{}/", c.id, l.key.trim_start_matches("ts:").replacen('-', "/", 1)));
                    v
                })
                .collect();
            Ok(json!({ "items": items, "total": total, "hasMore": more }))
        }
    }
}

#[tauri::command]
async fn install(app: AppHandle, game_id: String, key: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let mut keys = base_keys(&st, &g).await;
    let mut explicit: HashSet<String> = keys.iter().cloned().collect();
    explicit.insert(key.clone());
    keys.push(key);
    let _guard = st.busy.lock().await;
    install_many(&app, &st, &g, keys, false, &explicit).await?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn ensure_base(app: AppHandle, game_id: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let keys = base_keys(&st, &g).await;
    if let Some(root) = game_path(&st, &g) {
        reconcile(&st, &g, &root);
        let _guard = st.busy.lock().await;
        let explicit = keys.iter().cloned().collect();
        install_many(&app, &st, &g, keys, false, &explicit).await?;
    }
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn uninstall(game_id: String, key: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    if cached_base_keys(&st, &g).contains(&key) {
        return Err("This is the base mod loader every other mod needs, so it can't be removed.".into());
    }
    let root = require_path(&st, &g)?;
    let _guard = st.busy.lock().await;
    uninstall_inner(&st, &g, &root, &key)?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn set_enabled(game_id: String, key: String, enabled: bool, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    if !enabled && cached_base_keys(&st, &g).contains(&key) {
        return Err("The base mod loader is always on.".into());
    }
    let root = require_path(&st, &g)?;
    let _guard = st.busy.lock().await;
    set_enabled_inner(&st, &g, &root, &key, enabled)?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn check_updates(game_id: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let mods: Vec<Installed> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.values().cloned().collect()).unwrap_or_default();
    let ts: Vec<String> = mods.iter().filter_map(|m| m.key.strip_prefix("ts:").map(String::from)).collect();
    let details = futures_util::future::join_all(ts.iter().map(|f| thunderstore::detail(f))).await;
    let mut latest = st.latest.lock().unwrap();
    for (full, d) in ts.iter().zip(details) {
        if let Ok(d) = d {
            latest.insert(format!("{}|ts:{full}", g.id), d.version);
        }
    }
    for m in mods.iter().filter(|m| m.key.starts_with("own:")) {
        if let Some(p) = st.registry.lock().unwrap().package(&m.key[4..]) {
            latest.insert(format!("{}|{}", g.id, m.key), p.version.clone());
        }
    }
    drop(latest);
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn update_mods(app: AppHandle, game_id: String, keys: Vec<String>, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    // Updating keeps each mod's "you picked it / it's a dependency" status.
    let explicit: HashSet<String> = {
        let s = st.store.lock().unwrap();
        keys.iter().filter(|k| s.data.games.get(&g.id).and_then(|r| r.installed.get(*k)).map(|m| !m.auto).unwrap_or(true)).cloned().collect()
    };
    install_many(&app, &st, &g, keys, true, &explicit).await?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn install_file(app: AppHandle, game_id: String, st: State<'_, AppState>) -> CmdResult<Option<GameState>> {
    let g = find_game(&st, &game_id)?;
    let root = require_path(&st, &g)?;
    let Some(file) = app.dialog().file().set_title("Pick a mod zip").add_filter("Mod zip", &["zip"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = file.into_path().map_err(err)?;
    let bytes = std::fs::read(&path).map_err(err)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mod".into());
    let manifest = installer::read_manifest(&bytes);
    let title = manifest.as_ref().and_then(|m| m["name"].as_str().map(String::from)).unwrap_or_else(|| stem.clone());
    let version = manifest.as_ref().and_then(|m| m["version_number"].as_str().or(m["version"].as_str()).map(String::from)).unwrap_or_else(|| "local".into());
    let author = manifest.as_ref().and_then(|m| m["author"].as_str().map(String::from)).unwrap_or_default();
    let folder: String = title.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let layout = if installer::zip_has_file(&bytes, "winhttp.dll") {
        Layout::Root
    } else if g.id == "chained-beasts" && installer::zip_has_file(&bytes, "pack.json") {
        Layout::CfPack
    } else {
        match &g.kind {
            Kind::Own(_) => Layout::Plugin,
            Kind::Ts(c) if c.melonloader => Layout::Melon,
            Kind::Ts(_) => Layout::Thunderstore,
        }
    };
    let key = format!("local:{folder}");
    // Keep a copy so profiles can bring this mod back later.
    let _ = std::fs::create_dir_all(cache_dir(&st));
    let _ = std::fs::write(cache_file(&st, &key, &version), &bytes);
    let meta = LocalMeta { title: title.clone(), author: author.clone(), version: version.clone(), layout: layout.as_str().into(), folder: folder.clone() };
    let _ = std::fs::write(cache_file(&st, &key, &version).with_extension("json"), serde_json::to_vec(&meta).unwrap_or_default());

    let _guard = st.busy.lock().await;
    let old = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(&key).cloned());
    if let Some(o) = &old {
        installer::remove_files(&root, &o.files).map_err(err)?;
    }
    let files = installer::extract(&root, layout, &folder, &bytes).map_err(err)?;
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).installed.insert(
            key.clone(),
            Installed { key, title, author, version, layout: layout.as_str().into(), enabled: true, adopted: false, icon: None, files, dependencies: vec![], auto: false },
        );
        s.save();
    }
    sync_active(&st, &g);
    Ok(Some(state_of(&st, &g)))
}

#[tauri::command]
async fn set_game_folder(app: AppHandle, game_id: String, st: State<'_, AppState>) -> CmdResult<Option<GameState>> {
    let g = find_game(&st, &game_id)?;
    let Some(dir) = app.dialog().file().set_title(format!("Where is {} installed?", g.name)).blocking_pick_folder() else {
        return Ok(None);
    };
    let p = dir.into_path().map_err(err)?;
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).custom_path = Some(p.to_string_lossy().to_string());
        s.save();
    }
    Ok(Some(state_of(&st, &g)))
}

/// Start the game through Steam. `vanilla` starts it with mods switched off for that run.
#[tauri::command]
fn launch(app: AppHandle, game_id: String, vanilla: bool, st: State<'_, AppState>) -> CmdResult<()> {
    let g = find_game(&st, &game_id)?;
    let id = *g.app_ids.first().ok_or("No Steam id for this game")?;
    // Make sure the game folder matches the Steam setting before starting.
    if let Some(root) = game_path(&st, &g) {
        apply_steam_mode(&st, &g, &root)?;
    }
    // The launch option decides whether mods load this time, whatever the config file says.
    let args = installer::launch_args(id, g.melon(), !vanilla);
    if let Some(steam) = steam::steam_root().map(|r| r.join("steam.exe")).filter(|p| p.exists()) {
        std::process::Command::new(steam).args(&args).spawn().map_err(err)?;
        return Ok(());
    }
    // No steam.exe found: the steam:// link carries the same options (Steam may ask to confirm them).
    let extra = args[2..].join(" ");
    app.opener().open_url(format!("steam://run/{id}//{}/", extra.replace(' ', "%20")), None::<&str>).map_err(err)
}

/// Per game: should starting from Steam run the normal game (true) or the modded one (false)?
#[tauri::command]
async fn set_steam_mode(game_id: String, vanilla: bool, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).steam_vanilla = Some(vanilla);
        s.save();
    }
    if let Some(root) = game_path(&st, &g) {
        apply_steam_mode(&st, &g, &root)?;
    }
    Ok(state_of(&st, &g))
}

#[tauri::command]
fn open_folder(app: AppHandle, game_id: String, st: State<'_, AppState>) -> CmdResult<()> {
    let g = find_game(&st, &game_id)?;
    let p = require_path(&st, &g)?;
    app.opener().open_path(p.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[tauri::command]
fn save_settings(settings: Settings, st: State<'_, AppState>) -> CmdResult<()> {
    {
        let mut s = st.store.lock().unwrap();
        s.data.settings = settings;
        s.save();
    }
    // Apply the Steam setting to games you've already opened in ModNest (others follow when you open them).
    let reg = st.registry.lock().unwrap().clone();
    let opened: HashSet<String> = st.store.lock().unwrap().data.games.keys().cloned().collect();
    for g in all_games(&reg).iter().filter(|g| opened.contains(&g.id)) {
        if let Some(root) = game_path(&st, g) {
            let _ = apply_steam_mode(&st, g, &root);
        }
    }
    Ok(())
}

#[tauri::command]
fn clear_cache(st: State<'_, AppState>) -> CmdResult<u64> {
    let dir = cache_dir(&st);
    let mut freed = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // Keep copies of mods installed from files: there's nowhere else to get them back from.
            if name.starts_with("local_") {
                continue;
            }
            freed += e.metadata().map(|m| m.len()).unwrap_or(0);
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(freed)
}

// ---------- commands: profiles & modpacks ----------

#[tauri::command]
async fn save_profile(game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods: snapshot(&st, &g), modpack: None, updated: now() });
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).active_profile = Some(name);
        s.save();
    }
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn new_profile(app: AppHandle, game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods: vec![], modpack: None, updated: now() });
    apply_inner(&app, &st, &g, &name).await?;
    Ok(state_of(&st, &g))
}

#[derive(Serialize)]
struct Applied {
    state: GameState,
    failed: Vec<String>,
}

#[tauri::command]
async fn apply_profile(app: AppHandle, game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<Applied> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let failed = apply_inner(&app, &st, &g, &name).await?;
    Ok(Applied { state: state_of(&st, &g), failed })
}

#[tauri::command]
async fn delete_profile(game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    {
        let mut s = st.store.lock().unwrap();
        let rec = s.game(&g.id);
        rec.profiles.remove(&name);
        if rec.active_profile.as_deref() == Some(name.as_str()) {
            rec.active_profile = None;
        }
        s.save();
    }
    Ok(state_of(&st, &g))
}

#[tauri::command]
async fn rename_profile(game_id: String, from: String, to: String, st: State<'_, AppState>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let to = unique_name(&st, &g, &to);
    {
        let mut s = st.store.lock().unwrap();
        let rec = s.game(&g.id);
        if let Some(mut p) = rec.profiles.remove(&from) {
            p.name = to.clone();
            rec.profiles.insert(to.clone(), p);
            if rec.active_profile.as_deref() == Some(from.as_str()) {
                rec.active_profile = Some(to);
            }
        }
        s.save();
    }
    Ok(state_of(&st, &g))
}

fn profile_mods(st: &AppState, g: &Game, name: &str) -> CmdResult<Vec<ProfileMod>> {
    sync_active(st, g);
    st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.profiles.get(name)).map(|p| p.mods.clone()).ok_or_else(|| format!("No profile called {name}"))
}

/// A code with the whole mod list inside it. No server involved.
#[tauri::command]
fn export_profile(game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<String> {
    let g = find_game(&st, &game_id)?;
    Ok(profiles::encode(&g.id, &name, &profile_mods(&st, &g, &name)?))
}

/// A short code from Thunderstore's profile service (works in r2modman / Thunderstore Mod Manager too).
#[tauri::command]
async fn share_thunderstore(game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<String> {
    let g = find_game(&st, &game_id)?;
    if !matches!(g.kind, Kind::Ts(_)) {
        return Err("Thunderstore codes only work for Thunderstore games. Use the ModNest code instead.".into());
    }
    let mods = profile_mods(&st, &g, &name)?;
    profiles::create_thunderstore_code(&name, &mods).await.map_err(err)
}

/// Accepts a ModNest code (any game) or a Thunderstore / r2modman code (for the given game).
/// Saves it as a new profile and returns where it went.
#[tauri::command]
async fn import_code(code: String, game_id: String, st: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let (gid, name, mods) = if let Some(x) = profiles::decode(&code) {
        x
    } else {
        let (name, mods) = profiles::fetch_thunderstore_code(&code).await.map_err(err)?;
        (game_id, name, mods)
    };
    let g = find_game(&st, &gid).map_err(|_| "That code is for a game ModNest doesn't know.".to_string())?;
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods, modpack: None, updated: now() });
    Ok(json!({ "gameId": g.id, "name": name }))
}

/// Profiles saved by Thunderstore Mod Manager / r2modman on this PC.
#[tauri::command]
fn manager_profiles() -> Vec<serde_json::Value> {
    profiles::local_manager_profiles()
        .into_iter()
        .map(|(c, name, mods)| json!({ "gameId": format!("ts-{c}"), "name": name, "count": mods.len(), "mods": mods.iter().map(|m| m.title.clone()).collect::<Vec<_>>() }))
        .collect()
}

#[tauri::command]
fn import_manager_profile(game_id: String, name: String, st: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    let community = game_id.trim_start_matches("ts-");
    let (_, _, mods) = profiles::local_manager_profiles()
        .into_iter()
        .find(|(c, n, _)| c == community && *n == name)
        .ok_or("That profile isn't there any more.")?;
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods, modpack: None, updated: now() });
    Ok(json!({ "gameId": g.id, "name": name }))
}

/// Installs a modpack as its own profile (like CurseForge). Your current mods are kept in their profile.
#[tauri::command]
async fn install_modpack(app: AppHandle, game_id: String, key: String, title: String, st: State<'_, AppState>) -> CmdResult<Applied> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let name = unique_name(&st, &g, &title);
    put_profile(&st, &g, Profile { name: name.clone(), mods: vec![ProfileMod { key: key.clone(), title, version: String::new(), enabled: true }], modpack: Some(key), updated: now() });
    let failed = apply_inner(&app, &st, &g, &name).await?;
    sync_active(&st, &g);
    Ok(Applied { state: state_of(&st, &g), failed })
}

// ---------- app self-update (GitHub Releases) ----------

#[tauri::command]
async fn check_app_update(app: AppHandle) -> CmdResult<Option<serde_json::Value>> {
    let updater = app.updater().map_err(err)?;
    match updater.check().await {
        Ok(Some(u)) => Ok(Some(json!({ "version": u.version, "notes": u.body }))),
        // No release yet, offline, etc. Not worth bothering anyone about.
        _ => Ok(None),
    }
}

#[tauri::command]
async fn install_app_update(app: AppHandle) -> CmdResult<()> {
    let updater = app.updater().map_err(err)?;
    let Some(update) = updater.check().await.map_err(err)? else { return Ok(()) };
    let app2 = app.clone();
    let mut got: usize = 0;
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk;
                let f = total.map(|t| got as f64 / t as f64).unwrap_or(-1.0);
                let _ = app2.emit("app-update-progress", f);
            },
            || {},
        )
        .await
        .map_err(err)?;
    app.restart();
}

/// Serves Steam's cached game art: http://art.localhost/<appid>/<capsule|hero|header|logo>
fn art_protocol(path: &str) -> tauri::http::Response<Vec<u8>> {
    let mut parts = path.trim_matches('/').split('/');
    let found = (|| {
        let id: u32 = parts.next()?.parse().ok()?;
        let file = steam::library_art(id, parts.next()?)?;
        Some((std::fs::read(&file).ok()?, if file.extension().map(|e| e == "png").unwrap_or(false) { "image/png" } else { "image/jpeg" }))
    })();
    match found {
        Some((bytes, mime)) => tauri::http::Response::builder().header("Content-Type", mime).header("Cache-Control", "max-age=86400").body(bytes).unwrap(),
        None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_uri_scheme_protocol("art", |_ctx, req| art_protocol(req.uri().path()))
        .setup(|app| {
            let data_dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("ModNest"));
            app.manage(AppState {
                store: Mutex::new(Store::open(data_dir.clone())),
                registry: Mutex::new(Registry::bundled()),
                steam: Mutex::new(HashMap::new()),
                latest: Mutex::new(HashMap::new()),
                busy: tauri::async_runtime::Mutex::new(()),
                data_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            game_state,
            filters,
            browse,
            install,
            ensure_base,
            uninstall,
            set_enabled,
            check_updates,
            update_mods,
            install_file,
            set_game_folder,
            launch,
            open_folder,
            open_url,
            save_settings,
            set_steam_mode,
            clear_cache,
            save_profile,
            new_profile,
            apply_profile,
            delete_profile,
            rename_profile,
            export_profile,
            share_thunderstore,
            import_code,
            manager_profiles,
            import_manager_profile,
            install_modpack,
            check_app_update,
            install_app_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ModNest");
}
