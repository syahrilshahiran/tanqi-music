//! Process-wide handles, set once at startup. The backend and UI share one
//! process, so these are plain statics.

use crate::db::Db;
use crate::http::ReqwestHttp;
use crate::library_store::LibraryStore;
use crate::lyrics_analyzer::LyricsAnalyzer;
use crate::metadata::Metadata;
use crate::mpris::{MprisCommand, MprisHandle};
use std::sync::OnceLock;
use tokio::sync::broadcast;

static DB: OnceLock<Db> = OnceLock::new();
static STORE: OnceLock<LibraryStore> = OnceLock::new();
static METADATA: OnceLock<Metadata<ReqwestHttp>> = OnceLock::new();
static ANALYZER: OnceLock<LyricsAnalyzer<ReqwestHttp>> = OnceLock::new();
static MPRIS: OnceLock<MprisHandle> = OnceLock::new();
static MPRIS_EVENTS: OnceLock<broadcast::Sender<MprisCommand>> = OnceLock::new();

/// The database, for the commands that read settings and analysis caches.
/// Library logic takes a `LibraryStore` instead of reaching for this.
pub fn db() -> &'static Db {
    DB.get().expect("database not initialised")
}

/// The Library store, built from the same connection.
pub fn store() -> &'static LibraryStore {
    STORE.get().expect("database not initialised")
}

/// Online metadata lookups, built on first use.
pub fn metadata() -> &'static Metadata<ReqwestHttp> {
    METADATA.get_or_init(|| match DB.get() {
        Some(db) => Metadata::new(ReqwestHttp).with_cache(db.clone()),
        None => Metadata::new(ReqwestHttp),
    })
}

/// Lyrics analysis and translation through the local Ollama model.
pub fn lyrics_analyzer() -> &'static LyricsAnalyzer<ReqwestHttp> {
    ANALYZER.get_or_init(|| LyricsAnalyzer::new(db().clone(), ReqwestHttp))
}

pub fn init_db(db: Db) {
    let _ = STORE.set(LibraryStore::new(db.clone()));
    let _ = DB.set(db);
}

pub fn init_mpris(handle: MprisHandle) {
    let _ = MPRIS.set(handle);
}

pub fn mpris() -> Option<&'static MprisHandle> {
    MPRIS.get()
}

/// Where the MPRIS thread publishes D-Bus control requests; UI subscribes.
pub fn mpris_events() -> &'static broadcast::Sender<MprisCommand> {
    MPRIS_EVENTS.get_or_init(|| broadcast::channel(64).0)
}

/// Desktop notification via the system's notification service. Shown from a
/// plain thread: notify-rust's D-Bus client blocks on its own executor, which
/// panics ("cannot start a runtime from within a runtime") when called from
/// a tokio worker. Failures are logged, not returned.
/// Targets of notifications that were clicked (desktop): the UI subscribes, see `ui::targets`.
static NOTIFICATION_CLICKS: OnceLock<broadcast::Sender<String>> = OnceLock::new();

pub fn notification_clicks() -> &'static broadcast::Sender<String> {
    NOTIFICATION_CLICKS.get_or_init(|| broadcast::channel(16).0)
}

#[cfg(target_os = "android")]
pub fn notify(_title: &str, _body: &str, _target: Option<String>) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn notify(title: &str, body: &str, target: Option<String>) -> Result<(), String> {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        // On Linux, the desktop (Cinnamon at least) accepts a notification sent by this process, plays
        // its sound, and shows no pop-up - it treats a program with its own window differently. The
        // same message sent by `notify-send` pops up, so use that when it is installed. With a target it
        // also offers a default action: `notify-send` waits, and prints `default` if the pop-up is clicked.
        #[cfg(target_os = "linux")]
        {
            let mut cmd = std::process::Command::new("notify-send");
            cmd.args(["--app-name=Tanqi", "--icon=audio-x-generic"]);
            if target.is_some() {
                cmd.arg("--action=default=Open");
            }
            cmd.args(["--", &title, &body]);
            if let Ok(out) = cmd.output() {
                if out.status.success() {
                    if String::from_utf8_lossy(&out.stdout).trim() == "default" {
                        if let Some(t) = target {
                            let _ = notification_clicks().send(t);
                        }
                    }
                    return;
                }
            }
        }
        if let Err(e) = notify_rust::Notification::new().summary(&title).body(&body).show() {
            log::warn!("notification failed to show: {e}");
        }
    });
    Ok(())
}

/// App data dir (`~/.local/share/<identifier>` on Linux). Deliberately not
/// the original Tanqi's `com.tanqi.app`, so the two apps never share a database.
#[cfg(target_os = "android")]
pub fn app_data_dir() -> std::path::PathBuf {
    // `dirs` has no Android data dir. The app's private files dir is
    // /data/data/<package>/files, and the package name is the process name.
    let cmdline = std::fs::read("/proc/self/cmdline").unwrap_or_default();
    let package = String::from_utf8_lossy(cmdline.split(|b| *b == 0).next().unwrap_or_default()).into_owned();
    let package = if package.is_empty() { "com.tanqi.music".to_string() } else { package };
    std::path::PathBuf::from("/data/data").join(package).join("files")
}

#[cfg(not(target_os = "android"))]
pub fn app_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .expect("could not resolve data dir")
        .join("com.tanqi.music")
}
