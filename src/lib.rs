pub mod app;
pub mod audio;
pub mod commands;
pub mod db;
pub mod ai;
pub mod http;
mod itunes;
pub mod library;
pub mod library_store;
pub mod local_ref;
pub mod lyrics_analyzer;
mod lrclib;
pub mod metadata;
pub mod models;
pub mod mpris;
mod musicbrainz;
pub mod lyrics_ai;
pub mod secret;
pub mod path_tags;
pub mod scanner;
mod ytmusic;

use library_store::LibraryStore;
use std::future::Future;
use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start tokio runtime")
    })
}

/// Runs a backend future on the app's own tokio runtime and awaits it from
/// whatever executor the UI is on. reqwest and friends need
/// a tokio reactor, which Dioxus' local executor doesn't guarantee.
pub async fn run<T, F>(fut: F) -> T
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    runtime().spawn(fut).await.expect("backend task panicked")
}

/// Runs a blocking call against the Library store on the app's blocking
/// pool and awaits it from whatever executor the UI is on. The store's
/// methods are synchronous, and a Scan can take minutes.
pub async fn with_store<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&LibraryStore) -> Result<T, db::StoreError> + Send + 'static,
{
    runtime()
        .spawn_blocking(move || f(app::store()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(String::from)
}

/// Runs a metadata lookup on the app's tokio runtime (reqwest needs its
/// reactor) and awaits it from whatever executor the UI is on.
pub async fn with_metadata<T, Fut>(f: impl FnOnce(&'static metadata::Metadata<http::ReqwestHttp>) -> Fut + Send + 'static) -> T
where
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    run(async move { f(app::metadata()).await }).await
}

/// Runs a Lyrics analyzer call on the app's tokio runtime and awaits it from
/// whatever executor the UI is on.
pub async fn with_analyzer<T, Fut>(f: impl FnOnce(&'static lyrics_analyzer::LyricsAnalyzer<http::ReqwestHttp>) -> Fut + Send + 'static) -> T
where
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    run(async move { f(app::lyrics_analyzer()).await }).await
}

/// Opens the database, starts MPRIS and kicks off a library rescan.
/// Call once, before the UI launches.
pub fn init_backend() {
    let dir = app::app_data_dir();
    let conn = db::init_db(&dir).expect("failed to initialize database");
    // Reads get their own connection so a Scan's writes don't stall them; fall back to the one connection.
    app::init_db(match db::open_reader(&dir) {
        Ok(reader) => db::Db::with_reader(conn, reader),
        Err(e) => {
            eprintln!("read-only database connection unavailable ({e}); sharing one connection");
            db::Db::new(conn)
        }
    });
    // Lookup answers past their time are dropped at startup.
    let _ = app::db().with(|c| db::cache_prune(c));

    // Exposes playback over D-Bus so media keys, `playerctl`, and
    // the desktop's now-playing widget all work. Degrades to a
    // no-op (with a warning) where there's no session bus.
    let _guard = runtime().enter();
    app::init_mpris(mpris::start());

    // Pick up files added, changed or removed since the last run. Runs in
    // the background so the window isn't held up by a big library. On Android
    // the UI does this once it knows the media permission is granted (a Scan
    // without it would drop every Track; see `scanner::set_storage_readable`).
    #[cfg(not(target_os = "android"))]
    runtime().spawn(async {
        if let Err(e) = with_store(|s| s.scan_all(&scanner::FsScanner)).await {
            log::warn!("startup library rescan failed: {e}");
        }
    });
}
