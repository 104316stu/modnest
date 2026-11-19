//! Our own mod catalog: a plain JSON file kept in a public GitHub repo (no server).
//! The app fetches the latest copy, caches it, and falls back to the copy built into the app.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const REGISTRY_URL: &str = "https://raw.githubusercontent.com/104316stu/modnest/main/registry/registry.json";
const BUNDLED: &str = include_str!("../../registry/registry.json");

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OwnGame {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub steam_app_ids: Vec<u32>,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub accent: Option<String>,
    #[serde(default)]
    pub base_packages: Vec<String>,
    /// Thunderstore community id to also browse for this game (optional; matched by Steam id/name otherwise).
    #[serde(default)]
    pub thunderstore: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OwnPackage {
    pub id: String,
    pub games: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub category: String,
    /// root | plugin | cf-pack
    pub install_type: String,
    #[serde(default)]
    pub install_folder: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Empty for modpacks (they're just a list of dependencies).
    #[serde(default)]
    pub download: String,
    #[serde(default)]
    pub sha256: Option<String>,
    /// Files that, if all present, mean the package is already installed (e.g. installed by hand).
    #[serde(default)]
    pub detect: Vec<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Registry {
    #[serde(default)]
    pub games: Vec<OwnGame>,
    #[serde(default)]
    pub packages: Vec<OwnPackage>,
}

impl Registry {
    pub fn bundled() -> Registry {
        serde_json::from_str(BUNDLED).expect("bundled registry.json is valid")
    }

    /// Latest from GitHub, else last cached copy, else the built-in one.
    pub async fn load(cache_dir: &Path) -> Registry {
        let cache = cache_dir.join("registry-cache.json");
        if let Ok(Fetched::New(reg, text, _)) = fetch(None).await {
            let _ = std::fs::create_dir_all(cache_dir);
            let _ = std::fs::write(&cache, text);
            return reg;
        }
        if let Some(reg) = std::fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str(&t).ok()) {
            return reg;
        }
        Registry::bundled()
    }

    pub fn package(&self, id: &str) -> Option<&OwnPackage> {
        self.packages.iter().find(|p| p.id == id)
    }
}

/// Same catalog through GitHub's API: always the newest commit (raw.githubusercontent.com can serve a
/// copy up to 5 minutes old). Unauthenticated, GitHub allows 60 requests an hour, so the app checks every
/// few minutes and falls back to the raw file if that limit is ever reached.
pub const REGISTRY_API: &str = "https://api.github.com/repos/104316stu/modnest/contents/registry/registry.json?ref=main";

pub enum Fetched {
    /// The catalog, its text, and the ETag to send next time.
    New(Registry, String, Option<String>),
    Unchanged,
}

/// Downloads the catalog. With `etag`, GitHub answers "unchanged" if nothing was committed since.
pub async fn fetch(etag: Option<&str>) -> anyhow::Result<Fetched> {
    let client = reqwest::Client::builder().user_agent("ModNest (mod manager)").timeout(std::time::Duration::from_secs(10)).build()?;
    // Tests point ModNest at their own catalog file.
    if let Ok(url) = std::env::var("MODNEST_REGISTRY_URL") {
        let text = client.get(url).send().await?.error_for_status()?.text().await?;
        let reg: Registry = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
        return Ok(Fetched::New(reg, text, None));
    }
    let mut req = client.get(REGISTRY_API).header("Accept", "application/vnd.github.raw+json").header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(e) = etag {
        req = req.header("If-None-Match", e);
    }
    match req.send().await {
        Ok(res) if res.status().as_u16() == 304 => return Ok(Fetched::Unchanged),
        Ok(res) if res.status().is_success() => {
            let tag = res.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
            let text = res.text().await?;
            let reg: Registry = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
            return Ok(Fetched::New(reg, text, tag));
        }
        _ => {}
    }
    // API unavailable or rate-limited: fall back to the raw file.
    let text = client.get(REGISTRY_URL).send().await?.error_for_status()?.text().await?;
    let reg: Registry = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    Ok(Fetched::New(reg, text, None))
}
