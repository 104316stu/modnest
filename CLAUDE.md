# ModNest

- **Adding or updating a mod in the catalog?** Follow [AGENTS.md](AGENTS.md): one `submissions/<id>.json` file in a pull request. Don't edit `registry/registry.json` by hand for that.
- Working on the app itself: Rust (Tauri) backend in `src-tauri/src/`, UI in `ui/` (plain HTML/CSS/JS). See [README.md](README.md) for building (set `CARGO_TARGET_DIR` to a path without spaces on the MinGW toolchain) and `cd src-tauri; cargo test`.
