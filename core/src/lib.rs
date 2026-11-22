//! ModNest core: everything the app does (games, mods, profiles, catalogs), with no UI.
//! The Tauri app and the native GPUI app both call these functions.
pub mod installer;
pub mod profiles;
pub mod registry;
pub mod steam;
pub mod store;
pub mod curseforge;
pub mod thunderstore;

use installer::Layout;
use registry::{OwnGame, Registry};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use store::{Installed, Profile, ProfileMod, Settings, Store};

pub type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

/// Receives progress events: ("progress", {gameId, key, title, stage, fraction}).
pub type Emitter = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

pub struct Core {
    emitter: Emitter,
    store: Mutex<Store>,
    registry: Mutex<Registry>,
    steam: Mutex<HashMap<u32, steam::SteamGame>>,
    /// Every Thunderstore game, with Steam ids matched where possible.
    communities: Mutex<Vec<thunderstore::CommunityInfo>>,
    /// Every CurseForge game (empty until a CurseForge key is set).
    cf_games: Mutex<Vec<curseforge::CfGame>>,
    /// Latest known version per "gameId|key", filled by check_updates.
    latest: Mutex<HashMap<String, String>>,
    /// GitHub's ETag for the catalog, so an unchanged catalog comes back as a tiny "not modified".
    catalog_etag: Mutex<Option<String>>,
    /// Only one change to a game folder at a time.
    busy: tokio::sync::Mutex<()>,
    data_dir: PathBuf,
}

// ---------- games ----------

/// One game and every place its mods can come from.
#[derive(Clone)]
pub struct Game {
    id: String,
    name: String,
    app_ids: Vec<u32>,
    /// Your own catalog (Chained Beasts, Iron Nest).
    own: Option<OwnGame>,
    ts: Option<thunderstore::CommunityInfo>,
    cf: Option<curseforge::CfGame>,
    custom: Option<store::CustomGame>,
    melon: bool,
}

impl Game {
    fn melon(&self) -> bool {
        self.melon
    }
    fn loader_name(&self) -> String {
        if let Some(o) = &self.own {
            return o.loader.clone();
        }
        if let Some(c) = &self.custom {
            return match c.loader.as_str() {
                "bepinex5" => "BepInEx 5".into(),
                "bepinex6-il2cpp" => "BepInEx 6 IL2CPP".into(),
                "melonloader" => "MelonLoader".into(),
                _ => "No loader (plain files)".into(),
            };
        }
        if self.melon { "MelonLoader".into() } else if self.ts.is_some() { "BepInEx".into() } else { "Mods folder".into() }
    }
    /// Uses a BepInEx-style loader (so Thunderstore routing applies to its mods).
    fn bepinex(&self) -> bool {
        match &self.custom {
            Some(c) => c.loader.starts_with("bepinex"),
            None => !self.melon && (self.own.is_some() || self.ts.is_some()),
        }
    }
    /// Ids a catalog package may use for this game: its own id, or "ts-<community>" for its Thunderstore game.
    fn catalog_ids(&self) -> Vec<String> {
        let mut v = vec![self.id.clone()];
        if let Some(c) = &self.ts {
            v.push(format!("ts-{}", c.id));
        }
        v
    }
    fn in_catalog(&self, p: &registry::OwnPackage) -> bool {
        let ids = self.catalog_ids();
        p.games.iter().any(|g| ids.contains(g))
    }
    fn sources(&self, reg: &Registry) -> Vec<&'static str> {
        let mut v = Vec::new();
        if reg.packages.iter().any(|p| p.install_type != "root" && self.in_catalog(p)) {
            v.push("own");
        }
        if self.ts.is_some() {
            v.push("ts");
        }
        if self.cf.is_some() {
            v.push("cf");
        }
        v
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    id: String,
    name: String,
    /// "own" (your catalog), "thunderstore", "curseforge" or "custom": decides the library group.
    source: &'static str,
    /// Where Browse can look: "own", "ts", "cf".
    sources: Vec<&'static str>,
    installed: bool,
    path: Option<String>,
    loader: String,
    app_id: Option<u32>,
    mod_count: usize,
    /// Wide image (Steam header).
    cover: Option<String>,
    /// Tall cover for the library.
    capsule: Option<String>,
    /// Big banner for the game page.
    hero: Option<String>,
    active_profile: Option<String>,
    custom: Option<store::CustomGame>,
    mods_folder: Option<String>,
    /// Thunderstore community id (for linking games added by hand).
    ts_id: Option<String>,
}

fn squash(s: &str) -> String {
    thunderstore::squash(s)
}

/// Builds the full game list: your catalog, every Thunderstore game, CurseForge games (with a key)
/// and games added by hand. The same game from several places becomes one entry with several sources.
fn all_games(st: &Core) -> Vec<Game> {
    let reg = st.registry.lock().unwrap().clone();
    let communities = st.communities.lock().unwrap().clone();
    let cf_games = st.cf_games.lock().unwrap().clone();
    let (custom, melon_set) = {
        let s = st.store.lock().unwrap();
        (s.data.custom_games.clone(), s.data.melon_games.clone())
    };
    let steam = st.steam.lock().unwrap().clone();
    // Installed Steam games by squashed name, to give Thunderstore/CurseForge games their Steam id.
    let steam_by_name: HashMap<String, u32> = steam.iter().map(|(id, g)| (squash(&g.name), *id)).collect();

    let ts_ids = |c: &thunderstore::CommunityInfo| -> Vec<u32> {
        if !c.steam_app_ids.is_empty() {
            return c.steam_app_ids.clone();
        }
        steam_by_name.get(&squash(&c.name)).map(|id| vec![*id]).unwrap_or_default()
    };
    let mut claimed_ts: HashSet<String> = HashSet::new();
    let mut claimed_cf: HashSet<u32> = HashSet::new();
    let mut out: Vec<Game> = Vec::new();

    let ts_for = |ids: &[u32], name: &str, explicit: Option<&str>| -> Option<thunderstore::CommunityInfo> {
        if let Some(e) = explicit {
            return communities.iter().find(|c| c.id == e).cloned();
        }
        communities.iter().find(|c| (!ids.is_empty() && ts_ids(c).iter().any(|i| ids.contains(i))) || squash(&c.name) == squash(name)).cloned()
    };
    let cf_for = |ids: &[u32], name: &str, explicit: Option<u32>| -> Option<curseforge::CfGame> {
        if let Some(e) = explicit {
            return cf_games.iter().find(|g| g.id == e).cloned();
        }
        let installed_names: Vec<String> = ids.iter().filter_map(|i| steam.get(i)).map(|g| squash(&g.name)).collect();
        cf_games.iter().find(|g| squash(&g.name) == squash(name) || installed_names.contains(&squash(&g.name))).cloned()
    };
    let melon_of = |ts: &Option<thunderstore::CommunityInfo>| ts.as_ref().map(|c| c.melonloader || melon_set.contains(&c.id)).unwrap_or(false);

    // 1. Games added by hand.
    for c in &custom {
        let ids: Vec<u32> = c.steam_app_id.into_iter().collect();
        let ts = c.thunderstore.as_deref().and_then(|e| communities.iter().find(|x| x.id == e).cloned());
        let cf = c.curseforge.and_then(|e| cf_games.iter().find(|g| g.id == e).cloned());
        if let Some(t) = &ts { claimed_ts.insert(t.id.clone()); }
        if let Some(g) = &cf { claimed_cf.insert(g.id); }
        out.push(Game { id: c.id.clone(), name: c.name.clone(), app_ids: ids, own: None, melon: c.loader == "melonloader", ts, cf, custom: Some(c.clone()) });
    }
    // 2. Your catalog's games (they also get the matching Thunderstore / CurseForge community).
    for g in &reg.games {
        let ts = ts_for(&g.steam_app_ids, &g.name, g.thunderstore.as_deref());
        let cf = cf_for(&g.steam_app_ids, &g.name, None);
        if let Some(t) = &ts { claimed_ts.insert(t.id.clone()); }
        if let Some(c) = &cf { claimed_cf.insert(c.id); }
        out.push(Game { id: g.id.clone(), name: g.name.clone(), app_ids: g.steam_app_ids.clone(), own: Some(g.clone()), melon: false, ts, cf, custom: None });
    }
    // 3. Every other Thunderstore game.
    for c in communities.iter().filter(|c| !claimed_ts.contains(&c.id)) {
        let ids = ts_ids(c);
        let cf = cf_for(&ids, &c.name, None).filter(|g| !claimed_cf.contains(&g.id));
        if let Some(g) = &cf { claimed_cf.insert(g.id); }
        let ts = Some(c.clone());
        out.push(Game { id: format!("ts-{}", c.id), name: c.name.clone(), app_ids: ids, own: None, melon: melon_of(&ts), ts, cf, custom: None });
    }
    // 4. CurseForge-only games.
    for g in cf_games.iter().filter(|g| !claimed_cf.contains(&g.id)) {
        let ids: Vec<u32> = steam_by_name.get(&squash(&g.name)).map(|i| vec![*i]).unwrap_or_default();
        out.push(Game { id: format!("cf-{}", g.slug), name: g.name.clone(), app_ids: ids, own: None, melon: false, ts: None, cf: Some(g.clone()), custom: None });
    }
    out
}

