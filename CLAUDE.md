# ModNest

- **Adding or updating a mod in the catalog?** Follow [AGENTS.md](AGENTS.md): one `submissions/<id>.json` file in a pull request. Don't edit `registry/registry.json` by hand for that.
- Working on the app itself: the engine is `core/` (crate `modnest-core`, no UI; tests in `core/tests`). The current app is `src-tauri/` (thin Tauri layer) + `ui/` (HTML/CSS/JS). The native GPUI app is `native/` (being built to replace the WebView; reference screenshots of the current UI are in `native/reference/`). See [README.md](README.md) for building (set `CARGO_TARGET_DIR` to a path without spaces on the MinGW toolchain) and `cargo test -p modnest-core`.
