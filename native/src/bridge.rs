//! Connects the GPUI interface to the async engine (modnest-core), which runs on its own tokio runtime.
use futures::future::BoxFuture;
use gpui_http_client::{http, AsyncBody, HttpClient, Url};
use modnest_core as mn;
use std::future::Future;
use std::sync::{Arc, OnceLock};

/// The engine's background runtime (downloads, Thunderstore/CurseForge requests, file work).
pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("modnest-core")
            .build()
            .expect("tokio runtime")
    })
}

/// A progress event from the engine.
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub payload: serde_json::Value,
}

/// The engine plus the channel its progress events arrive on.
pub struct Engine {
    pub core: Arc<mn::Core>,
    pub events: async_channel::Receiver<Event>,
}

impl Engine {
    pub fn start(data_dir: std::path::PathBuf) -> Engine {
        let (tx, rx) = async_channel::unbounded();
        let emitter: mn::Emitter = Arc::new(move |name: &str, payload: serde_json::Value| {
            let _ = tx.try_send(Event { name: name.to_string(), payload });
        });
        Engine { core: Arc::new(mn::Core::new(data_dir, emitter)), events: rx }
    }
}

/// Runs an engine call on the background runtime; the returned future can be awaited from GPUI.
pub fn run<F, T>(f: F) -> impl Future<Output = T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = rt().spawn(f);
    async move { handle.await.expect("engine task panicked") }
}

/// Where ModNest keeps its settings and saved state (same place as the Tauri app, so both share it).
pub fn data_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("MODNEST_DATA_DIR") {
        return d.into();
    }
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("app.modnest.desktop")
}

/// GPUI loads web images (mod icons, covers) through this, using reqwest on the engine runtime.
pub struct Http {
    client: reqwest::Client,
    agent: http::HeaderValue,
}

impl Http {
    pub fn new() -> Arc<Http> {
        Arc::new(Http {
            client: reqwest::Client::builder().user_agent("ModNest (mod manager)").build().expect("http client"),
            agent: http::HeaderValue::from_static("ModNest (mod manager)"),
        })
    }
}

impl HttpClient for Http {
    fn type_name(&self) -> &'static str {
        "ModNestHttp"
    }
    fn user_agent(&self) -> Option<&http::HeaderValue> {
        Some(&self.agent)
    }
    fn proxy(&self) -> Option<&Url> {
        None
    }
    fn send(&self, req: http::Request<AsyncBody>) -> BoxFuture<'static, anyhow::Result<http::Response<AsyncBody>>> {
        let client = self.client.clone();
        let url = req.uri().to_string();
        let method = req.method().clone();
        Box::pin(run(async move {
            let res = client.request(reqwest::Method::from_bytes(method.as_str().as_bytes())?, &url).send().await?;
            let status = res.status().as_u16();
            let bytes = res.bytes().await?;
            Ok(http::Response::builder().status(status).body(AsyncBody::from_bytes(bytes))?)
        }))
    }
}