fn find_game(st: &Core, id: &str) -> CmdResult<Game> {
    all_games(st).into_iter().find(|g| g.id == id).ok_or_else(|| format!("Unknown game {id}"))
}

fn game_path(st: &Core, g: &Game) -> Option<PathBuf> {
    if let Some(c) = &g.custom {
        return Some(PathBuf::from(&c.path)).filter(|p| p.exists());
    }
    if let Some(p) = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.custom_path.clone()) {
        if Path::new(&p).exists() {
            return Some(PathBuf::from(p));
        }
    }
    let steam = st.steam.lock().unwrap();
    g.app_ids.iter().find_map(|id| steam.get(id).map(|s| s.path.clone()))
}

fn require_path(st: &Core, g: &Game) -> CmdResult<PathBuf> {
    game_path(st, g).ok_or_else(|| format!("{} isn't installed (or ModNest can't find it). Use \"Set game folder\".", g.name))
}

/// Folder (relative to the game) that plain mod files go into.
fn mods_folder(st: &Core, g: &Game) -> String {
    if let Some(f) = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.mods_folder.clone()) {
        return f;
    }
    g.custom.as_ref().map(|c| c.mods_folder.clone()).unwrap_or_default()
}

/// URL for art served from Steam's local cache (see the "art" protocol in run()).
fn art_url(app_id: u32, kind: &str) -> Option<String> {
    steam::library_art(app_id, kind).map(|_| format!("http://art.localhost/{app_id}/{kind}"))
}

fn view(st: &Core, g: &Game) -> GameView {
    let path = game_path(st, g);
    let reg = st.registry.lock().unwrap().clone();
    let (mod_count, header, active, folder) = {
        let s = st.store.lock().unwrap();
        let rec = s.data.games.get(&g.id);
        (
            rec.map(|r| r.installed.len()).unwrap_or(0),
            g.app_ids.iter().find_map(|id| s.data.art.get(id).cloned()),
            rec.and_then(|r| r.active_profile.clone()),
            rec.and_then(|r| r.mods_folder.clone()),
        )
    };
    let app_id = g.app_ids.first().copied();
    let header = header.or_else(|| app_id.map(|id| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/header.jpg")));
    let ts_cover = g.ts.as_ref().and_then(|c| c.cover.clone());
    let ts_hero = g.ts.as_ref().and_then(|c| c.hero.clone());
    let cf_cover = g.cf.as_ref().and_then(|c| c.cover.clone());
    GameView {
        id: g.id.clone(),
        name: g.name.clone(),
        source: if g.own.is_some() { "own" } else if g.custom.is_some() { "custom" } else if g.ts.is_some() { "thunderstore" } else { "curseforge" },
        sources: g.sources(&reg),
        installed: path.is_some(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        loader: g.loader_name(),
        app_id,
        mod_count,
        capsule: app_id.and_then(|id| art_url(id, "capsule")).or(ts_cover).or_else(|| cf_cover.clone()).or_else(|| header.clone()),
        hero: app_id.and_then(|id| art_url(id, "hero")).or(ts_hero).or_else(|| header.clone()).or(cf_cover),
        cover: header,
        active_profile: active,
        custom: g.custom.clone(),
        mods_folder: folder.or_else(|| g.custom.as_ref().map(|c| c.mods_folder.clone())),
        ts_id: g.ts.as_ref().map(|c| c.id.clone()),
    }
}

/// Steam header images for installed games (newer games use hashed URLs), fetched once and remembered.
async fn fetch_missing_art(st: &Core, games: &[Game]) {
    let missing: Vec<u32> = {
        let s = st.store.lock().unwrap();
        let steam = st.steam.lock().unwrap();
        games.iter().filter_map(|g| g.app_ids.first().copied()).filter(|id| steam.contains_key(id) && !s.data.art.contains_key(id)).collect()
    };
    if missing.is_empty() {
        return;
    }
    let Ok(client) = reqwest::Client::builder().user_agent("ModNest/0.2").timeout(std::time::Duration::from_secs(6)).build() else { return };
    let jobs = missing.into_iter().map(|id| {
        let client = client.clone();
        async move {
            let url = format!("https://store.steampowered.com/api/appdetails?appids={id}&filters=basic");
            let v: serde_json::Value = client.get(url).send().await.ok()?.json().await.ok()?;
            Some((id, v[id.to_string()]["data"]["header_image"].as_str()?.to_string()))
        }
    });
    let arts = futures_util::future::join_all(jobs).await;
    let mut s = st.store.lock().unwrap();
    s.data.art.extend(arts.into_iter().flatten());
    s.save();
}

/// Loads every Thunderstore game (cached for offline use) and gives them Steam ids from the hand-picked list.
async fn load_communities(st: &Core) {
    let cache = st.data_dir.join("thunderstore-games.json");
    let mut list = match thunderstore::fetch_all().await {
        Ok(l) if !l.is_empty() => {
            let _ = std::fs::write(&cache, serde_json::to_vec(&l).unwrap_or_default());
            l
        }
        _ => std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default(),
    };
    for k in thunderstore::known() {
        match list.iter_mut().find(|c| c.id == k.id) {
            Some(c) => {
                c.steam_app_ids = k.steam_app_ids.clone();
                c.melonloader |= k.melonloader;
            }
            None => list.push(k),
        }
    }
    *st.communities.lock().unwrap() = list;
}

/// Loads CurseForge's game list when a key is set (cached for offline use).
async fn load_cf_games(st: &Core) -> Result<(), String> {
    let key = st.store.lock().unwrap().data.settings.curseforge_key.clone();
    if key.trim().is_empty() {
        st.cf_games.lock().unwrap().clear();
        return Ok(());
    }
    let cache = st.data_dir.join("curseforge-games.json");
    match curseforge::games(&key).await {
        Ok(list) => {
            let _ = std::fs::write(&cache, serde_json::to_vec(&list).unwrap_or_default());
            *st.cf_games.lock().unwrap() = list;
            Ok(())
        }
        Err(e) => {
            if let Some(list) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
                *st.cf_games.lock().unwrap() = list;
            }
            Err(e.to_string())
        }
    }
}

// ---------- packages ----------

/// Everything needed to install one package.
#[derive(Clone, Debug)]
pub struct Pkg {
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
    /// Original file name (CurseForge files are sometimes a single .dll rather than a zip).
    file_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct LocalMeta {
    title: String,
    author: String,
    version: String,
    layout: String,
    folder: String,
}

fn cache_dir(st: &Core) -> PathBuf {
    st.data_dir.join("cache")
}

fn cache_file(st: &Core, key: &str, version: &str) -> PathBuf {
    cache_dir(st).join(format!("{}@{}.zip", safe_name(key), safe_name(version)))
}

fn cf_key(st: &Core) -> String {
    st.store.lock().unwrap().data.settings.curseforge_key.clone()
}

/// How a mod from Thunderstore/CurseForge/a file lands in this game.
fn mod_layout(g: &Game) -> Layout {
    if g.melon() {
        Layout::Melon
    } else if g.bepinex() {
        Layout::Thunderstore
    } else {
        Layout::Folder
    }
}

async fn resolve(st: &Core, g: &Game, key: &str) -> CmdResult<Pkg> {
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
            file_name: String::new(),
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
            file_name: String::new(),
        });
    }
    if let Some(id) = key.strip_prefix("cf:") {
        let m = curseforge::mod_detail(&cf_key(st), id).await.map_err(err)?;
        let download = m.download.clone().ok_or_else(|| {
            format!("{}'s author only allows downloading it from the CurseForge website{}", m.title, m.page_url.as_ref().map(|u| format!(": {u}")).unwrap_or_default())
        })?;
        let layout = mod_layout(g);
        let folder = if layout == Layout::Folder { mods_folder(st, g) } else { format!("cf-{}", safe_name(&m.title)) };
        return Ok(Pkg {
            key: key.to_string(),
            title: m.title,
            author: m.author,
            version: m.version,
            download,
            sha256: None,
            layout,
            folder,
            deps: m.dependencies,
            icon: m.icon,
            file_name: m.file_name,
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
            file_name: String::new(),
        });
    }
    Err(format!("Unknown mod {key}"))
}

