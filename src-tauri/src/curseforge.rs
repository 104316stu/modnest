//! CurseForge connector: the official CurseForge API (needs a free API key from
//! https://console.curseforge.com, pasted into ModNest's settings).
//! Mods whose authors turned off third-party downloads are never fetched; ModNest links to the page instead.
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const BASE: &str = "https://api.curseforge.com";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CfGame {
    pub id: u32,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CfListing {
    pub key: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub icon: Option<String>,
    pub downloads: u64,
    pub rating: u64,
    pub updated: String,
    pub categories: Vec<String>,
    #[serde(rename = "pageUrl")]
    pub page_url: Option<String>,
    /// False when the author doesn't allow apps like ModNest to download it.
    pub downloadable: bool,
    pub modpack: bool,
}

#[derive(Clone, Debug)]
pub struct CfMod {
    pub key: String,
    pub title: String,
    pub author: String,
    /// The newest file's date (YYYY-MM-DD), so newer files compare as newer versions.
    pub version: String,
    pub download: Option<String>,
    pub file_name: String,
    pub icon: Option<String>,
    pub page_url: Option<String>,
    /// Required dependencies as "cf:<modId>".
    pub dependencies: Vec<String>,
}

fn client(key: &str) -> Result<reqwest::Client> {
    if key.trim().is_empty() {
        bail!("Add your CurseForge API key in Settings first.");
    }
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("x-api-key", key.trim().parse()?);
    h.insert("Accept", "application/json".parse()?);
    Ok(reqwest::Client::builder().user_agent("ModNest/0.2 (mod manager)").default_headers(h).timeout(std::time::Duration::from_secs(25)).build()?)
}

async fn get(key: &str, path: &str) -> Result<Value> {
    let res = client(key)?.get(format!("{BASE}{path}")).send().await?;
    match res.status().as_u16() {
        200 => Ok(res.json().await?),
        401 | 403 => bail!("CurseForge refused the API key. Check it in Settings."),
        s => bail!("CurseForge answered {s} for {path}"),
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Checks a key works. Returns how many games CurseForge offers.
pub async fn test_key(key: &str) -> Result<usize> {
    Ok(games(key).await?.len())
}

/// Every game on CurseForge (paged, 50 at a time).
pub async fn games(key: &str) -> Result<Vec<CfGame>> {
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        let d = get(key, &format!("/v1/games?index={index}&pageSize=50")).await?;
        let page = d["data"].as_array().cloned().unwrap_or_default();
        for g in &page {
            let a = &g["assets"];
            out.push(CfGame {
                id: g["id"].as_u64().unwrap_or(0) as u32,
                name: s(g, "name"),
                slug: s(g, "slug"),
                cover: a["coverUrl"].as_str().or(a["tileUrl"].as_str()).filter(|u| !u.is_empty()).map(String::from),
            });
        }
        let total = d["pagination"]["totalCount"].as_u64().unwrap_or(0) as usize;
        index += page.len();
        if page.is_empty() || index >= total || index > 2000 {
            break;
        }
    }
    Ok(out)
}

/// Top-level classes for a game ("Mods", "Modpacks", ...): (id, name).
pub async fn classes(key: &str, game_id: u32) -> Result<Vec<(u32, String)>> {
    let d = get(key, &format!("/v1/categories?gameId={game_id}&classesOnly=true")).await?;
    Ok(d["data"].as_array().map(|a| a.iter().map(|c| (c["id"].as_u64().unwrap_or(0) as u32, s(c, "name"))).collect()).unwrap_or_default())
}

/// ModNest sort names -> CurseForge sortField numbers.
fn sort_field(ordering: &str) -> u32 {
    match ordering {
        "top-rated" => 12,
        "last-updated" => 3,
        "newest" => 11,
        "popular" => 2,
        _ => 6, // total downloads
    }
}

pub fn listing_from(m: &Value, modpack: bool) -> CfListing {
    CfListing {
        key: format!("cf:{}", m["id"].as_u64().unwrap_or(0)),
        title: s(m, "name"),
        author: m["authors"].as_array().and_then(|a| a.first()).map(|a| s(a, "name")).unwrap_or_default(),
        description: s(m, "summary"),
        icon: m["logo"]["thumbnailUrl"].as_str().filter(|u| !u.is_empty()).map(String::from),
        downloads: m["downloadCount"].as_f64().unwrap_or(0.0) as u64,
        rating: m["thumbsUpCount"].as_u64().unwrap_or(0),
        updated: s(m, "dateModified"),
        categories: m["categories"].as_array().map(|a| a.iter().map(|c| s(c, "name")).collect()).unwrap_or_default(),
        page_url: m["links"]["websiteUrl"].as_str().filter(|u| !u.is_empty()).map(String::from),
        downloadable: m["allowModDistribution"].as_bool().unwrap_or(true),
        modpack,
    }
}

/// One page of mods. `class_id` narrows to a class such as Modpacks.
pub async fn search(key: &str, game_id: u32, q: &str, page: u32, ordering: &str, class_id: Option<u32>, category: Option<u32>, modpack: bool) -> Result<(Vec<CfListing>, u64, bool)> {
    const SIZE: u32 = 20;
    let index = (page.max(1) - 1) * SIZE;
    let mut path = format!("/v1/mods/search?gameId={game_id}&sortField={}&sortOrder=desc&pageSize={SIZE}&index={index}", sort_field(ordering));
    if !q.is_empty() {
        path.push_str(&format!("&searchFilter={}", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>()));
    }
    if let Some(c) = class_id {
        path.push_str(&format!("&classId={c}"));
    }
    if let Some(c) = category {
        path.push_str(&format!("&categoryId={c}"));
    }
    let d = get(key, &path).await?;
    let items: Vec<CfListing> = d["data"].as_array().map(|a| a.iter().map(|m| listing_from(m, modpack)).collect()).unwrap_or_default();
    let total = d["pagination"]["totalCount"].as_u64().unwrap_or(0);
    Ok((items, total, (index as u64 + SIZE as u64) < total.min(10_000)))
}

/// Picks the file to install from a mod: its main file, else the newest one.
pub fn mod_from(m: &Value) -> Result<CfMod> {
    let id = m["id"].as_u64().ok_or_else(|| anyhow!("CurseForge sent a mod without an id"))?;
    let files = m["latestFiles"].as_array().cloned().unwrap_or_default();
    let main = m["mainFileId"].as_u64();
    let file = files
        .iter()
        .find(|f| f["id"].as_u64() == main)
        .or_else(|| files.iter().max_by_key(|f| s(f, "fileDate")))
        .ok_or_else(|| anyhow!("{} has no files on CurseForge", s(m, "name")))?;
    let allowed = m["allowModDistribution"].as_bool().unwrap_or(true);
    Ok(CfMod {
        key: format!("cf:{id}"),
        title: s(m, "name"),
        author: m["authors"].as_array().and_then(|a| a.first()).map(|a| s(a, "name")).unwrap_or_default(),
        version: s(file, "fileDate").chars().take(10).collect(),
        download: if allowed { file["downloadUrl"].as_str().filter(|u| !u.is_empty()).map(String::from) } else { None },
        file_name: s(file, "fileName"),
        icon: m["logo"]["thumbnailUrl"].as_str().filter(|u| !u.is_empty()).map(String::from),
        page_url: m["links"]["websiteUrl"].as_str().filter(|u| !u.is_empty()).map(String::from),
        dependencies: file["dependencies"]
            .as_array()
            .map(|a| a.iter().filter(|d| d["relationType"].as_u64() == Some(3)).filter_map(|d| d["modId"].as_u64()).map(|i| format!("cf:{i}")).collect())
            .unwrap_or_default(),
    })
}

pub async fn mod_detail(key: &str, mod_id: &str) -> Result<CfMod> {
    let d = get(key, &format!("/v1/mods/{mod_id}")).await?;
    mod_from(&d["data"])
}

/// The mods listed in a CurseForge modpack zip (manifest.json "files"): "cf:<projectID>".
pub fn modpack_mods(zip_bytes: &[u8]) -> Result<Vec<String>> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))?;
    let mut s = String::new();
    zip.by_name("manifest.json")?.read_to_string(&mut s)?;
    let v: Value = serde_json::from_str(s.trim_start_matches('\u{feff}'))?;
    Ok(v["files"].as_array().map(|a| a.iter().filter_map(|f| f["projectID"].as_u64()).map(|i| format!("cf:{i}")).collect()).unwrap_or_default())
}
