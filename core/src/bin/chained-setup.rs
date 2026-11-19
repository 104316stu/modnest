//! Chained Beasts mods: one-click setup for friends.
//! Finds the game, installs BepInEx + ChainedForge + its packs and the host's pack settings,
//! all downloaded from the ModNest catalog on GitHub. Run it again to update; it can also remove the mods.
use anyhow::{bail, Context, Result};
use modnest_core::installer::{self, Layout};
use modnest_core::registry::{OwnPackage, Registry};
use modnest_core::steam;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const APP_ID: u32 = 3569420;

fn main() {
    let result = run();
    if let Err(e) = &result {
        println!("\n  Something went wrong:\n  {e:#}\n");
        println!("  Nothing was half-installed on purpose; just run this again. If it keeps failing, send this message to whoever gave you the file.");
    }
    print!("\n  Press Enter to close...");
    let _ = io::stdout().flush();
    let _ = io::stdin().lock().read_line(&mut String::new());
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn ask(prompt: &str) -> String {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().lock().read_line(&mut s);
    // Some ways of feeding input add an invisible byte-order mark in front; ignore it.
    s.trim().trim_start_matches('\u{feff}').trim().trim_matches('"').to_string()
}

fn run() -> Result<()> {
    println!();
    println!("  ==============================================");
    println!("     CHAINED BEASTS MODS  -  setup");
    println!("     BepInEx + ChainedForge + all mod packs");
    println!("  ==============================================\n");

    let root = find_game()?;
    println!("  Game found: {}\n", root.display());

    while game_running() {
        ask("  Chained Beasts is running. Close the game, then press Enter...");
    }

    let plugin = root.join("BepInEx/plugins/ChainedForge/ChainedForge.dll");
    if plugin.exists() {
        let a = ask("  The mods are already installed.\n  Press Enter to UPDATE them, or type R and press Enter to REMOVE them: ");
        if a.eq_ignore_ascii_case("r") {
            return remove(&root);
        }
        println!();
    }

    println!("  Getting the latest mod list...");
    let cache = std::env::temp_dir().join("ChainedBeastsModsSetup");
    let reg = runtime().block_on(Registry::load(&cache));
    let order = install_order(&reg)?;
    let steps = order.iter().filter(|p| p.install_type != "modpack").count() + 1;
    let mut n = 0;
    for p in &order {
        let layout = Layout::parse(&p.install_type);
        if layout == Layout::Modpack {
            continue;
        }
        n += 1;
        let label = format!("[{n}/{steps}] {} {}", p.name, p.version);
        if layout == Layout::Root && !p.detect.is_empty() && p.detect.iter().all(|d| root.join(d).exists()) {
            println!("  {label}: already installed");
            continue;
        }
        let bytes = fetch(p, &label)?;
        let folder = p.install_folder.clone().unwrap_or_else(|| p.id.clone());
        // Clear the previous copy first so files removed in a new version don't linger.
        match layout {
            Layout::Plugin => { let _ = std::fs::remove_dir_all(root.join("BepInEx/plugins").join(&folder)); }
            Layout::CfPack => { let _ = std::fs::remove_dir_all(root.join("BepInEx/ChainedForgePacks").join(&folder)); }
            _ => {}
        }
        installer::extract(&root, layout, &folder, &bytes).with_context(|| format!("Could not install {}", p.name))?;
        println!("  {label}: installed");
    }

    // Starting from Steam should load the mods.
    installer::set_loader_default(&root, true).context("Could not switch the mod loader on")?;
    println!("  [{steps}/{steps}] Mods switched on for Steam\n");

    println!("  ----------------------------------------------");
    println!("  All done! Start Chained Beasts from Steam as usual.");
    println!("  The first start takes a bit longer while BepInEx sets itself up.");
    println!("  The main menu now has a Mods button.");
    println!();
    println!("  Playing together: everyone needs the same mod version.");
    println!("  When there's an update, just run this file again.");
    println!("  ----------------------------------------------");
    Ok(())
}

fn find_game() -> Result<PathBuf> {
    // A folder dragged onto the .exe (or passed on the command line) wins.
    if let Some(p) = std::env::args().nth(1).map(PathBuf::from) {
        if p.join("ChainedBeasts.exe").exists() {
            return Ok(p);
        }
        println!("  {} doesn't look like the Chained Beasts folder; searching Steam instead.\n", p.display());
    }
    if let Some(g) = steam::installed_games().remove(&APP_ID) {
        return Ok(g.path);
    }
    println!("  Couldn't find Chained Beasts in your Steam library automatically.");
    println!("  (In Steam: right-click Chained Beasts > Manage > Browse local files, then copy that folder's address.)\n");
    for _ in 0..3 {
        let p = PathBuf::from(ask("  Paste or drag the Chained Beasts folder here and press Enter: "));
        if p.join("ChainedBeasts.exe").exists() {
            return Ok(p);
        }
        println!("  That folder doesn't have ChainedBeasts.exe in it. Try again.\n");
    }
    bail!("Chained Beasts wasn't found. Is it installed through Steam?")
}

fn game_running() -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ChainedBeasts.exe", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("chainedbeasts.exe"))
        .unwrap_or(false)
}