/// Base loader for games whose loader is known up front (your catalog, games added by hand).
/// None means "ask Thunderstore" (it depends on what that community uses).
fn static_base(g: &Game) -> Option<Vec<String>> {
    if let Some(o) = &g.own {
        return Some(o.base_packages.iter().map(|p| format!("own:{p}")).collect());
    }
    if let Some(c) = &g.custom {
        return Some(match c.loader.as_str() {
            "bepinex5" => vec!["ts:BepInEx-BepInExPack".into()],
            "bepinex6-il2cpp" => vec!["own:bepinex-6-il2cpp".into()],
            "melonloader" => vec!["ts:LavaGang-MelonLoader".into()],
            _ => vec![],
        });
    }
    if g.ts.is_none() {
        return Some(vec![]); // CurseForge-only: plain files, no loader
    }
    None
}

/// Base (loader/framework) packages the game always needs.
async fn base_keys(st: &Core, g: &Game) -> Vec<String> {
    if let Some(v) = static_base(g) {
        return v;
    }
    if let Some(k) = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.base_key.clone()) {
        return vec![k];
    }
    let Some(c) = &g.ts else { return vec![] };
    match thunderstore::find_base(&c.id, g.melon()).await {
        Ok(Some((full, melon))) => {
            let k = format!("ts:{full}");
            let mut s = st.store.lock().unwrap();
            s.game(&g.id).base_key = Some(k.clone());
            if melon {
                s.data.melon_games.insert(c.id.clone());
            }
            s.save();
            vec![k]
        }
        _ => vec![],
    }
}

fn cached_base_keys(st: &Core, g: &Game) -> Vec<String> {
    static_base(g).unwrap_or_else(|| st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.base_key.clone()).into_iter().collect())
}

/// True when starting the game from Steam should run it without mods.
fn steam_vanilla(st: &Core, g: &Game) -> bool {
    let s = st.store.lock().unwrap();
    s.data.games.get(&g.id).and_then(|r| r.steam_vanilla).unwrap_or(s.data.settings.steam_vanilla)
}

/// Keeps the game's doorstop_config.ini in step with the Steam setting (BepInEx games only).
fn apply_steam_mode(st: &Core, g: &Game, root: &Path) -> CmdResult<()> {
    if g.melon() || !g.bepinex() {
        return Ok(());
    }
    installer::set_loader_default(root, !steam_vanilla(st, g)).map(|_| ()).map_err(err)
}

