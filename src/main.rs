// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

// The webview host: dioxus-desktop under either name (see the `desktop` and `mobile` features).
#[cfg(feature = "desktop")]
pub use dioxus::desktop as host;
#[cfg(feature = "mobile")]
pub use dioxus::mobile as host;

use host::{wry::http::Response, Config};
use std::borrow::Cow;

const MAIN_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/main.css"));
const BOOT_JS: &str = include_str!("../assets/boot.js");
const ENGINE_JS: &str = include_str!("../assets/engine.js");
const PREVIEW_JS: &str = include_str!("../assets/preview.js");

fn main() {
    // WebKitGTK's DMA-BUF renderer path has a long-standing bug on many
    // Linux GPU/driver combos where hardware-accelerated video stutters.
    // Must be set before WebKit initializes.
    #[cfg(target_os = "linux")]
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    app_lib::init_backend();

    let head = format!(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\"><style>{MAIN_CSS}</style><script>{BOOT_JS}</script><script>{ENGINE_JS}</script><script>{PREVIEW_JS}</script>"
    );

    let mut cfg = Config::new().with_custom_head(head).with_asynchronous_custom_protocol("tq", |_id, request, responder| {
        let uri = request.uri().to_string();
        app_lib::runtime().spawn(async move {
            responder.respond(handle_protocol(&uri).await);
        });
    });

    #[cfg(feature = "desktop")]
    {
        use host::{LogicalSize, WindowBuilder};
        let window = WindowBuilder::new()
            .with_title("Tanqi")
            // Transparent so the mini player can float without a backdrop; the full app paints its own.
            .with_transparent(true)
            .with_inner_size(LogicalSize::new(1200.0, 800.0))
            // Small enough to check the phone layout (< 960px wide) in a desktop window.
            .with_min_inner_size(LogicalSize::new(360.0, 560.0));
        cfg = cfg.with_window(window).with_menu(None);

        // The window / taskbar icon. An icon that fails to decode is not worth
        // stopping for: the window just keeps the default one.
        match host::icon_from_memory(include_bytes!("../icons/128x128@2x.png")) {
            Ok(icon) => cfg = cfg.with_icon(icon),
            Err(e) => log::warn!("couldn't load the window icon: {e}"),
        }
    }

    #[cfg(feature = "desktop")]
    let launcher = dioxus::LaunchBuilder::desktop();
    #[cfg(feature = "mobile")]
    let launcher = dioxus::LaunchBuilder::mobile();
    launcher.with_cfg(cfg).launch(ui::App);
}

fn respond(status: u16, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(body))
        .unwrap()
}

fn query_param(uri: &str, key: &str) -> Option<String> {
    let q = uri.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| urlencoding::decode(v).map(|c| c.into_owned()).ok()).flatten()
    })
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|e| e.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("m4a") | Some("aac") => "audio/mp4",
        _ => "application/octet-stream",
    }
}

/// `tq://localhost/file?path=..` streams local audio and cover art.
async fn handle_protocol(uri: &str) -> Response<Cow<'static, [u8]>> {
    let path = uri.split_once("//").map(|(_, r)| r).and_then(|r| r.find('/').map(|i| &r[i..])).unwrap_or("/");
    let route = path.split('?').next().unwrap_or("/");
    match route {
        "/file" => {
            let Some(file) = query_param(uri, "path") else {
                return respond(400, "text/plain", b"missing path".to_vec());
            };
            let mime = mime_for(&file);
            if mime == "application/octet-stream" {
                return respond(403, "text/plain", b"not a servable file type".to_vec());
            }
            match tokio::fs::read(&file).await {
                Ok(bytes) => respond(200, mime, bytes),
                Err(e) => respond(404, "text/plain", e.to_string().into_bytes()),
            }
        }
        _ => respond(404, "text/plain", b"not found".to_vec()),
    }
}
