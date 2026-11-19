//! Thunderstore connector: browse, search and resolve packages for supported games.
//! Uses Thunderstore's public API, so nothing is hosted by us.
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

const BASE: &str = "https://thunderstore.io";

pub struct Community {
    pub id: &'static str,
    pub name: &'static str,
    pub steam_app_ids: &'static [u32],
    pub melonloader: bool,
}

const fn c(id: &'static str, name: &'static str, steam_app_ids: &'static [u32]) -> Community {
    Community { id, name, steam_app_ids, melonloader: false }
}

pub const COMMUNITIES: &[Community] = &[
    c("lethal-company", "Lethal Company", &[1966720]),
    c("repo", "R.E.P.O.", &[3241660]),
    c("peak", "PEAK", &[3527290]),
    c("rounds", "ROUNDS", &[1557740]),
    c("riskofrain2", "Risk of Rain 2", &[632360]),
    c("content-warning", "Content Warning", &[2881650]),
    c("valheim", "Valheim", &[892970]),
    c("straftat", "STRAFTAT", &[2386720]),
    c("bopl-battle", "Bopl Battle", &[1686940]),
    c("mimesis", "MIMESIS", &[2827200]),
    c("super-battle-golf", "Super Battle Golf", &[4069520]),
    c("ultrakill", "ULTRAKILL", &[1229490]),
    c("h3vr", "H3VR", &[450540]),
    c("dyson-sphere-program", "Dyson Sphere Program", &[1366540]),
    c("webfishing", "WEBFISHING", &[3146520]),
    c("gtfo", "GTFO", &[493520]),
    c("inscryption", "Inscryption", &[1092790]),
    c("subnautica", "Subnautica", &[264710]),
    c("raft", "Raft", &[648800]),
    Community { id: "schedule-i", name: "Schedule I", steam_app_ids: &[3164500], melonloader: true },
];

/// A Thunderstore community (one game), from the hand-picked list above or Thunderstore's full list.
#[derive(Serialize, serde::Deserialize, Clone, Debug)]
pub struct CommunityInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub steam_app_ids: Vec<u32>,
    #[serde(default)]
    pub melonloader: bool,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
}

/// The hand-picked communities (Steam ids known up front).
pub fn known() -> Vec<CommunityInfo> {
    COMMUNITIES
        .iter()
        .map(|c| CommunityInfo { id: c.id.into(), name: c.name.into(), steam_app_ids: c.steam_app_ids.to_vec(), melonloader: c.melonloader, cover: None, hero: None })
        .collect()
}