/// Matches up the store with what is really in the game folder:
/// adopts mods installed by hand, forgets mods whose files are gone.
fn reconcile(st: &Core, g: &Game, root: &Path) {
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
    // Catalog packages for this game (and catalog loaders used by games added by hand).
    for p in reg.packages.iter().filter(|p| (g.in_catalog(p) || base.contains(&format!("own:{}", p.id))) && !p.detect.is_empty()) {
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
    // Thunderstore loader packs (BepInExPack / MelonLoader) already in the folder.
    let present = if g.melon() { exists("version.dll") && exists("MelonLoader") } else { exists("winhttp.dll") && exists("BepInEx/core") };
    for key in base.iter().filter(|k| k.starts_with("ts:")) {
        if !present {
            rec.installed.remove(key);
        } else if !rec.installed.contains_key(key) {
            let title = thunderstore::pretty(key.trim_start_matches("ts:").split_once('-').map(|x| x.1).unwrap_or(key));
            rec.installed.insert(key.clone(), adopted(key.clone(), title, String::new(), "root", vec![], None));
        }
    }
    s.save();
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModView {
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
pub struct ProfileView {
    name: String,
    count: usize,
    active: bool,
    modpack: Option<String>,
    updated: u64,
    mods: Vec<String>,
    /// Imported from Thunderstore Mod Manager / r2modman.
    from_manager: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    game: GameView,
    mods: Vec<ModView>,
    base_missing: Vec<String>,
    profiles: Vec<ProfileView>,
    /// Starting from Steam runs the normal game (mods only via ModNest's Play).
    steam_vanilla: bool,
    /// Whether this game's loader can be switched that way (BepInEx with a doorstop config).
    steam_switchable: bool,
    /// Dependencies of installed mods that are missing ("ts:Namespace-Name" ...).
    missing_deps: Vec<String>,
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

fn state_of(st: &Core, g: &Game) -> GameState {
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
            from_manager: p.manager.is_some(),
        })
        .collect();
    profiles.sort_by(|a, b| b.active.cmp(&a.active).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    GameState { game: view(st, g), mods, base_missing, profiles, steam_vanilla: steam_vanilla(st, g), steam_switchable, missing_deps: missing_deps(st, g) }
}

fn emit_progress(e: &Emitter, game: &str, key: &str, title: &str, stage: &str, fraction: f64) {
    e("progress", json!({ "gameId": game, "key": key, "title": title, "stage": stage, "fraction": fraction }));
}

fn progress(st: &Core, game: &str, key: &str, title: &str, stage: &str, fraction: f64) {
    emit_progress(&st.emitter, game, key, title, stage, fraction);
}

/// Install `keys` plus anything they depend on that isn't installed yet.
/// `force` reinstalls the listed keys even if present (used for updates).
/// Keys in `explicit` are mods the user asked for; everything else pulled in is a dependency.
/// The caller must hold `st.busy`.
async fn install_many(st: &Core, g: &Game, keys: Vec<String>, force: bool, explicit: &HashSet<String>) -> CmdResult<()> {
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
        progress(st, &g.id, &key, &key, "resolve", -1.0);
        let p = resolve(st, g, &key).await?;
        stack.push((key.clone(), true));
        for d in p.deps.iter().rev() {
            stack.push((d.clone(), false));
        }
        resolved.insert(key, p);
    }

    for mut p in order {
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
            } else if let Some((b, v)) = from_manager_cache(st, g, &p.key, Some(&p.version))
                // Already on this PC from Thunderstore Mod Manager / r2modman: use that copy, even if it's an
                // older version, rather than downloading. An update (forced) always gets the newest.
                .or_else(|| if forced.contains(&p.key) { None } else { from_manager_cache(st, g, &p.key, None) })
            {
                p.version = v;
                let _ = std::fs::create_dir_all(cache_dir(st));
                let _ = std::fs::write(cache_file(st, &p.key, &p.version), &b);
                b
            } else if p.download.is_empty() {
                return Err(format!("{} has no download.", p.title));
            } else {
                progress(st, &g.id, &p.key, &p.title, "download", 0.0);
                let (app2, gid, key, title) = (st.emitter.clone(), g.id.clone(), p.key.clone(), p.title.clone());
                let mut last = 0.0;
                let b = installer::download(&p.download, move |f| {
                    if f < 0.0 || f - last >= 0.02 || f >= 1.0 {
                        last = f;
                        emit_progress(&app2, &gid, &key, &title, "download", f);
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
        progress(st, &g.id, &p.key, &p.title, "install", 1.0);

        let old = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(&p.key).cloned());
        let (root2, layout, folder, file_name) = (root.clone(), p.layout, p.folder.clone(), p.file_name.clone());
        let old_files = old.as_ref().map(|o| o.files.clone()).unwrap_or_default();
        let files = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            // Remove the old version's files first so renamed/deleted files don't linger.
            if !old_files.is_empty() && layout != Layout::Root {
                installer::remove_files(&root2, &old_files)?;
            }
            if layout == Layout::Modpack {
                return Ok(vec![]);
            }
            if !installer::is_zip(&bytes) && !file_name.is_empty() {
                return installer::write_single(&root2, layout, &folder, &file_name, &bytes);
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
        progress(st, &g.id, &p.key, &p.title, "done", 1.0);
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
fn uninstall_inner(st: &Core, g: &Game, root: &Path, key: &str) -> CmdResult<()> {
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

fn set_enabled_inner(st: &Core, g: &Game, root: &Path, key: &str, enabled: bool) -> CmdResult<()> {
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
fn snapshot(st: &Core, g: &Game) -> Vec<ProfileMod> {
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

/// Records what you installed, updated or switched on/off in the active profile. It only adds and
/// updates: a profile mod that isn't installed right now (e.g. its install failed) stays in the profile,
/// so selecting the profile again can still bring it back. Uninstalling removes it (profile_forget).
fn sync_active(st: &Core, g: &Game) {
    let mods = snapshot(st, g);
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);
    if let Some(name) = rec.active_profile.clone() {
        if let Some(p) = rec.profiles.get_mut(&name) {
            for m in mods {
                match p.mods.iter_mut().find(|x| x.key == m.key) {
                    Some(x) => *x = m,
                    None => p.mods.push(m),
                }
            }
            p.updated = now();
        }
    }
    s.save();
}

/// Takes a mod out of the active profile (you uninstalled it).
fn profile_forget(st: &Core, g: &Game, key: &str) {
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);
    if let Some(name) = rec.active_profile.clone() {
        if let Some(p) = rec.profiles.get_mut(&name) {
            p.mods.retain(|m| m.key != key);
            p.updated = now();
        }
    }
    s.save();
}

/// The package as a zip, taken from Thunderstore Mod Manager's / r2modman's download cache, if it's there.
/// `version: None` takes the newest version that manager has. Returns the zip and its version.
fn from_manager_cache(st: &Core, g: &Game, key: &str, version: Option<&str>) -> Option<(Vec<u8>, String)> {
    let full = key.strip_prefix("ts:")?;
    let community = g.ts.as_ref()?.id.clone();
    let comms = st.communities.lock().unwrap().clone();
    let (dir, v) = profiles::manager_package_dir(&comms, &community, full, version)?;
    Some((installer::zip_dir(&dir).ok()?, v))
}

/// Thunderstore Mod Manager / r2modman profiles for this game show up as ModNest profiles by themselves.
/// One that shares a name with a ModNest profile becomes "<name> (Thunderstore)". Later changes in the
/// other manager are picked up (unless that profile is the one in use), and a deleted import stays deleted.
fn sync_manager_profiles(st: &Core, g: &Game, found: &[(String, String, Vec<ProfileMod>)]) {
    let Some(community) = g.ts.as_ref().map(|t| t.id.clone()) else { return };
    let mine: Vec<&(String, String, Vec<ProfileMod>)> = found.iter().filter(|(c, _, _)| *c == community).collect();
    if mine.is_empty() {
        return;
    }
    let same = |a: &[ProfileMod], b: &[ProfileMod]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.key == y.key && x.enabled == y.enabled);
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);
    let mut changed = false;
    for (_, name, mods) in mine {
        let linked = rec.profiles.values().find(|p| p.manager.as_deref() == Some(name.as_str())).map(|p| p.name.clone());
        if let Some(pname) = linked {
            if rec.active_profile.as_deref() != Some(pname.as_str()) {
                if let Some(p) = rec.profiles.get_mut(&pname) {
                    if !same(&p.mods, mods) {
                        p.mods = mods.clone();
                        p.updated = now();
                        changed = true;
                    }
                }
            }
            continue;
        }
        if rec.manager_seen.contains(name) {
            continue;
        }
        let base = if rec.profiles.contains_key(name) { format!("{name} (Thunderstore)") } else { name.clone() };
        let pname = if !rec.profiles.contains_key(&base) { base.clone() } else { (2..).map(|i| format!("{base} {i}")).find(|n| !rec.profiles.contains_key(n)).unwrap() };
        rec.profiles.insert(pname.clone(), Profile { name: pname, mods: mods.clone(), manager: Some(name.clone()), updated: now(), ..Default::default() });
        rec.manager_seen.push(name.clone());
        changed = true;
    }
    if changed {
        s.save();
    }
}

fn unique_name(st: &Core, g: &Game, want: &str) -> String {
    let s = st.store.lock().unwrap();
    let taken = |n: &str| s.data.games.get(&g.id).map(|r| r.profiles.contains_key(n)).unwrap_or(false);
    let base = if want.trim().is_empty() { "Profile".to_string() } else { want.trim().chars().take(60).collect() };
    if !taken(&base) {
        return base;
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap()
}

/// Name for an imported profile: an empty profile with the same name (like the "Default" ModNest makes
/// on first open) is replaced instead of ending up with "Default (2)". If that empty profile is the one
/// in use, it's released first so switching to the import doesn't save the empty setup over it.
fn import_name(st: &Core, g: &Game, want: &str) -> String {
    let want = want.trim();
    let mut s = st.store.lock().unwrap();
    let rec = s.game(&g.id);
    if rec.profiles.get(want).map(|p| p.mods.is_empty()).unwrap_or(false) {
        if rec.active_profile.as_deref() == Some(want) {
            rec.active_profile = None;
        }
        s.save();
        return want.to_string();
    }
    drop(s);
    unique_name(st, g, want)
}

fn put_profile(st: &Core, g: &Game, p: Profile) {
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).profiles.insert(p.name.clone(), p);
    s.save();
}

/// Before switching away, make sure the current setup is saved somewhere.
fn keep_current_setup(st: &Core, g: &Game) {
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
    put_profile(st, g, Profile { name: name.clone(), mods, modpack: None, updated: now(), ..Default::default() });
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).active_profile = Some(name);
    s.save();
}

/// Makes the game folder match a saved profile. Caller holds `st.busy`. Returns mods that failed.
async fn apply_inner(st: &Core, g: &Game, name: &str) -> CmdResult<Vec<String>> {
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
    install_many(st, g, loader.clone(), false, &loader.iter().cloned().collect()).await?;
    let mut failed = Vec::new();
    for m in &profile.mods {
        let one: HashSet<String> = [m.key.clone()].into();
        if let Err(e) = install_many(st, g, vec![m.key.clone()], false, &one).await {
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
    // Anything an installed mod needs but that's missing (e.g. after an interrupted change) comes back too.
    let missing = missing_deps(st, g);
    if !missing.is_empty() {
        if let Err(e) = install_many(st, g, missing, false, &HashSet::new()).await {
            failed.push(format!("dependencies: {e}"));
        }
    }
    prune_orphans(st, g, &root)?;
    let mut s = st.store.lock().unwrap();
    s.game(&g.id).active_profile = Some(name.to_string());
    s.save();
    Ok(failed)
}

/// Dependencies of installed mods that aren't installed (e.g. left behind by an interrupted change).
/// A dependency on a mod loader counts as met when the game has any loader.
fn missing_deps(st: &Core, g: &Game) -> Vec<String> {
    let reg = st.registry.lock().unwrap().clone();
    let s = st.store.lock().unwrap();
    let Some(r) = s.data.games.get(&g.id) else { return vec![] };
    let has_loader = r.installed.values().any(|m| m.layout == "root");
    let is_loader = |k: &str| match k.strip_prefix("ts:") {
        Some(full) => thunderstore::is_loader(full),
        None => k.strip_prefix("own:").and_then(|id| reg.package(id)).map(|p| p.install_type == "root").unwrap_or(false),
    };
    let mut out: Vec<String> = r
        .installed
        .values()
        .flat_map(|m| m.dependencies.iter())
        .filter(|d| !r.installed.contains_key(*d) && !(has_loader && is_loader(d)))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Removes dependencies nothing needs any more (e.g. left behind when a mod failed to install).
fn prune_orphans(st: &Core, g: &Game, root: &Path) -> CmdResult<()> {
    let base: HashSet<String> = cached_base_keys(st, g).into_iter().collect();
    // Everything reachable from the mods you chose (and the loader) stays; other dependencies go.
    let orphans: Vec<String> = {
        let s = st.store.lock().unwrap();
        let Some(r) = s.data.games.get(&g.id) else { return Ok(()) };
        let mut keep: HashSet<String> = r.installed.iter().filter(|(k, m)| !m.auto || base.contains(*k)).map(|(k, _)| k.clone()).collect();
        let mut queue: Vec<String> = keep.iter().cloned().collect();
        while let Some(k) = queue.pop() {
            for d in r.installed.get(&k).map(|m| m.dependencies.clone()).unwrap_or_default() {
                if r.installed.contains_key(&d) && keep.insert(d.clone()) {
                    queue.push(d);
                }
            }
        }
        r.installed.keys().filter(|k| !keep.contains(*k)).cloned().collect()
    };
    for k in orphans {
        let rec = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.installed.get(&k).cloned());
        if let Some(m) = rec {
            installer::remove_files(root, &m.files).map_err(err)?;
            let mut s = st.store.lock().unwrap();
            s.game(&g.id).installed.remove(&k);
            s.save();
        }
    }
    Ok(())
}


// ---------- is the game running? ----------

/// Programs running from inside the game folder (the game itself, its crash handler...). Windows locks
/// mod files while they run, so changing mods then would stop halfway.
fn running_in(root: &Path) -> Vec<(sysinfo::Pid, String)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always));
    let root = root.to_string_lossy().to_lowercase();
    let root = root.trim_end_matches(['\\', '/']).to_string() + "\\";
    sys.processes()
        .iter()
        .filter_map(|(pid, p)| {
            let exe = p.exe()?.to_string_lossy().to_lowercase();
            exe.starts_with(&root).then(|| (*pid, p.name().to_string_lossy().to_string()))
        })
        .collect()
}

/// Refuses to change mods while the game is open. The UI recognises the "GAME_RUNNING|" prefix
/// and offers to close the game and try again.
fn ensure_not_running(st: &Core, g: &Game) -> CmdResult<()> {
    let Some(root) = game_path(st, g) else { return Ok(()) };
    if running_in(&root).is_empty() {
        return Ok(());
    }
    Err(format!("GAME_RUNNING|{}|{} is running. Windows locks mod files while the game is open, so close it first.", g.name, g.name))
}

/// The name of the game's program if it's running right now (for the UI to ask before changing mods).
pub fn game_running(st: &Core, game_id: String) -> CmdResult<Option<String>> {
    let g = find_game(&st, &game_id)?;
    Ok(game_path(&st, &g).and_then(|root| running_in(&root).into_iter().next().map(|(_, n)| n)))
}

/// Closes everything running from the game folder, then waits (up to 10 s) until it has exited.
pub async fn close_game(st: &Core, game_id: String) -> CmdResult<usize> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let g = find_game(&st, &game_id)?;
    let root = require_path(&st, &g)?;
    let procs = running_in(&root);
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    for (pid, _) in &procs {
        if let Some(p) = sys.process(*pid) {
            p.kill();
        }
    }
    for _ in 0..40 {
        if running_in(&root).is_empty() {
            // Give Windows a moment to release the file locks.
            std::thread::sleep(std::time::Duration::from_millis(400));
            return Ok(procs.len());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err(format!("{} didn't close. Close it yourself and try again.", g.name))
}

// ---------- commands: games & mods ----------

pub async fn init(st: &Core, version: String) -> CmdResult<serde_json::Value> {
    let reg = Registry::load(&st.data_dir).await;
    *st.registry.lock().unwrap() = reg;
    let games = tokio::task::spawn_blocking(steam::installed_games).await.map_err(err)?;
    *st.steam.lock().unwrap() = games;
    let (_, cf) = futures_util::join!(load_communities(&st), load_cf_games(&st));
    let games = all_games(&st);
    fetch_missing_art(&st, &games).await;
    let views: Vec<GameView> = games.iter().map(|g| view(&st, g)).collect();
    let settings = st.store.lock().unwrap().data.settings.clone();
    Ok(json!({ "games": views, "settings": settings, "version": version, "curseforgeError": cf.err() }))
}

pub async fn game_state(st: &Core, game_id: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let found = profiles::local_manager_profiles(&st.communities.lock().unwrap().clone());
    sync_manager_profiles(&st, &g, &found);
    base_keys(&st, &g).await;
    Ok(state_of(&st, &g))
}

/// Sections (Mods / Modpacks ...) and categories for one source of a game.
pub async fn filters(st: &Core, game_id: String, source: String) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    match source.as_str() {
        "ts" => {
            let c = g.ts.as_ref().ok_or("This game isn't on Thunderstore")?;
            let (sections, categories) = thunderstore::filters(&c.id).await.map_err(err)?;
            Ok(json!({ "sections": sections, "categories": categories }))
        }
        "cf" => {
            let c = g.cf.as_ref().ok_or("This game isn't on CurseForge")?;
            let classes = curseforge::classes(&cf_key(&st), c.id).await.map_err(err)?;
            let sections: Vec<_> = classes.iter().map(|(id, name)| json!({ "id": id.to_string(), "name": name, "slug": name.to_lowercase() })).collect();
            Ok(json!({ "sections": sections, "categories": [] }))
        }
        _ => {
            let reg = st.registry.lock().unwrap().clone();
            let mut cats: Vec<String> = reg.packages.iter().filter(|p| g.in_catalog(p)).map(|p| p.category.clone()).filter(|c| !c.is_empty()).collect();
            cats.sort();
            cats.dedup();
            let sections = json!([{ "id": "mods", "name": "Mods", "slug": "mods" }, { "id": "modpacks", "name": "Modpacks", "slug": "modpacks" }]);
            let categories: Vec<_> = cats.iter().map(|c| json!({ "id": c, "name": c, "slug": c })).collect();
            Ok(json!({ "sections": sections, "categories": categories }))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn browse(st: &Core, game_id: String, source: String, q: String, page: u32, ordering: String, section: Option<String>, categories: Vec<String>) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    let installed: HashSet<String> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.keys().cloned().collect()).unwrap_or_default();
    let base = cached_base_keys(&st, &g);
    match source.as_str() {
        "ts" => {
            let c = g.ts.as_ref().ok_or("This game isn't on Thunderstore")?;
            let (items, total, more) = thunderstore::list_filtered(&c.id, &q, page.max(1), &ordering, section.as_deref(), &categories).await.map_err(err)?;
            let items: Vec<_> = items
                .into_iter()
                .map(|l| {
                    let mut v = serde_json::to_value(&l).unwrap();
                    v["installed"] = json!(installed.contains(&l.key));
                    v["required"] = json!(base.contains(&l.key));
                    v["modpack"] = json!(l.categories.iter().any(|c| c == "Modpacks"));
                    v["source"] = json!("ts");
                    v["pageUrl"] = json!(format!("https://thunderstore.io/c/{}/p/{}/", c.id, l.key.trim_start_matches("ts:").replacen('-', "/", 1)));
                    v
                })
                .collect();
            Ok(json!({ "items": items, "total": total, "hasMore": more }))
        }
        "cf" => {
            let c = g.cf.as_ref().ok_or("This game isn't on CurseForge")?;
            let class_id = section.as_deref().and_then(|s| s.parse::<u32>().ok());
            let modpack_class = class_id.is_some() && section_is_modpacks(&st, c.id, class_id).await;
            let category = categories.first().and_then(|c| c.parse::<u32>().ok());
            let (items, total, more) = curseforge::search(&cf_key(&st), c.id, &q, page, &ordering, class_id, category, modpack_class).await.map_err(err)?;
            let items: Vec<_> = items
                .into_iter()
                .map(|l| {
                    let mut v = serde_json::to_value(&l).unwrap();
                    v["installed"] = json!(installed.contains(&l.key));
                    v["required"] = json!(false);
                    v["source"] = json!("cf");
                    v
                })
                .collect();
            Ok(json!({ "items": items, "total": total, "hasMore": more }))
        }
        _ => {
            let reg = st.registry.lock().unwrap().clone();
            let ql = q.to_lowercase();
            let items: Vec<_> = reg
                .packages
                .iter()
                .filter(|p| g.in_catalog(p))
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
                        "modpack": p.install_type == "modpack", "source": "own",
                    })
                })
                .collect();
            Ok(json!({ "items": items, "total": items.len(), "hasMore": false }))
        }
    }
}

async fn section_is_modpacks(st: &Core, cf_game: u32, class_id: Option<u32>) -> bool {
    let Some(id) = class_id else { return false };
    curseforge::classes(&cf_key(st), cf_game).await.map(|c| c.iter().any(|(i, n)| *i == id && n.to_lowercase().contains("modpack"))).unwrap_or(false)
}

pub async fn install(st: &Core, game_id: String, key: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let mut keys = base_keys(&st, &g).await;
    let mut explicit: HashSet<String> = keys.iter().cloned().collect();
    explicit.insert(key.clone());
    keys.push(key);
    let _guard = st.busy.lock().await;
    install_many(&st, &g, keys, false, &explicit).await?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

pub async fn ensure_base(st: &Core, game_id: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let keys = base_keys(&st, &g).await;
    if let Some(root) = game_path(&st, &g) {
        reconcile(&st, &g, &root);
        let _guard = st.busy.lock().await;
        let explicit = keys.iter().cloned().collect();
        install_many(&st, &g, keys, false, &explicit).await?;
    }
    Ok(state_of(&st, &g))
}

pub async fn uninstall(st: &Core, game_id: String, key: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    if cached_base_keys(&st, &g).contains(&key) {
        return Err("This is the base mod loader every other mod needs, so it can't be removed.".into());
    }
    let root = require_path(&st, &g)?;
    let _guard = st.busy.lock().await;
    uninstall_inner(&st, &g, &root, &key)?;
    profile_forget(&st, &g, &key);
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

pub async fn set_enabled(st: &Core, game_id: String, key: String, enabled: bool) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    if !enabled && cached_base_keys(&st, &g).contains(&key) {
        return Err("The base mod loader is always on.".into());
    }
    let root = require_path(&st, &g)?;
    let _guard = st.busy.lock().await;
    set_enabled_inner(&st, &g, &root, &key, enabled)?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

pub async fn check_updates(st: &Core, game_id: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let mods: Vec<Installed> = st.store.lock().unwrap().data.games.get(&g.id).map(|r| r.installed.values().cloned().collect()).unwrap_or_default();
    let ts: Vec<String> = mods.iter().filter_map(|m| m.key.strip_prefix("ts:").map(String::from)).collect();
    let details = futures_util::future::join_all(ts.iter().map(|f| thunderstore::detail(f))).await;
    let cf: Vec<String> = mods.iter().filter_map(|m| m.key.strip_prefix("cf:").map(String::from)).collect();
    let key = cf_key(&st);
    let cf_details = futures_util::future::join_all(cf.iter().map(|id| curseforge::mod_detail(&key, id))).await;
    let mut latest = st.latest.lock().unwrap();
    for (id, d) in cf.iter().zip(cf_details) {
        if let Ok(d) = d {
            latest.insert(format!("{}|cf:{id}", g.id), d.version);
        }
    }
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

pub async fn update_mods(st: &Core, game_id: String, keys: Vec<String>) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let _guard = st.busy.lock().await;
    // Updating keeps each mod's "you picked it / it's a dependency" status.
    let explicit: HashSet<String> = {
        let s = st.store.lock().unwrap();
        keys.iter().filter(|k| s.data.games.get(&g.id).and_then(|r| r.installed.get(*k)).map(|m| !m.auto).unwrap_or(true)).cloned().collect()
    };
    install_many(&st, &g, keys, true, &explicit).await?;
    sync_active(&st, &g);
    Ok(state_of(&st, &g))
}

pub async fn install_file(st: &Core, game_id: String, path: PathBuf) -> CmdResult<Option<GameState>> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let root = require_path(&st, &g)?;
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
    } else if g.own.is_some() && !installer::read_manifest(&bytes).is_some() {
        Layout::Plugin
    } else {
        mod_layout(&g)
    };
    // Plain-files games put the mod in their mods folder; everything else uses the mod's own name.
    let folder = if layout == Layout::Folder { mods_folder(&st, &g) } else { folder };
    let key = format!("local:{}", safe_name(&title));
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

pub async fn set_game_folder(st: &Core, game_id: String, p: PathBuf) -> CmdResult<Option<GameState>> {
    let g = find_game(&st, &game_id)?;
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).custom_path = Some(p.to_string_lossy().to_string());
        s.save();
    }
    Ok(Some(state_of(&st, &g)))
}

