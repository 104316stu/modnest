# ModNest

A mod manager for Steam games, written in **Rust** (Tauri). It works like Thunderstore + CurseForge:
pick a game, browse mods, click Install. Dependencies and the mod loader come along automatically,
and mods can be switched off, updated or removed with one click.

- **Your games:** Chained Beasts (BepInEx 6 IL2CPP + ChainedForge) and IRON NEST (BepInEx 6 IL2CPP + Iron Nest Multiplayer).
  Their base mods are installed as soon as you open the game and can't be removed or switched off.
- **20 Thunderstore games** (Lethal Company, R.E.P.O., PEAK, ROUNDS, Risk of Rain 2, Schedule I …): the full Thunderstore catalog,
  with search, sorting and automatic dependencies. The loader (BepInExPack / MelonLoader) is installed with the first mod.
- **Install from file:** any mod `.zip` on your PC (Thunderstore zips, ChainedForge packs, plain plugin folders).
- **Profiles:** saved sets of mods per game. Switching swaps the mods in the game folder (downloads are cached, so it's quick).
  Share a profile as a **ModNest code** (the mod list is inside the code, no server) or a **Thunderstore code**
  (works with r2modman / Thunderstore Mod Manager too). Profiles from Thunderstore Mod Manager on this PC can be imported.
- **Modpacks:** browse a game's modpacks; installing one makes it its own profile, so your current mods stay saved.
- **Steam starts the normal game, ModNest's Play starts it with mods** (BepInEx games, on by default, changeable per game
  in the Play menu). ModNest sets `enabled = false` in the game's `doorstop_config.ini` and its Play button launches through
  Steam with `--doorstop-enabled true`. MelonLoader games (Schedule I) always load mods from Steam.
- Dependencies installed for a mod are removed again when nothing needs them. **Play without mods** starts the game with the loader switched off for that run.
- **Auto-update:** mods update when you open a game; the app updates itself from GitHub Releases.
- Finds your games by reading Steam's own library files, on every drive.

## Where things are stored (no server)

| What | Where |
|---|---|
| Your mods' catalog | `registry/registry.json` in this GitHub repo (read through raw.githubusercontent.com) |
| Your mod files | GitHub Release assets on this repo (one release per mod version) |
| Thunderstore mods | Downloaded straight from Thunderstore |
| App updates | GitHub Releases (`latest.json` + signed installer) |
| What's installed | `%APPDATA%\app.modnest.desktop\modnest-state.json` on each PC |

GitHub hosts it all for free; there's nothing to run or pay for. The repo has to be **public** so the app can read it without a login.

## Publishing a mod update

```powershell
.\scripts\publish-mod.ps1 -Id chainedforge -Version 1.1.0 -Source "path\to\folder-or-zip"
```

This zips the mod, uploads it as a GitHub Release, writes the new version + checksum into `registry.json` and pushes.
Everyone's ModNest picks it up the next time they open the game. For a brand-new mod, copy an entry in `registry.json` first.

`installType`: `plugin` (goes in `BepInEx/plugins/<installFolder>`), `cf-pack` (ChainedForge content pack → `BepInEx/ChainedForgePacks/<id>`),
`root` (game folder, for loaders). `detect` lists files that mean "already installed by hand", so existing setups are adopted instead of overwritten.

## Releasing a new version of the app

1. Bump `version` in `src-tauri/tauri.conf.json` (and `package.json`).
2. `git tag v0.2.0 && git push --tags`

The GitHub Action builds the installer, signs it and publishes the release. Installed copies show an "Update and restart" bar.
It needs one repo secret, `TAURI_SIGNING_PRIVATE_KEY`: the contents of `%USERPROFILE%\.tauri\modnest.key`.
**Back that key file up.** Without it you can't ship updates to people who already installed ModNest.

## Building it yourself

Needs Node and Rust. This PC uses the GNU (MinGW) Rust toolchain, whose resource compiler breaks on the space in
`github clones\Mod App`, so point the build output somewhere without spaces:

```powershell
$env:CARGO_TARGET_DIR = "$env:USERPROFILE\.modnest-target"
npm install
npm run dev        # run it
npm run build      # installer -> $env:CARGO_TARGET_DIR\release\bundle\nsis
```

Tests: `cd src-tauri; cargo test` (checks where files land for every package type, plus an install → disable → enable → remove round trip).

## Layout

```
src-tauri/src/lib.rs           commands the UI calls, dependency resolving, install/update logic
src-tauri/src/installer.rs     download, zip layout rules, enable/disable, uninstall
src-tauri/src/thunderstore.rs  Thunderstore API + supported games
src-tauri/src/steam.rs         Steam library detection
src-tauri/src/registry.rs      your catalog (GitHub, cached, with a built-in fallback)
src-tauri/src/profiles.rs      share codes, Thunderstore codes, Thunderstore Mod Manager import
ui/                            the screens (plain HTML/CSS/JS, no build step)
registry/registry.json         your mods catalog
scripts/publish-mod.ps1        publish a mod version
```
