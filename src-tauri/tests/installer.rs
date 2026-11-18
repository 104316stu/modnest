// Checks where mod files end up for each kind of package.
use modnest_lib::installer::*;
use std::path::Path;

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn bepinex_pack_wrapped_in_folder_goes_to_root() {
    let p = plan(Layout::Root, "x", &names(&["manifest.json", "icon.png", "BepInExPack/winhttp.dll", "BepInExPack/BepInEx/core/BepInEx.dll", "BepInExPack/doorstop_config.ini"]));
    let dests: Vec<_> = p.iter().map(|(_, d)| d.as_str()).collect();
    assert_eq!(dests, vec!["winhttp.dll", "BepInEx/core/BepInEx.dll", "doorstop_config.ini"]);
}

#[test]
fn bepinex_be_zip_at_root() {
    let p = plan(Layout::Root, "x", &names(&["winhttp.dll", ".doorstop_version", "dotnet/", "dotnet/coreclr.dll", "BepInEx/core/BepInEx.Core.dll"]));
    assert_eq!(p.len(), 4);
    assert!(p.iter().any(|(_, d)| d == "dotnet/coreclr.dll"));
}

#[test]
fn thunderstore_routes_folders() {
    let p = plan(Layout::Thunderstore, "Auth-Mod", &names(&["manifest.json", "Mod.dll", "plugins/Sub/A.dll", "BepInEx/config/a.cfg", "patchers/P.dll"]));
    let d: Vec<_> = p.iter().map(|(_, d)| d.as_str()).collect();
    assert_eq!(d, vec![
        "BepInEx/plugins/Auth-Mod/manifest.json",
        "BepInEx/plugins/Auth-Mod/Mod.dll",
        "BepInEx/plugins/Auth-Mod/Sub/A.dll",
        "BepInEx/config/a.cfg",
        "BepInEx/patchers/Auth-Mod/P.dll",
    ]);
}

#[test]
fn plugin_and_pack_layouts() {
    let p = plan(Layout::Plugin, "ChainedForge", &names(&["ChainedForge.dll", "packs/S/pack.json"]));
    assert_eq!(p[1].1, "BepInEx/plugins/ChainedForge/packs/S/pack.json");
    let p = plan(Layout::CfPack, "MyPack", &names(&["MyPack/pack.json", "MyPack/icons/a.png"]));
    assert_eq!(p[0].1, "BepInEx/ChainedForgePacks/MyPack/pack.json");
    assert_eq!(p[1].1, "BepInEx/ChainedForgePacks/MyPack/icons/a.png");
}

#[test]
fn rejects_escaping_paths() {
    assert!(safe_join(Path::new("C:/g"), "../evil.dll").is_err());
    assert!(safe_join(Path::new("C:/g"), "BepInEx/plugins/a.dll").is_ok());
}