/// Start the game through Steam. `vanilla` starts it with mods switched off for that run.
pub fn launch(st: &Core, game_id: String, vanilla: bool) -> CmdResult<()> {
    let g = find_game(&st, &game_id)?;
    // Make sure the game folder matches the Steam setting before starting.
    if let Some(root) = game_path(&st, &g) {
        apply_steam_mode(&st, &g, &root)?;
    }
    let Some(id) = g.app_ids.first().copied() else {
        // Not a Steam game: start its .exe directly with the same mods on/off switch.
        let exe = g.custom.as_ref().and_then(|c| c.exe.clone()).ok_or("ModNest doesn't know how to start this game. Start it yourself.")?;
        let args: Vec<String> = installer::launch_args(0, g.melon(), !vanilla).into_iter().skip(2).collect();
        let args = if g.bepinex() || g.melon() { args } else { vec![] };
        std::process::Command::new(&exe).args(&args).current_dir(Path::new(&exe).parent().unwrap_or(Path::new("."))).spawn().map_err(err)?;
        return Ok(());
    };
    // The launch option decides whether mods load this time, whatever the config file says.
    let args = installer::launch_args(id, g.melon(), !vanilla);
    if let Some(steam) = steam::steam_root().map(|r| r.join("steam.exe")).filter(|p| p.exists()) {
        std::process::Command::new(steam).args(&args).spawn().map_err(err)?;
        return Ok(());
    }
    // No steam.exe found: the steam:// link carries the same options (Steam may ask to confirm them).
    let extra = args[2..].join(" ");
    open::that_detached(format!("steam://run/{id}//{}/", extra.replace(' ', "%20"))).map_err(err)
}

