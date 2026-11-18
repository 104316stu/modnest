//! Profile sharing and importing.
//! - ModNest codes: the mod list packed into the code itself (works offline, no server).
//! - Thunderstore codes: the same short codes r2modman / Thunderstore Mod Manager use.
//! - Thunderstore Mod Manager: reads the profiles it saved on this PC.
use crate::store::ProfileMod;
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;
const STD: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
const PREFIX: &str = "MODNEST1-";

#[derive(Serialize, Deserialize)]
struct Packed {
    g: String,
    n: String,
    m: Vec<(String, String, bool)>,
}

pub fn encode(game_id: &str, name: &str, mods: &[ProfileMod]) -> String {
    let p = Packed {
        g: game_id.into(),
        n: name.into(),
        m: mods.iter().filter(|m| !m.key.starts_with("local:")).map(|m| (m.key.clone(), m.version.clone(), m.enabled)).collect(),
    };
    format!("{PREFIX}{}", B64.encode(serde_json::to_vec(&p).unwrap_or_default()))
}

/// (game id, profile name, mods)
pub fn decode(code: &str) -> Option<(String, String, Vec<ProfileMod>)> {
    let body = code.trim().strip_prefix(PREFIX)?;
    let p: Packed = serde_json::from_slice(&B64.decode(body.trim()).ok()?).ok()?;
    let mods = p.m.into_iter().map(|(key, version, enabled)| ProfileMod { title: title_of(&key), key, version, enabled }).collect();
    Some((p.g, p.n, mods))
}

fn title_of(key: &str) -> String {
    let k = key.split_once(':').map(|x| x.1).unwrap_or(key);
    k.split_once('-').map(|x| x.1).unwrap_or(k).replace('_', " ")
}

/// Pulls (name, enabled, version) out of r2modman's YAML (mods.yml / export.r2x).
/// Only the handful of fields we need, so no YAML library.
pub fn parse_r2_yaml(text: &str) -> Vec<ProfileMod> {
    let lines: Vec<&str> = text.lines().collect();
    let item_indent = lines.iter().find_map(|l| {
        let t = l.trim_start();
        (t.starts_with("- ") && (t.contains("name:") || t.contains("manifestVersion:"))).then(|| l.len() - t.len())
    });
    let Some(indent) = item_indent else { return vec![] };
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for l in lines {
        let t = l.trim_start();
        if t.starts_with("- ") && l.len() - t.len() == indent {
            blocks.push(vec![&t[2..]]);
        } else if let Some(b) = blocks.last_mut() {
            b.push(t);
        }
    }
    let field = |b: &[&str], k: &str| -> Option<String> {
        b.iter().find_map(|l| l.strip_prefix(&format!("{k}:")).map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string()))
    };
    blocks
        .into_iter()
        .filter_map(|b| {
            let name = field(&b, "name")?;
            if !name.contains('-') {
                return None;
            }
            let v = |k| field(&b, k).unwrap_or_else(|| "0".into());
            Some(ProfileMod {
                title: title_of(&name),
                key: format!("ts:{name}"),
                version: format!("{}.{}.{}", v("major"), v("minor"), v("patch")),
                enabled: field(&b, "enabled").map(|e| e != "false").unwrap_or(true),
            })
        })
        .collect()
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().user_agent("ModNest/0.1 (mod manager)").timeout(std::time::Duration::from_secs(20)).build()?)
}

/// Downloads a Thunderstore / r2modman profile code. Returns (profile name, mods).
pub async fn fetch_thunderstore_code(code: &str) -> Result<(String, Vec<ProfileMod>)> {
    let code = code.trim();
    if code.len() > 64 || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("That doesn't look like a profile code.");
    }
    let res = client()?.get(format!("https://thunderstore.io/api/experimental/legacyprofile/get/{code}/")).send().await?;
    if !res.status().is_success() {
        bail!("Thunderstore doesn't know that code (it may have expired).");
    }
    let text = res.text().await?;
    let b64 = text.trim().strip_prefix("#r2modman").ok_or_else(|| anyhow!("Unknown profile format"))?.trim();
    let bytes = STD.decode(b64.replace(['\n', '\r'], ""))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut yaml = String::new();
    zip.by_name("export.r2x")?.read_to_string(&mut yaml)?;
    let name = yaml.lines().find_map(|l| l.strip_prefix("profileName:")).map(|s| s.trim().trim_matches('\'').to_string()).unwrap_or_else(|| "Imported".into());
    let mods = parse_r2_yaml(&yaml);
    if mods.is_empty() {
        bail!("That profile has no mods in it.");
    }
    Ok((name, mods))
}

/// Uploads a profile to Thunderstore's profile-code service (the one r2modman uses). Returns the code.
pub async fn create_thunderstore_code(name: &str, mods: &[ProfileMod]) -> Result<String> {
    let mut yaml = format!("profileName: {}\nmods:\n", name.replace(['\n', ':'], " "));
    for m in mods.iter().filter(|m| m.key.starts_with("ts:")) {
        let full = &m.key[3..];
        let mut v = m.version.split('.').map(|x| x.parse::<u64>().unwrap_or(0));
        let (a, b, c) = (v.next().unwrap_or(0), v.next().unwrap_or(0), v.next().unwrap_or(0));
        yaml.push_str(&format!("  - name: {full}\n    version:\n      major: {a}\n      minor: {b}\n      patch: {c}\n    enabled: {}\n", m.enabled));
    }
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        z.start_file("export.r2x", zip::write::SimpleFileOptions::default())?;
        z.write_all(yaml.as_bytes())?;
        z.finish()?;
    }
    let body = format!("#r2modman\n{}", STD.encode(buf.into_inner()));
    let res = client()?
        .post("https://thunderstore.io/api/experimental/legacyprofile/create/")
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .await?;
    if !res.status().is_success() {
        bail!("Thunderstore refused the profile ({})", res.status());
    }
    let v: serde_json::Value = res.json().await?;
    v["key"].as_str().map(String::from).ok_or_else(|| anyhow!("Thunderstore didn't return a code"))
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Profiles saved by Thunderstore Mod Manager / r2modman on this PC: (community id, profile name, mods).
pub fn local_manager_profiles() -> Vec<(String, String, Vec<ProfileMod>)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        roots.push(appdata.join("Thunderstore Mod Manager").join("DataFolder"));
        roots.push(appdata.join("r2modmanPlus-local"));
    }
    let mut out = Vec::new();
    for root in roots {
        let Ok(games) = std::fs::read_dir(&root) else { continue };
        for g in games.flatten() {
            let folder = g.file_name().to_string_lossy().to_string();
            let Some(c) = crate::thunderstore::COMMUNITIES.iter().find(|c| squash(c.id) == squash(&folder) || squash(c.name) == squash(&folder)) else { continue };
            let Ok(profiles) = std::fs::read_dir(g.path().join("profiles")) else { continue };
            for p in profiles.flatten() {
                let Ok(text) = std::fs::read_to_string(p.path().join("mods.yml")) else { continue };
                let mods = parse_r2_yaml(&text);
                if !mods.is_empty() {
                    out.push((c.id.to_string(), p.file_name().to_string_lossy().to_string(), mods));
                }
            }
        }
    }
    out
}
