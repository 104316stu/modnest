# ModNest

A mod manager for Steam games, written in **Rust** (Tauri). It works like Thunderstore + CurseForge:
pick a game, browse mods, click Install. Dependencies and the mod loader come along automatically,
and mods can be switched off, updated or removed with one click.

- **Your games:** Chained Beasts (BepInEx 6 IL2CPP + ChainedForge) and IRON NEST (BepInEx 6 IL2CPP + Iron Nest Multiplayer).
  Their base mods are installed as soon as you open the game and can't be removed or switched off.
- **Every Thunderstore game** (about 376), matched to your Steam library by name, with search, sorting, categories and automatic dependencies.
  The loader (BepInExPack / MelonLoader) is installed with the first mod.
- **CurseForge:** add your free API key (Settings; get one at console.curseforge.com) to search and install CurseForge mods and modpacks.
  Mods whose authors don't allow downloads by other apps link to their CurseForge page instead.
- **Several sources per game:** Browse has a switch between your catalog, Thunderstore and CurseForge (e.g. Iron Nest shows your mods and the community's).
- **Add a game:** any folder. ModNest works out the loader (BepInEx 5, BepInEx 6 IL2CPP, MelonLoader, or plain files into a mods folder you choose),
  can link it to a Thunderstore game for browsing, and starts non-Steam games from their .exe.
- **My stuff:** your profiles in every game, Thunderstore Mod Manager profiles you can import, and everything you've published.
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

## Community mods (pull requests)

Anyone can suggest a mod: one `submissions/<id>.json` file in a pull request ([AGENTS.md](AGENTS.md) for AI agents,
[CONTRIBUTING.md](CONTRIBUTING.md) for people). GitHub checks it automatically; when you merge it, GitHub copies the zip into this
repo's releases and adds it to the catalog. Check one yourself: `python scripts/check_submission.py submissions/<id>.json`.

## Chained Beasts for friends (no ModNest needed)

Send this link: **https://github.com/104316stu/modnest/releases/download/chained-beasts-setup/ChainedBeasts-Mods-Setup.exe**

It finds the game, installs BepInEx + ChainedForge + the *Chained Beasts Complete Pack* (newest versions from the catalog),
and switches mods on for Steam. Running it again updates; typing R removes. Source: `src-tauri/src/bin/chained-setup.rs`.
To change what friends get, edit the `chained-beasts-complete` entry's `dependencies` in `registry/registry.json`.

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

From 0.3.0 the installer contains the native app (`native/`, no WebView). It's built and signed on your PC:

1. Bump the version in `src-tauri/tauri.conf.json`, `package.json` and the `Cargo.toml` files of `core`, `src-tauri` and `native`.
2. `powershell -ExecutionPolicy Bypass -File scripts\package-native.ps1`
   (builds `native`, then the usual NSIS installer with the native exe in it, signed with `modnest.key`).
3. Make `latest.json` and upload, as in steps 2–3 below.

The GitHub Action (`git tag vX.Y.Z && git push --tags`) still builds the older WebView installer, so don't use it for native releases.

**Or release from your own PC** (the key never leaves it; this is how v0.1.0 was published):

1. Bump the version, then build with the key in the environment:
   `$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $env:USERPROFILE\.tauri\modnest.key -Raw; npm run build`
2. Make `latest.json`: `{"version":"X.Y.Z","pub_date":"<UTC time>","platforms":{"windows-x86_64":{"signature":"<contents of the .sig file>","url":"https://github.com/104316stu/modnest/releases/download/vX.Y.Z/ModNest_X.Y.Z_x64-setup.exe"}}}`
3. `gh release create vX.Y.Z --latest <setup.exe> <setup.exe.sig> latest.json`

**Back up the key file.** Without it you can't ship updates to people who already installed ModNest.

## Building it yourself

Needs Node and Rust. This PC uses the GNU (MinGW) Rust toolchain, whose resource compiler breaks on the space in
`github clones\Mod App`, so point the build output somewhere without spaces:

```powershell
$env:CARGO_TARGET_DIR = "$env:USERPROFILE\.modnest-target"
npm install
npm run dev        # run it
npm run build      # installer -> $env:CARGO_TARGET_DIR\release\bundle\nsis
```

The native app (GPUI) also needs Microsoft's shader compiler. On the GNU toolchain there's no `fxc.exe`, so build the stand-in in
`tools/fxc-shim` (it calls `d3dcompiler_47.dll`, which ships with Windows) and point GPUI at it:

```powershell
cargo build --release --manifest-path toolsxc-shim\Cargo.toml
$env:GPUI_FXC_PATH = "$env:CARGO_TARGET_DIReleasexc.exe"
cargo run -p modnest-native     # run the native app
```

Tests: `cargo test -p modnest-core` (checks where files land for every package type, plus an install → disable → enable → remove round trip).

## Layout

```
core/                          the engine (no UI): install/update logic, Thunderstore, CurseForge, Steam, profiles
native/                        the app: native GPUI interface (window, screens, dialogs, self-updater)
src-tauri/                     the older WebView app (thin layer over core) and the installer config/icons
ui/                            the WebView screens (HTML/CSS/JS)
registry/registry.json         your mods catalog
scripts/publish-mod.ps1        publish a mod version
scripts/package-native.ps1     build the signed installer with the native app
tools/fxc-shim/                stand-in for fxc.exe so GPUI builds on the GNU toolchain
```