/// Every game on Thunderstore (one request, ~400 entries, with artwork). Steam ids aren't included;
/// they're matched by name against the Steam library afterwards.
pub async fn fetch_all() -> Result<Vec<CommunityInfo>> {
    let d = get_json(&format!("{BASE}/api/cyberstorm/community/?page_size=1000")).await?;
    Ok(d["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| CommunityInfo {
                    id: s(c, "identifier"),
                    name: s(c, "name"),
                    steam_app_ids: vec![],
                    melonloader: false,
                    cover: c["cover_image_url"].as_str().map(String::from),
                    hero: c["hero_image_url"].as_str().map(String::from),
                })
                .filter(|c| !c.id.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

/// Lowercase letters and digits only, "&" as "and", and a trailing Demo/Playtest dropped,
/// so "IRON NEST: Heavy Turret Simulator Demo" matches Thunderstore's "IRON NEST".
pub fn squash(name: &str) -> String {
    let n = name.to_ascii_lowercase().replace('&', "and");
    let n = n.trim_end_matches(" demo").trim_end_matches(" playtest");
    n.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

#[derive(Serialize, Clone, Debug)]
pub struct Listing {
    pub key: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub icon: Option<String>,
    pub downloads: u64,
    pub rating: u64,
    pub updated: String,
    pub categories: Vec<String>,
    pub pinned: bool,
    pub deprecated: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Detail {
    pub key: String,
    pub title: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub icon: Option<String>,
    pub download: String,
    pub website: Option<String>,
    /// "Namespace-Name" without the version.
    pub dependencies: Vec<String>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("ModNest/0.1 (mod manager)")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("http client")
}

async fn get_json(url: &str) -> Result<Value> {
    let res = client().get(url).send().await?;
    if !res.status().is_success() {
        return Err(anyhow!("Thunderstore answered {} for {}", res.status(), url));
    }
    Ok(res.json().await?)
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub fn pretty(name: &str) -> String {
    name.replace('_', " ")
}

#[derive(Serialize, Clone, Debug)]
pub struct Filter {
    pub id: String,
    pub name: String,
    pub slug: String,
}

/// Sections (Mods, Libraries, Modpacks …) and categories a community offers.
pub async fn filters(community: &str) -> Result<(Vec<Filter>, Vec<Filter>)> {
    let d = get_json(&format!("{BASE}/api/cyberstorm/community/{community}/filters/")).await?;
    let read = |arr: &Value, id_key: &str| -> Vec<Filter> {
        arr.as_array()
            .map(|a| a.iter().map(|x| Filter { id: s(x, id_key), name: s(x, "name"), slug: s(x, "slug") }).collect())
            .unwrap_or_default()
    };
    let sections = read(&d["sections"], "uuid").into_iter().filter(|f| f.slug != "mods-including-ai").collect();
    let categories = read(&d["package_categories"], "id").into_iter().filter(|f| f.slug != "ai-generated").collect();
    Ok((sections, categories))
}

/// One page of packages. ordering: most-downloaded | last-updated | newest | top-rated
pub async fn list(community: &str, q: &str, page: u32, ordering: &str) -> Result<(Vec<Listing>, u64, bool)> {
    list_filtered(community, q, page, ordering, None, &[]).await
}

pub async fn list_filtered(community: &str, q: &str, page: u32, ordering: &str, section: Option<&str>, categories: &[String]) -> Result<(Vec<Listing>, u64, bool)> {
    let mut u = url::Url::parse(&format!("{BASE}/api/cyberstorm/listing/{community}/"))?;
    u.query_pairs_mut().append_pair("ordering", ordering).append_pair("page", &page.to_string());
    if !q.is_empty() {
        u.query_pairs_mut().append_pair("q", q);
    }
    if let Some(sec) = section.filter(|s| !s.is_empty()) {
        u.query_pairs_mut().append_pair("section", sec);
    }
    for c in categories {
        u.query_pairs_mut().append_pair("included_categories", c);
    }
    let data = get_json(u.as_str()).await?;
    let results = data["results"].as_array().cloned().unwrap_or_default();
    let items = results
        .iter()
        .filter(|p| !p["is_nsfw"].as_bool().unwrap_or(false))
        .map(|p| {
            let ns = s(p, "namespace");
            let name = s(p, "name");
            Listing {
                key: format!("ts:{ns}-{name}"),
                title: pretty(&name),
                author: ns,
                description: s(p, "description"),
                icon: p["icon_url"].as_str().map(String::from),
                downloads: p["download_count"].as_u64().unwrap_or(0),
                rating: p["rating_count"].as_u64().unwrap_or(0),
                updated: s(p, "last_updated"),
                categories: p["categories"].as_array().map(|a| a.iter().map(|c| s(c, "name")).collect()).unwrap_or_default(),
                pinned: p["is_pinned"].as_bool().unwrap_or(false),
                deprecated: p["is_deprecated"].as_bool().unwrap_or(false),
            }
        })
        .collect();
    Ok((items, data["count"].as_u64().unwrap_or(0), !data["next"].is_null()))
}

/// Latest version of a package. `full` is "Namespace-Name".
pub async fn detail(full: &str) -> Result<Detail> {
    let (ns, name) = full.split_once('-').ok_or_else(|| anyhow!("bad package name {full}"))?;
    let d = get_json(&format!("{BASE}/api/experimental/package/{ns}/{name}/")).await?;
    let l = &d["latest"];
    let deps = l["dependencies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|dep| dep.rsplit_once('-').map(|(n, _)| n.to_string()).unwrap_or_else(|| dep.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(Detail {
        key: format!("ts:{full}"),
        title: pretty(name),
        author: ns.to_string(),
        version: s(l, "version_number"),
        description: s(l, "description"),
        icon: l["icon"].as_str().map(String::from),
        download: s(l, "download_url"),
        website: l["website_url"].as_str().filter(|w| !w.is_empty()).map(String::from),
        dependencies: deps,
    })
}

/// The loader package ("Namespace-Name") that a community's mods depend on.
/// Returns ("Namespace-Name", is_melonloader). Tries the expected loader first, then the other one,
/// so games we don't know in advance are worked out from what their community actually uses.
pub async fn find_base(community_id: &str, melon_hint: bool) -> Result<Option<(String, bool)>> {
    let order = if melon_hint { ["MelonLoader", "BepInExPack"] } else { ["BepInExPack", "MelonLoader"] };
    for q in order {
        let (items, _, _) = list(community_id, q, 1, "most-downloaded").await?;
        let starts = |l: &&Listing| l.title.replace(' ', "_").starts_with(q) || l.title.starts_with(q);
        if let Some(l) = items.iter().find(|l| starts(l) && l.pinned).or_else(|| items.iter().find(starts)) {
            return Ok(Some((l.key.trim_start_matches("ts:").to_string(), q == "MelonLoader")));
        }
    }
    Ok(None)
}

/// True when a Thunderstore package is a mod loader pack (goes in the game root).
pub fn is_loader(full: &str) -> bool {
    let name = full.split_once('-').map(|(_, n)| n).unwrap_or(full);
    name.starts_with("BepInExPack") || name == "MelonLoader"
}