/// Per game: should starting from Steam run the normal game (true) or the modded one (false)?
pub async fn set_steam_mode(st: &Core, game_id: String, vanilla: bool) -> CmdResult<GameState> {
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

pub fn open_folder(st: &Core, game_id: String) -> CmdResult<()> {
    let g = find_game(&st, &game_id)?;
    let p = require_path(&st, &g)?;
    open::that_detached(p).map_err(err)
}

pub fn open_url(_st: &Core, url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened.".into());
    }
    open::that_detached(url).map_err(err)
}

pub fn save_settings(st: &Core, settings: Settings) -> CmdResult<()> {
    {
        let mut s = st.store.lock().unwrap();
        s.data.settings = settings;
        s.save();
    }
    // Apply the Steam setting to games you've already opened in ModNest (others follow when you open them).
    let opened: HashSet<String> = st.store.lock().unwrap().data.games.keys().cloned().collect();
    for g in all_games(&st).iter().filter(|g| opened.contains(&g.id)) {
        if let Some(root) = game_path(&st, g) {
            let _ = apply_steam_mode(&st, g, &root);
        }
    }
    Ok(())
}

pub fn clear_cache(st: &Core) -> CmdResult<u64> {
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

pub async fn save_profile(st: &Core, game_id: String, name: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    let _guard = st.busy.lock().await;
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods: snapshot(&st, &g), modpack: None, updated: now(), ..Default::default() });
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).active_profile = Some(name);
        s.save();
    }
    Ok(state_of(&st, &g))
}

pub async fn new_profile(st: &Core, game_id: String, name: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let name = unique_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods: vec![], modpack: None, updated: now(), ..Default::default() });
    apply_inner(&st, &g, &name).await?;
    Ok(state_of(&st, &g))
}

#[derive(Serialize)]
pub struct Applied {
    state: GameState,
    failed: Vec<String>,
    /// False when the game already had exactly this profile's mods (nothing installed, removed or switched).
    changed: bool,
}

