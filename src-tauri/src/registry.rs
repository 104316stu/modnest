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
        let fresh = async {
            let res = reqwest::Client::builder()
                .user_agent("ModNest/0.1")
                .timeout(std::time::Duration::from_secs(8))
                .build()?
                .get(REGISTRY_URL)
                .send()
                .await?
                .error_for_status()?;
            let text = res.text().await?;
            let reg: Registry = serde_json::from_str(&text)?;
            anyhow::Ok((reg, text))
        };
        if let Ok((reg, text)) = fresh.await {
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