fn fetch(p: &OwnPackage, label: &str) -> Result<Vec<u8>> {
    let label = label.to_string();
    let mut last_pct = u32::MAX;
    let bytes = runtime().block_on(installer::download(&p.download, |f| {
        // Redraw only when the percentage changes.
        if f >= 0.0 && (f * 100.0) as u32 != last_pct {
            last_pct = (f * 100.0) as u32;
            let filled = (f * 24.0) as usize;
            print!("\r  {label}: downloading [{}{}] {:>3}%", "#".repeat(filled), ".".repeat(24 - filled.min(24)), (f * 100.0) as u32);
            let _ = io::stdout().flush();
        }
    }))
    .with_context(|| format!("Download failed for {}. Check your internet connection.", p.name))?;
    print!("\r{}\r", " ".repeat(70));
    if let Some(want) = &p.sha256 {
        if !installer::sha256_hex(&bytes).eq_ignore_ascii_case(want) {
            bail!("The {} download was damaged. Run this again.", p.name);
        }
    }
    Ok(bytes)
}

const COMPLETE_PACK: &str = "chained-beasts-complete";

/// Everything to install, dependencies first: the Complete Pack modpack from the catalog,
/// or (if it's ever missing) ChainedForge plus every Chained Beasts pack.
fn install_order(reg: &Registry) -> Result<Vec<OwnPackage>> {
    let roots: Vec<String> = if reg.package(COMPLETE_PACK).is_some() {
        vec![COMPLETE_PACK.into()]
    } else {
        std::iter::once("chainedforge".to_string())
            .chain(reg.packages.iter().filter(|p| p.install_type == "cf-pack" && p.games.iter().any(|g| g == "chained-beasts")).map(|p| p.id.clone()))
            .collect()
    };
    let mut order: Vec<OwnPackage> = Vec::new();
    fn visit(reg: &Registry, id: &str, order: &mut Vec<OwnPackage>, depth: usize) -> Result<()> {
        if order.iter().any(|p| p.id == id) {
            return Ok(());
        }
        if depth > 20 {
            bail!("The mod list has a dependency loop at '{id}'");
        }
        let p = reg.package(id).cloned().with_context(|| format!("'{id}' is missing from the mod list"))?;
        for d in &p.dependencies {
            visit(reg, d, order, depth + 1)?;
        }
        order.push(p);
        Ok(())
    }
    // The loader always comes first.
    visit(reg, "bepinex-6-il2cpp", &mut order, 0)?;
    for r in &roots {
        visit(reg, r, &mut order, 0)?;
    }
    Ok(order)
}

fn remove(root: &Path) -> Result<()> {
    std::fs::remove_dir_all(root.join("BepInEx/plugins/ChainedForge")).context("Could not remove ChainedForge. Is the game running?")?;
    let _ = std::fs::remove_dir_all(root.join("BepInEx/ChainedForgePacks"));
    installer::set_loader_default(root, false)?;
    println!("\n  Removed. Chained Beasts now starts without mods.");
    println!("  (BepInEx is left in place but switched off. Run this file again any time to reinstall.)");
    Ok(())
}

/// One background runtime for the downloads.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
}