/// What's in the game folder right now: each mod with its version and on/off state.
fn folder_fingerprint(st: &Core, g: &Game) -> Vec<(String, String, bool)> {
    let s = st.store.lock().unwrap();
    let mut v: Vec<_> = s.data.games.get(&g.id).map(|r| r.installed.values().map(|m| (m.key.clone(), m.version.clone(), m.enabled)).collect()).unwrap_or_default();
    v.sort();
    v
}

pub async fn apply_profile(st: &Core, game_id: String, name: String) -> CmdResult<Applied> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let before = folder_fingerprint(&st, &g);
    let was_active = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.active_profile.clone()) == Some(name.clone());
    let failed = apply_inner(&st, &g, &name).await?;
    let changed = !was_active || folder_fingerprint(&st, &g) != before;
    Ok(Applied { state: state_of(&st, &g), failed, changed })
}

pub async fn delete_profile(st: &Core, game_id: String, name: String) -> CmdResult<GameState> {
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

pub async fn rename_profile(st: &Core, game_id: String, from: String, to: String) -> CmdResult<GameState> {
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

fn profile_mods(st: &Core, g: &Game, name: &str) -> CmdResult<Vec<ProfileMod>> {
    sync_active(st, g);
    st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.profiles.get(name)).map(|p| p.mods.clone()).ok_or_else(|| format!("No profile called {name}"))
}

/// A code with the whole mod list inside it. No server involved.
pub fn export_profile(st: &Core, game_id: String, name: String) -> CmdResult<String> {
    let g = find_game(&st, &game_id)?;
    Ok(profiles::encode(&g.id, &name, &profile_mods(&st, &g, &name)?))
}

/// A short code from Thunderstore's profile service (works in r2modman / Thunderstore Mod Manager too).
pub async fn share_thunderstore(st: &Core, game_id: String, name: String) -> CmdResult<String> {
    let g = find_game(&st, &game_id)?;
    if g.ts.is_none() {
        return Err("Thunderstore codes only work for Thunderstore games. Use the ModNest code instead.".into());
    }
    let mods = profile_mods(&st, &g, &name)?;
    profiles::create_thunderstore_code(&name, &mods).await.map_err(err)
}

/// Accepts a ModNest code (any game) or a Thunderstore / r2modman code (for the given game).
/// Saves it as a new profile and returns where it went.
pub async fn import_code(st: &Core, code: String, game_id: String) -> CmdResult<serde_json::Value> {
    let (gid, name, mods) = if let Some(x) = profiles::decode(&code) {
        x
    } else {
        let (name, mods) = profiles::fetch_thunderstore_code(&code).await.map_err(err)?;
        (game_id, name, mods)
    };
    let g = find_game(&st, &gid).map_err(|_| "That code is for a game ModNest doesn't know.".to_string())?;
    let name = import_name(&st, &g, &name);
    put_profile(&st, &g, Profile { name: name.clone(), mods, modpack: None, updated: now(), ..Default::default() });
    Ok(json!({ "gameId": g.id, "name": name }))
}

/// Profiles saved by Thunderstore Mod Manager / r2modman on this PC.
pub fn manager_profiles(st: &Core) -> Vec<serde_json::Value> {
    let games = all_games(&st);
    profiles::local_manager_profiles(&st.communities.lock().unwrap().clone())
        .into_iter()
        .filter_map(|(c, name, mods)| {
            let g = games.iter().find(|g| g.ts.as_ref().map(|t| t.id == c).unwrap_or(false))?;
            Some(json!({ "gameId": g.id, "gameName": g.name, "name": name, "count": mods.len(), "mods": mods.iter().map(|m| m.title.clone()).collect::<Vec<_>>() }))
        })
        .collect()
}

pub fn import_manager_profile(st: &Core, game_id: String, name: String) -> CmdResult<serde_json::Value> {
    let g = find_game(&st, &game_id)?;
    let community = g.ts.as_ref().map(|t| t.id.clone()).ok_or("This game isn't on Thunderstore")?;
    let (_, _, mods) = profiles::local_manager_profiles(&st.communities.lock().unwrap().clone())
        .into_iter()
        .find(|(c, n, _)| *c == community && *n == name)
        .ok_or("That profile isn't there any more.")?;
    let found = vec![(community, name.clone(), mods)];
    sync_manager_profiles(&st, &g, &found);
    let linked = st.store.lock().unwrap().data.games.get(&g.id).and_then(|r| r.profiles.values().find(|p| p.manager.as_deref() == Some(name.as_str())).map(|p| p.name.clone()));
    let name = match linked {
        Some(n) => n,
        // imported before and then deleted: bring it back on request
        None => {
            let (_, _, mods) = found.into_iter().next().unwrap();
            let n = unique_name(&st, &g, &format!("{name} (Thunderstore)"));
            put_profile(&st, &g, Profile { name: n.clone(), mods, manager: Some(name.clone()), updated: now(), ..Default::default() });
            n
        }
    };
    Ok(json!({ "gameId": g.id, "name": name }))
}

/// Installs a modpack as its own profile (like CurseForge). Your current mods are kept in their profile.
pub async fn install_modpack(st: &Core, game_id: String, key: String, title: String) -> CmdResult<Applied> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let _guard = st.busy.lock().await;
    keep_current_setup(&st, &g);
    let name = unique_name(&st, &g, &title);
    // A CurseForge modpack is a zip listing its mods (manifest.json); its profile holds those mods.
    // Thunderstore and ModNest modpacks are packages whose dependencies are the mods.
    let mods = if let Some(id) = key.strip_prefix("cf:") {
        let m = curseforge::mod_detail(&cf_key(&st), id).await.map_err(err)?;
        let url = m.download.clone().ok_or_else(|| format!("{}'s author only allows downloading it from the CurseForge website.", m.title))?;
        let bytes = installer::download(&url, |_| {}).await.map_err(err)?;
        let list = curseforge::modpack_mods(&bytes).map_err(|e| format!("{} isn't a modpack ModNest can read: {e}", m.title))?;
        list.into_iter().map(|k| ProfileMod { key: k, title: String::new(), version: String::new(), enabled: true }).collect()
    } else {
        vec![ProfileMod { key: key.clone(), title, version: String::new(), enabled: true }]
    };
    put_profile(&st, &g, Profile { name: name.clone(), mods, modpack: Some(key), updated: now(), ..Default::default() });
    let failed = apply_inner(&st, &g, &name).await?;
    sync_active(&st, &g);
    Ok(Applied { state: state_of(&st, &g), failed, changed: true })
}

/// Installs dependencies that installed mods need but are missing.
pub async fn repair_dependencies(st: &Core, game_id: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    ensure_not_running(&st, &g)?;
    let _guard = st.busy.lock().await;
    let missing = missing_deps(&st, &g);
    if !missing.is_empty() {
        install_many(&st, &g, missing, false, &HashSet::new()).await?;
    }
    Ok(state_of(&st, &g))
}

/// Checks GitHub for a newer catalog while ModNest is running (new mods, new versions).
/// Returns what changed so the UI can refresh without a restart.
pub async fn refresh_catalog(st: &Core) -> CmdResult<serde_json::Value> {
    let etag = st.catalog_etag.lock().unwrap().clone();
    let (reg, text, tag) = match registry::fetch(etag.as_deref()).await {
        Ok(registry::Fetched::Unchanged) => return Ok(json!({ "changed": false })),
        Ok(registry::Fetched::New(reg, text, tag)) => (reg, text, tag),
        Err(e) => return Ok(json!({ "changed": false, "error": e.to_string() })),
    };
    *st.catalog_etag.lock().unwrap() = tag;
    let old = st.registry.lock().unwrap().clone();
    if serde_json::to_value(&old).ok() == serde_json::to_value(&reg).ok() {
        return Ok(json!({ "changed": false }));
    }
    let _ = std::fs::create_dir_all(&st.data_dir);
    let _ = std::fs::write(st.data_dir.join("registry-cache.json"), text);
    let mut added = Vec::new();
    let mut updated = Vec::new();
    for p in reg.packages.iter().filter(|p| p.install_type != "root") {
        match old.package(&p.id) {
            None => added.push(json!({ "id": p.id, "name": p.name, "version": p.version, "games": p.games })),
            Some(o) if o.version != p.version => updated.push(json!({ "id": p.id, "name": p.name, "version": p.version, "games": p.games })),
            _ => {}
        }
    }
    *st.registry.lock().unwrap() = reg;
    let games: Vec<GameView> = all_games(&st).iter().map(|g| view(&st, g)).collect();
    Ok(json!({ "changed": true, "added": added, "updated": updated, "games": games }))
}

