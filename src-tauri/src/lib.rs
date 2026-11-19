//! ModNest (Tauri app): a thin layer over modnest-core. The core does all the work; this file
//! exposes it to the web UI in ../ui, plus the few things only Tauri does (file pickers, self-update).
use modnest_core as mn;
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::UpdaterExt;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

struct Shared {
    core: mn::Core,
}

#[tauri::command]
async fn init(app: AppHandle, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::init(&st.core, app.package_info().version.to_string()).await
}

#[tauri::command]
async fn install_file(app: AppHandle, game_id: String, st: State<'_, Shared>) -> CmdResult<Option<mn::GameState>> {
    let Some(file) = app.dialog().file().set_title("Pick a mod zip").add_filter("Mod zip", &["zip"]).blocking_pick_file() else {
        return Ok(None);
    };
    mn::install_file(&st.core, game_id, file.into_path().map_err(err)?).await
}

#[tauri::command]
async fn set_game_folder(app: AppHandle, game_id: String, st: State<'_, Shared>) -> CmdResult<Option<mn::GameState>> {
    let Some(dir) = app.dialog().file().set_title("Where is the game installed?").blocking_pick_folder() else {
        return Ok(None);
    };
    mn::set_game_folder(&st.core, game_id, dir.into_path().map_err(err)?).await
}

#[tauri::command]
async fn add_custom_game(app: AppHandle, game: mn::NewGame, st: State<'_, Shared>) -> CmdResult<Option<mn::GameView>> {
    let Some(dir) = app.dialog().file().set_title("Pick the game's folder (where its .exe is)").blocking_pick_folder() else {
        return Ok(None);
    };
    mn::add_custom_game(&st.core, game, dir.into_path().map_err(err)?).await
}

#[tauri::command]
fn game_running(game_id: String, st: State<'_, Shared>) -> CmdResult<Option<String>> {
    mn::game_running(&st.core, game_id)
}

#[tauri::command]
async fn close_game(game_id: String, st: State<'_, Shared>) -> CmdResult<usize> {
    mn::close_game(&st.core, game_id).await
}

#[tauri::command]
async fn game_state(game_id: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::game_state(&st.core, game_id).await
}

#[tauri::command]
async fn filters(game_id: String, source: String, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::filters(&st.core, game_id, source).await
}

#[tauri::command]
async fn browse(game_id: String, source: String, q: String, page: u32, ordering: String, section: Option<String>, categories: Vec<String>, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::browse(&st.core, game_id, source, q, page, ordering, section, categories).await
}

#[tauri::command]
async fn install(game_id: String, key: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::install(&st.core, game_id, key).await
}

#[tauri::command]
async fn ensure_base(game_id: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::ensure_base(&st.core, game_id).await
}

#[tauri::command]
async fn uninstall(game_id: String, key: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::uninstall(&st.core, game_id, key).await
}

#[tauri::command]
async fn set_enabled(game_id: String, key: String, enabled: bool, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::set_enabled(&st.core, game_id, key, enabled).await
}

#[tauri::command]
async fn check_updates(game_id: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::check_updates(&st.core, game_id).await
}

#[tauri::command]
async fn update_mods(game_id: String, keys: Vec<String>, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::update_mods(&st.core, game_id, keys).await
}

#[tauri::command]
fn launch(game_id: String, vanilla: bool, st: State<'_, Shared>) -> CmdResult<()> {
    mn::launch(&st.core, game_id, vanilla)
}

#[tauri::command]
async fn set_steam_mode(game_id: String, vanilla: bool, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::set_steam_mode(&st.core, game_id, vanilla).await
}

#[tauri::command]
fn open_folder(game_id: String, st: State<'_, Shared>) -> CmdResult<()> {
    mn::open_folder(&st.core, game_id)
}

#[tauri::command]
fn open_url(url: String, st: State<'_, Shared>) -> CmdResult<()> {
    mn::open_url(&st.core, url)
}

#[tauri::command]
fn save_settings(settings: mn::store::Settings, st: State<'_, Shared>) -> CmdResult<()> {
    mn::save_settings(&st.core, settings)
}

#[tauri::command]
fn clear_cache(st: State<'_, Shared>) -> CmdResult<u64> {
    mn::clear_cache(&st.core)
}

#[tauri::command]
async fn save_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::save_profile(&st.core, game_id, name).await
}

#[tauri::command]
async fn new_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::new_profile(&st.core, game_id, name).await
}

#[tauri::command]
async fn apply_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<mn::Applied> {
    mn::apply_profile(&st.core, game_id, name).await
}

#[tauri::command]
async fn delete_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::delete_profile(&st.core, game_id, name).await
}

#[tauri::command]
async fn rename_profile(game_id: String, from: String, to: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::rename_profile(&st.core, game_id, from, to).await
}

#[tauri::command]
fn export_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<String> {
    mn::export_profile(&st.core, game_id, name)
}

#[tauri::command]
async fn share_thunderstore(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<String> {
    mn::share_thunderstore(&st.core, game_id, name).await
}

#[tauri::command]
async fn import_code(code: String, game_id: String, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::import_code(&st.core, code, game_id).await
}

#[tauri::command]
fn manager_profiles(st: State<'_, Shared>) -> Vec<serde_json::Value> {
    mn::manager_profiles(&st.core)
}

#[tauri::command]
fn import_manager_profile(game_id: String, name: String, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::import_manager_profile(&st.core, game_id, name)
}

#[tauri::command]
async fn install_modpack(game_id: String, key: String, title: String, st: State<'_, Shared>) -> CmdResult<mn::Applied> {
    mn::install_modpack(&st.core, game_id, key, title).await
}

#[tauri::command]
async fn repair_dependencies(game_id: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::repair_dependencies(&st.core, game_id).await
}

#[tauri::command]
async fn refresh_catalog(st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::refresh_catalog(&st.core).await
}

#[tauri::command]
async fn set_curseforge_key(key: String, st: State<'_, Shared>) -> CmdResult<serde_json::Value> {
    mn::set_curseforge_key(&st.core, key).await
}

#[tauri::command]
fn remove_custom_game(game_id: String, st: State<'_, Shared>) -> CmdResult<()> {
    mn::remove_custom_game(&st.core, game_id)
}

#[tauri::command]
fn set_mods_folder(game_id: String, folder: String, st: State<'_, Shared>) -> CmdResult<mn::GameState> {
    mn::set_mods_folder(&st.core, game_id, folder)
}

#[tauri::command]
fn my_stuff(st: State<'_, Shared>) -> serde_json::Value {
    mn::my_stuff(&st.core)
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
        let file = mn::steam::library_art(id, parts.next()?)?;
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
            // MODNEST_DATA_DIR lets tests run with their own settings and saved state.
            let data_dir = std::env::var_os("MODNEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("ModNest")));
            let handle = app.handle().clone();
            let emitter: mn::Emitter = Arc::new(move |event: &str, payload: serde_json::Value| {
                let _ = handle.emit(event, payload);
            });
            app.manage(Shared { core: mn::Core::new(data_dir, emitter) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            game_running,
            close_game,
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
            set_steam_mode,
            open_folder,
            open_url,
            save_settings,
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
            repair_dependencies,
            refresh_catalog,
            set_curseforge_key,
            add_custom_game,
            remove_custom_game,
            set_mods_folder,
            my_stuff,
            check_app_update,
            install_app_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ModNest");
}
