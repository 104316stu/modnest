// Checks where mod files end up for each kind of package.
use modnest_core::installer::*;
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
    let mods = modnest_core::profiles::parse_r2_yaml(yaml);
    assert_eq!(mods.len(), 2);
    assert_eq!(mods[0].key, "ts:BepInEx-BepInExPack");
    assert_eq!(mods[0].version, "5.4.2100");
    assert_eq!(mods[1].key, "ts:notnotnotswipez-MoreCompany");
    assert!(!mods[1].enabled);
    // export.r2x shape (share codes)
    let r2x = "profileName: Friends\nmods:\n  - name: notnotnotswipez-MoreCompany\n    version:\n      major: 1\n      minor: 9\n      patch: 1\n    enabled: true\n";
    let mods = modnest_core::profiles::parse_r2_yaml(r2x);
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].version, "1.9.1");
}

#[test]
fn modnest_codes_round_trip() {
    use modnest_core::store::ProfileMod;
    let mods = vec![
        ProfileMod { key: "ts:A-B".into(), title: "B".into(), version: "1.0.0".into(), enabled: true },
        ProfileMod { key: "local:x".into(), title: "x".into(), version: "1".into(), enabled: true },
    ];
    let code = modnest_core::profiles::encode("ts-repo", "My pack", &mods);
    let (g, n, m) = modnest_core::profiles::decode(&code).unwrap();
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

#[test]
fn reads_curseforge_mods() {
    use modnest_core::curseforge::{listing_from, mod_from};
    // Shape of a CurseForge API mod (trimmed).
    let m = serde_json::json!({
        "id": 1234, "name": "Better Thing", "summary": "Makes things better", "downloadCount": 5100.0, "thumbsUpCount": 7,
        "authors": [{"name": "Someone"}], "logo": {"thumbnailUrl": "https://x/logo.png"}, "links": {"websiteUrl": "https://www.curseforge.com/x/better-thing"},
        "allowModDistribution": true, "mainFileId": 2,
        "latestFiles": [
            {"id": 1, "fileName": "old.zip", "fileDate": "2024-01-01T00:00:00Z", "downloadUrl": "https://edge/old.zip", "dependencies": []},
            {"id": 2, "fileName": "BetterThing.dll", "fileDate": "2025-03-09T10:00:00Z", "downloadUrl": "https://edge/BetterThing.dll",
             "dependencies": [{"modId": 77, "relationType": 3}, {"modId": 88, "relationType": 2}]}
        ]
    });
    let d = mod_from(&m).unwrap();
    assert_eq!(d.key, "cf:1234");
    assert_eq!(d.file_name, "BetterThing.dll");
    assert_eq!(d.version, "2025-03-09");
    assert_eq!(d.dependencies, vec!["cf:77"], "only required dependencies");
    assert_eq!(d.download.as_deref(), Some("https://edge/BetterThing.dll"));
    let l = listing_from(&m, false);
    assert_eq!((l.downloads, l.author.as_str(), l.downloadable), (5100, "Someone", true));

    // Author turned off third-party downloads: never fetched, page link instead.
    let mut no = m.clone();
    no["allowModDistribution"] = serde_json::json!(false);
    assert!(mod_from(&no).unwrap().download.is_none());
    assert!(!listing_from(&no, false).downloadable);
}

#[test]
fn matches_game_names_across_stores() {
    use modnest_core::thunderstore::squash;
    assert_eq!(squash("IRON NEST: Heavy Turret Simulator Demo"), squash("IRON NEST: Heavy Turret Simulator"));
    assert_eq!(squash("R.E.P.O."), "repo");
    assert_eq!(squash("Liar's Bar"), squash("Liars Bar"));
    assert_eq!(squash("Gamble With Your Friends"), "gamblewithyourfriends");
}

#[test]
fn plain_folder_and_single_file_installs() {
    let p = plan(Layout::Folder, "Mods", &names(&["readme.md", "cool/thing.pak", "x.pak"]));
    let d: Vec<_> = p.iter().map(|(_, d)| d.as_str()).collect();
    assert_eq!(d, vec!["Mods/cool/thing.pak", "Mods/x.pak"]);
    let p = plan(Layout::Folder, "", &names(&["x.pak"]));
    assert_eq!(p[0].1, "x.pak");

    let game = std::env::temp_dir().join(format!("modnest-single-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(&game).unwrap();
    assert!(!is_zip(b"MZ not a zip"));
    let f = write_single(&game, Layout::Thunderstore, "cf-Thing", "Thing.dll", b"MZ").unwrap();
    assert_eq!(f, vec!["BepInEx/plugins/cf-Thing/Thing.dll"]);
    let f = write_single(&game, Layout::Melon, "x", "Thing.dll", b"MZ").unwrap();
    assert_eq!(f, vec!["Mods/Thing.dll"]);
    let f = write_single(&game, Layout::Folder, "Mods", "Thing.pak", b"x").unwrap();
    assert!(game.join(&f[0]).exists());
    assert!(write_single(&game, Layout::Folder, "Mods", "", b"x").is_err());
    let _ = std::fs::remove_dir_all(&game);
}

#[test]
fn short_codes_round_trip_and_old_codes_still_work() {
    use modnest_core::profiles::{decode, encode, encode_v1};
    use modnest_core::store::ProfileMod;
    let pm = |k: &str, on: bool| ProfileMod { key: k.into(), title: String::new(), version: "1.2.3".into(), enabled: on };
    let mods = vec![pm("ts:Pykess-ModdingUtils", true), pm("ts:willis81808-UnboundLib", false), pm("own:chainedforge", true), pm("cf:1234", true), pm("local:mine", true)];

    let code = encode("ts-rounds", "fvxzd expanded GG", &mods);
    assert!(code.starts_with("MN2-"));
    let (g, n, m) = decode(&code).unwrap();
    assert_eq!((g.as_str(), n.as_str()), ("ts-rounds", "fvxzd expanded GG"));
    let got: Vec<(String, bool)> = m.iter().map(|x| (x.key.clone(), x.enabled)).collect();
    assert_eq!(got.len(), 4, "files from disk aren't shareable");
    for want in [("ts:Pykess-ModdingUtils", true), ("ts:willis81808-UnboundLib", false), ("own:chainedforge", true), ("cf:1234", true)] {
        assert!(got.contains(&(want.0.to_string(), want.1)), "missing {want:?}");
    }

    // Chat apps wrap long codes: spaces and line breaks are ignored.
    let wrapped: String = code.chars().enumerate().flat_map(|(i, c)| if i % 40 == 39 { vec![c, '\n', ' '] } else { vec![c] }).collect();
    assert_eq!(decode(&wrapped).unwrap().2.len(), 4);

    // Codes shared before this change still import.
    let old = encode_v1("ts-rounds", "Old", &mods);
    assert!(old.starts_with("MODNEST1-"));
    assert_eq!(decode(&old).unwrap().2.len(), 4);

    // A big profile: the new code is far shorter than the old one.
    let many: Vec<ProfileMod> = (0..130).map(|i| pm(&format!("ts:{}-SomeCardPack{i}", ["Pykess", "willis81808", "BossSloth", "Root"][i % 4]), true)).collect();
    let (new_len, old_len) = (encode("ts-rounds", "Big", &many).len(), encode_v1("ts-rounds", "Big", &many).len());
    assert!(new_len * 3 < old_len, "new {new_len} vs old {old_len}");
    assert!(decode("MN2-not-a-real-code").is_none());
}

#[test]
fn installs_an_unpacked_package_from_another_managers_cache() {
    // Thunderstore Mod Manager keeps packages unpacked: cache/<Namespace-Name>/<version>/
    let base = std::env::temp_dir().join(format!("modnest-zipdir-{}", std::process::id()));
    let pkg = base.join("cache").join("Auth-Mod").join("1.0.0");
    let game = base.join("game");
    std::fs::create_dir_all(pkg.join("plugins").join("Sub")).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(pkg.join("manifest.json"), "{}").unwrap();
    std::fs::write(pkg.join("Mod.dll"), b"dll").unwrap();
    std::fs::write(pkg.join("plugins").join("Sub").join("A.dll"), b"a").unwrap();
    let bytes = zip_dir(&pkg).unwrap();
    let mut files = extract(&game, Layout::Thunderstore, "Auth-Mod", &bytes).unwrap();
    files.sort();
    assert!(files.contains(&"BepInEx/plugins/Auth-Mod/Mod.dll".to_string()));
    assert!(files.contains(&"BepInEx/plugins/Auth-Mod/Sub/A.dll".to_string()));
    assert_eq!(std::fs::read(game.join("BepInEx/plugins/Auth-Mod/Sub/A.dll")).unwrap(), b"a");
    let _ = std::fs::remove_dir_all(&base);
}