// ---------- commands: CurseForge key, games added by hand, My stuff ----------

/// Saves the CurseForge API key after checking it works, then loads CurseForge's games.
pub async fn set_curseforge_key(st: &Core, key: String) -> CmdResult<serde_json::Value> {
    let key = key.trim().to_string();
    let count = if key.is_empty() { 0 } else { curseforge::test_key(&key).await.map_err(err)? };
    {
        let mut s = st.store.lock().unwrap();
        s.data.settings.curseforge_key = key;
        s.save();
    }
    load_cf_games(&st).await?;
    let games: Vec<GameView> = all_games(&st).iter().map(|g| view(&st, g)).collect();
    Ok(json!({ "games": games, "curseforgeGames": count }))
}

/// Works out which mod loader a game folder uses (or would need).
fn detect_loader(root: &Path) -> &'static str {
    let has = |p: &str| root.join(p).exists();
    if has("BepInEx/core/BepInEx.Unity.IL2CPP.dll") {
        return "bepinex6-il2cpp";
    }
    if has("BepInEx/core/BepInEx.dll") || has("BepInEx/core/BepInEx.Preloader.dll") {
        return "bepinex5";
    }
    if has("MelonLoader") || has("version.dll") && has("MelonLoader") {
        return "melonloader";
    }
    // A Unity game without a loader yet: IL2CPP builds need BepInEx 6, Mono builds BepInEx 5.
    if has("GameAssembly.dll") {
        return "bepinex6-il2cpp";
    }
    let mono = std::fs::read_dir(root).ok().map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with("_Data") && e.path().join("Managed").exists())).unwrap_or(false);
    if mono || has("MonoBleedingEdge") {
        return "bepinex5";
    }
    "none"
}

fn find_exe(root: &Path) -> Option<String> {
    let skip = ["unitycrashhandler", "unins", "crashreport", "launcher_helper", "vc_redist", "dxsetup"];
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            !skip.iter().any(|s| n.starts_with(s))
        })
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .map(|p| p.to_string_lossy().to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewGame {
    name: String,
    /// auto | bepinex5 | bepinex6-il2cpp | melonloader | none
    loader: String,
    #[serde(default)]
    mods_folder: String,
    #[serde(default)]
    thunderstore: Option<String>,
    #[serde(default)]
    curseforge: Option<u32>,
}

/// Adds any game by folder. Asks for the folder, then fills in what it can work out.
pub async fn add_custom_game(st: &Core, game: NewGame, path: PathBuf) -> CmdResult<Option<GameView>> {
    let name = if game.name.trim().is_empty() { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "My game".into()) } else { game.name.trim().to_string() };
    let loader = if game.loader == "auto" || game.loader.is_empty() { detect_loader(&path).to_string() } else { game.loader.clone() };
    let steam_app_id = st.steam.lock().unwrap().iter().find(|(_, s)| s.path == path).map(|(id, _)| *id);
    let slug: String = squash(&name);
    let id = {
        let s = st.store.lock().unwrap();
        let taken = |i: &str| s.data.custom_games.iter().any(|c| c.id == i);
        let base = format!("custom-{}", if slug.is_empty() { "game".into() } else { slug });
        if !taken(&base) { base.clone() } else { (2..).map(|n| format!("{base}-{n}")).find(|i| !taken(i)).unwrap() }
    };
    let c = store::CustomGame {
        id: id.clone(),
        name,
        path: path.to_string_lossy().to_string(),
        loader,
        mods_folder: game.mods_folder.trim().trim_matches(['/', '\\']).to_string(),
        thunderstore: game.thunderstore.filter(|t| !t.is_empty()),
        curseforge: game.curseforge,
        exe: find_exe(&path),
        steam_app_id,
    };
    {
        let mut s = st.store.lock().unwrap();
        s.data.custom_games.push(c);
        s.save();
    }
    let g = find_game(&st, &id)?;
    Ok(Some(view(&st, &g)))
}

/// Removes a game added by hand from ModNest (its files and mods are left alone).
pub fn remove_custom_game(st: &Core, game_id: String) -> CmdResult<()> {
    let mut s = st.store.lock().unwrap();
    s.data.custom_games.retain(|c| c.id != game_id);
    s.data.games.remove(&game_id);
    s.save();
    Ok(())
}

/// Where plain mod files go for this game (CurseForge mods for games without a loader).
pub fn set_mods_folder(st: &Core, game_id: String, folder: String) -> CmdResult<GameState> {
    let g = find_game(&st, &game_id)?;
    {
        let mut s = st.store.lock().unwrap();
        s.game(&g.id).mods_folder = Some(folder.trim().trim_matches(['/', '\\']).replace('\\', "/"));
        s.save();
    }
    Ok(state_of(&st, &g))
}

/// Everything that's yours: profiles across all games, and what you've published in the ModNest catalog.
pub fn my_stuff(st: &Core) -> serde_json::Value {
    let games = all_games(&st);
    let found = profiles::local_manager_profiles(&st.communities.lock().unwrap().clone());
    for g in games.iter().filter(|g| g.ts.as_ref().map(|t| found.iter().any(|(c, _, _)| *c == t.id)).unwrap_or(false)) {
        sync_manager_profiles(&st, g, &found);
    }
    let records = st.store.lock().unwrap().data.games.clone();
    let mut profiles = Vec::new();
    for (gid, rec) in records.iter().filter(|(_, r)| !r.profiles.is_empty()) {
        let Some(g) = games.iter().find(|g| &g.id == gid) else { continue };
        let v = view(&st, g);
        let list: Vec<_> = rec
            .profiles
            .values()
            .map(|p| json!({ "name": p.name, "count": p.mods.len(), "active": rec.active_profile.as_deref() == Some(p.name.as_str()), "modpack": p.modpack, "fromManager": p.manager.is_some(), "mods": p.mods.iter().map(|m| if m.title.is_empty() { m.key.clone() } else { m.title.clone() }).collect::<Vec<_>>() }))
            .collect();
        profiles.push(json!({ "gameId": g.id, "gameName": g.name, "capsule": v.capsule, "profiles": list }));
    }
    let reg = st.registry.lock().unwrap().clone();
    let published: Vec<_> = reg
        .games
        .iter()
        .map(|g| {
            let pk: Vec<_> = reg
                .packages
                .iter()
                .filter(|p| p.games.contains(&g.id) && p.install_type != "root")
                .map(|p| json!({ "id": p.id, "name": p.name, "version": p.version, "kind": if p.install_type == "modpack" { "Modpack" } else if p.install_type == "cf-pack" { "Pack" } else { "Mod" }, "icon": p.icon, "description": p.description }))
                .collect();
            json!({ "gameId": g.id, "gameName": g.name, "packages": pk })
        })
        .collect();
    json!({ "profiles": profiles, "published": published, "repo": "https://github.com/104316stu/modnest" })
}

impl Core {
    /// Creates the core. `data_dir` holds saved state and caches; `emitter` receives progress events.
    pub fn new(data_dir: PathBuf, emitter: Emitter) -> Core {
        Core {
            emitter,
            store: Mutex::new(Store::open(data_dir.clone())),
            registry: Mutex::new(Registry::bundled()),
            steam: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
            catalog_etag: Mutex::new(None),
            communities: Mutex::new(thunderstore::known()),
            cf_games: Mutex::new(Vec::new()),
            busy: tokio::sync::Mutex::new(()),
            data_dir,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/// Local file behind an "http://art.localhost/<appid>/<kind>" URL (Steam's cached game art).
pub fn art_file(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("http://art.localhost/")?;
    let (id, kind) = rest.split_once('/')?;
    steam::library_art(id.parse().ok()?, kind)
}