#[test]
fn install_disable_enable_remove_roundtrip() {
    // Build a small zip in memory, install it into a temp "game", then toggle and remove it.
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("Cool.dll", o).unwrap();
        z.write_all(b"dll").unwrap();
        z.start_file("config/cool.cfg", o).unwrap();
        z.write_all(b"cfg").unwrap();
        z.finish().unwrap();
    }
    let game = std::env::temp_dir().join(format!("modnest-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(&game).unwrap();

    let files = extract(&game, Layout::Thunderstore, "A-Cool", buf.get_ref()).unwrap();
    assert!(game.join("BepInEx/plugins/A-Cool/Cool.dll").exists());
    assert!(game.join("BepInEx/config/cool.cfg").exists());

    set_enabled(&game, &files, false).unwrap();
    assert!(!game.join("BepInEx/plugins/A-Cool/Cool.dll").exists());
    assert!(game.join("BepInEx/plugins/A-Cool/Cool.dll.disabled").exists());
    set_enabled(&game, &files, true).unwrap();
    assert!(game.join("BepInEx/plugins/A-Cool/Cool.dll").exists());

    remove_files(&game, &files).unwrap();
    assert!(!game.join("BepInEx").exists(), "empty folders are cleaned up");
    let _ = std::fs::remove_dir_all(&game);
}

#[test]
fn reads_thunderstore_mod_manager_profiles() {
    // Shape of r2modman / Thunderstore Mod Manager mods.yml
    let yaml = "- manifestVersion: 1\n  name: BepInEx-BepInExPack\n  authorName: BepInEx\n  dependencies: []\n  versionNumber:\n    major: 5\n    minor: 4\n    patch: 2100\n  enabled: true\n- manifestVersion: 1\n  name: notnotnotswipez-MoreCompany\n  dependencies:\n    - BepInEx-BepInExPack-5.4.2100\n  versionNumber:\n    major: 1\n    minor: 9\n    patch: 1\n  enabled: false\n";
    let mods = modnest_lib::profiles::parse_r2_yaml(yaml);
    assert_eq!(mods.len(), 2);
    assert_eq!(mods[0].key, "ts:BepInEx-BepInExPack");
    assert_eq!(mods[0].version, "5.4.2100");
    assert_eq!(mods[1].key, "ts:notnotnotswipez-MoreCompany");
    assert!(!mods[1].enabled);
    // export.r2x shape (share codes)
    let r2x = "profileName: Friends\nmods:\n  - name: notnotnotswipez-MoreCompany\n    version:\n      major: 1\n      minor: 9\n      patch: 1\n    enabled: true\n";
    let mods = modnest_lib::profiles::parse_r2_yaml(r2x);
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].version, "1.9.1");
}

#[test]
fn modnest_codes_round_trip() {
    use modnest_lib::store::ProfileMod;
    let mods = vec![
        ProfileMod { key: "ts:A-B".into(), title: "B".into(), version: "1.0.0".into(), enabled: true },
        ProfileMod { key: "local:x".into(), title: "x".into(), version: "1".into(), enabled: true },
    ];
    let code = modnest_lib::profiles::encode("ts-repo", "My pack", &mods);
    let (g, n, m) = modnest_lib::profiles::decode(&code).unwrap();
    assert_eq!((g.as_str(), n.as_str(), m.len()), ("ts-repo", "My pack", 1), "files from disk aren't shareable");
}

#[test]
fn switches_loader_in_doorstop_config() {
    let d4 = "# General options\r\n[General]\r\n\r\n# Enable Doorstop?\r\nenabled = true\r\ntarget_assembly=BepInEx/core/BepInEx.Preloader.dll\r\nignore_disable_switch = false\r\n\r\n[UnityMono]\r\ndebug_enabled = false\r\n";
    let (off, changed) = edit_doorstop(d4, false);
    assert!(changed);
    assert!(off.contains("enabled = false\r\n"));
    assert!(off.contains("ignore_disable_switch = false"), "other keys untouched");
    assert!(off.contains("debug_enabled = false"), "other sections untouched");
    assert_eq!(edit_doorstop(&off, false).1, false, "no change when already off");
    let (on, _) = edit_doorstop(&off, true);
    assert_eq!(on, d4, "round trip restores the original file exactly");
    let d3 = "[UnityDoorstop]\nenabled=true\ntargetAssembly=BepInEx/core/BepInEx.Preloader.dll\n";
    assert!(edit_doorstop(d3, false).0.contains("enabled = false"));
}

#[test]
fn launch_args_turn_mods_on_or_off() {
    assert_eq!(launch_args(10, false, true), vec!["-applaunch", "10", "--doorstop-enabled", "true", "--doorstop-enable", "true"]);
    assert_eq!(launch_args(10, false, false)[3], "false");
    assert_eq!(launch_args(10, true, true), vec!["-applaunch", "10"]);
    assert_eq!(launch_args(10, true, false), vec!["-applaunch", "10", "--no-mods"]);
}
