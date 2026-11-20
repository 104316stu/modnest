//! ModNest updating itself from GitHub releases: the same latest.json and minisign key the Tauri app uses.
use crate::app::ModNest;
use crate::bridge;
use crate::widgets::*;
use gpui::*;
use serde_json::Value;

const LATEST: &str = "https://github.com/104316stu/modnest/releases/latest/download/latest.json";
/// tauri.conf.json → plugins.updater.pubkey (base64 of the minisign public key file).
const PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQ5NEIyRkQyMzU0MEJERUQKUldUdHZVQTEwaTlMMlROU1hMSnFjc2JXTUJsM3B2RDRwVkcyR2pmUHFITnpZbjBIT3NMQlV4MjUK";

fn newer(current: &str, latest: &str) -> bool {
    let parse = |v: &str| v.trim_start_matches('v').split(['.', '-']).take(3).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parse(latest) > parse(current)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent("ModNest (mod manager)").build().expect("http client")
}

async fn check(current: String) -> Result<Option<Value>, String> {
    let v: Value = client().get(LATEST).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    Ok(newer(&current, &s(&v, "version")).then_some(v))
}

fn b64(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| e.to_string())
}

/// Downloads the installer, checks its signature, starts it and returns (the caller quits).
async fn install(u: Value, progress: impl Fn(f64) + Send + 'static) -> Result<std::path::PathBuf, String> {
    let p = &u["platforms"]["windows-x86_64"];
    let (url, sig) = (s(p, "url"), s(p, "signature"));
    if url.is_empty() {
        return Err("This update has no Windows download.".into());
    }
    let mut res = client().get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    let total = res.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if total > 0 {
            progress(bytes.len() as f64 / total as f64);
        }
    }
    let key_text = String::from_utf8(b64(PUBKEY_B64)?).map_err(|e| e.to_string())?;
    let key = minisign_verify::PublicKey::decode(&key_text).map_err(|e| e.to_string())?;
    let sig_text = String::from_utf8(b64(&sig)?).map_err(|e| e.to_string())?;
    let signature = minisign_verify::Signature::decode(&sig_text).map_err(|e| e.to_string())?;
    key.verify(&bytes, &signature, true).map_err(|_| "The update's signature didn't match, so it wasn't installed.".to_string())?;
    let name = url.rsplit('/').next().unwrap_or("ModNest-setup.exe").to_string();
    let path = std::env::temp_dir().join(format!("modnest-update-{}", name));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

impl ModNest {
    pub fn check_app_update(&mut self, cx: &mut Context<Self>) {
        let current = self.version.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(Some(u)) = bridge::run(check(current)).await {
                let _ = this.update(cx, |this, cx| {
                    this.app_update = Some(u);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn install_app_update(&mut self, cx: &mut Context<Self>) {
        let Some(u) = self.app_update.clone() else { return };
        if self.updating {
            return;
        }
        self.updating = true;
        cx.notify();
        let (tx, rx) = async_channel::unbounded::<f64>();
        cx.spawn(async move |this, cx| {
            while let Ok(f) = rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.toast("selfupd", "Downloading ModNest update", "", "", Some(f), 0);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let r = bridge::run(install(u, move |f| {
                let _ = tx.try_send(f);
            }))
            .await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(path) => {
                    // Same arguments as the Tauri updater: passive, update mode, start ModNest again when done.
                    match std::process::Command::new(&path).args(["/P", "/UPDATE", "/R"]).spawn() {
                        Ok(_) => cx.quit(),
                        Err(e) => {
                            this.updating = false;
                            this.dismiss("selfupd");
                            this.toast_error(&e.to_string());
                        }
                    }
                }
                Err(e) => {
                    this.updating = false;
                    this.dismiss("selfupd");
                    this.toast_error(&e);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
