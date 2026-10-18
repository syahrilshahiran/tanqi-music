//! Local music library, browsed as a discography: Artists -> Albums ->
//! tracks, built from the tags of the files in the watched folders.

use app_lib::db::StoreError;
use app_lib::library::{group_key, Album, Artist, Library as LibraryData};
use app_lib::library_store::ScanReport;
use app_lib::scanner::FsScanner;
use app_lib::with_store;
use app_lib::models::{Track, TrackSummary};
use dioxus::prelude::*;
use std::sync::Arc;

use crate::ui::api;
use crate::ui::components::{
    confirm_modal::ConfirmModal,
    filter_pill::FilterPill,
    folders_modal::FoldersModal,
    add_to_playlist::AddToPlaylist,
    icons::{BackIcon, FolderIcon, GearIcon, HeartIcon, ImageIcon, LyricsIcon, PulseIcon, RefreshIcon, UserIcon, InfoIcon, PauseIcon, PlayIcon, QueueIcon},
    song_info_modal::SongInfoModal,
    track_actions_sheet::TrackActionsSheet,
    track_thumbnail::{ThumbKind, TrackThumbnail},
};
use crate::ui::favorites::use_favorites;
use crate::ui::jobs::{bulk_audio_analysis_job, bulk_lyrics_analysis_job, fetch_artist_pictures_job, fetch_covers_job, use_jobs, JobKind};
use crate::ui::pages::playlists::{PlaylistList, PlaylistPage};
use crate::ui::player::use_player;
use crate::ui::util::format_duration;

/// How many rows the flat Songs tab renders at first; "Show more" adds this
/// many again. A library can hold tens of thousands of tracks.
const SONGS_PAGE: usize = 50;

/// The two whole-Library analyses that ask first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BulkJob {
    Audio,
    Lyrics,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Artists,
    Albums,
    Tracks,
    Favorites,
    Playlists,
}

/// A drill-down page. The stack of these is the back-navigation history.
#[derive(Clone, PartialEq)]
enum Page {
    Artist(String),
    /// Album id (see `Album::id`).
    Album(String),
    /// Playlist id.
    Playlist(i64),
}

fn to_summary(t: &Track) -> TrackSummary {
    TrackSummary {
        id: t.id.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: t.duration_ms,
        thumbnail_url: t.thumbnail_url.clone(),
    }
}

fn summaries(tracks: &[Track]) -> Vec<TrackSummary> {
    tracks.iter().map(to_summary).collect()
}

/// The Play queue a list hands to every one of its rows. Rows hold a handle, not a copy: it is cloned and
/// compared by pointer, and only turned into a Vec when a row is actually played.
#[derive(Clone)]
struct SharedQueue(Arc<Vec<TrackSummary>>);

impl SharedQueue {
    fn of(tracks: &[Track]) -> Self {
        SharedQueue(Arc::new(summaries(tracks)))
    }

    fn to_vec(&self) -> Vec<TrackSummary> {
        (*self.0).clone()
    }

    fn first(&self) -> Option<&TrackSummary> {
        self.0.first()
    }
}

impl PartialEq for SharedQueue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The Tracks / Favorites list as filtered by the search box, built once per change of tab, query, library or
/// hearts rather than on every render.
#[derive(Clone)]
struct TrackList {
    tracks: Arc<Vec<Track>>,
    queue: SharedQueue,
}

impl PartialEq for TrackList {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.tracks, &other.tracks)
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn album_meta(album: &Album) -> String {
    let mut bits = vec![];
    if let Some(y) = album.year {
        bits.push(y.to_string());
    }
    bits.push(plural(album.tracks.len(), "track"));
    let ms = album.duration_ms();
    if ms > 0 {
        bits.push(format_duration(ms));
    }
    bits.join(" · ")
}

fn matches(query: &str, haystack: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || matches_needle(&q, haystack)
}

/// `needle` is already trimmed and lowercased (so a long list lowercases the query once, not per track).
fn matches_needle(needle: &str, haystack: &[&str]) -> bool {
    haystack.iter().any(|h| h.to_lowercase().contains(needle))
}

#[component]
pub fn Library() -> Element {
    let jobs = use_jobs();
    let features = crate::ui::settings::use_features();
    let ai_remote = crate::ui::settings::use_settings().read().ai_provider.needs_key();
    let mut library = use_signal(LibraryData::default);
    let mut folders = use_signal(Vec::<app_lib::library_store::FolderInfo>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut status = use_signal(|| None::<String>);
    let mut scanning = use_signal(|| false);
    let mut show_folders = use_signal(|| false);
    // The empty Library's button opens the Folders popup already browsing (Android has no native picker).
    let mut add_on_open = use_signal(|| false);
    // Bulk analysis runs over the whole Library, so it is confirmed first.
    let mut confirm = use_signal(|| None::<BulkJob>);
    // Phone: the tool buttons live in a bottom drawer opened from the gear next to the title.
    let mut tools_open = use_signal(|| false);
    let mut tab = use_signal(|| Tab::Artists);
    let needs_permission = use_context::<crate::ui::NeedsStoragePermission>().0;
    let favorites = use_favorites();
    // The wordmark was clicked: back to the top level of the Library.
    let home = use_context::<crate::ui::HomeTick>().0;
    let mut stack = use_signal(Vec::<Page>::new);
    let mut query = use_signal(String::new);
    let mut songs_shown = use_signal(|| SONGS_PAGE);
    let mut info_track = use_signal(|| None::<TrackSummary>);
    // Artist photos by `group_key` of the name; an artist without one shows an album cover.
    let mut artist_pictures = use_signal(std::collections::HashMap::<String, String>::new);

    use_effect(move || {
        let _ = home();
        stack.write().clear();
        query.set(String::new());
    });

    // Scroll with the drill-down: a new artist/album page opens at its top,
    // and Back returns to where the list was (see `tqLibNav` in boot.js).
    use_hook(|| document::eval("tqLibForget()"));
    use_effect(move || {
        let depth = stack.read().len();
        document::eval(&format!("tqLibNav({depth})"));
    });

    let load = move || {
        spawn(async move {
            if let Ok(pics) = api::list_artist_pictures().await {
                artist_pictures.set(pics);
            }
            match with_store(|s| Ok::<_, StoreError>((s.library()?, s.folder_stats()?))).await {
                Ok((l, f)) => {
                    library.set(l);
                    folders.set(f);
                    error.set(None);
                }
                Err(e) => error.set(Some(e)),
            }
            loading.set(false);
        });
    };
    use_hook(load);
    // Also reload when the library may have changed behind the page's back: the
    // wordmark was tapped, onboarding added the first folder, or (Android) the
    // storage permission arrived and a Scan ran.
    use_effect(move || {
        if home() > 0 {
            load();
        }
    });

    // The player's artist name was clicked: open that artist's page.
    let open_target = crate::ui::targets::use_open_target();
    use_effect(move || {
        if let Some(name) = open_target.track_for("artist") {
            stack.set(vec![Page::Artist(name)]);
            query.set(String::new());
            open_target.clear();
        }
    });

    // Runs a scan-like operation with the shared busy flag and a summary line.
    let mut run_scan = move |op: std::pin::Pin<Box<dyn std::future::Future<Output = Result<ScanReport, String>>>>| {
        scanning.set(true);
        status.set(None);
        spawn(async move {
            match op.await {
                Ok(report) => {
                    let s = report.summary;
                    let mut parts = vec![format!("{} added", s.added)];
                    if s.updated > 0 {
                        parts.push(format!("{} updated", s.updated));
                    }
                    if s.removed > 0 {
                        parts.push(format!("{} removed", s.removed));
                    }
                    if !report.failed.is_empty() {
                        parts.push(format!("{} unavailable", plural(report.failed.len(), "folder")));
                    }
                    status.set(Some(format!("Scanned {}: {}", plural(s.total, "file"), parts.join(", "))));
                    load();
                }
                Err(e) => error.set(Some(e)),
            }
            scanning.set(false);
        });
    };

    // Desktop: the native folder dialog, then a Scan of what was picked.
    let add_native = move || {
        spawn(async move {
            match api::pick_local_audio_folder().await {
                Ok(None) => {} // cancelled
                Ok(Some(folder)) => run_scan(Box::pin(async move {
                    with_store(move |s| s.add_folder(&folder, &FsScanner)).await.map(ScanReport::from)
                })),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    // Android: the Folders popup browsed to a path itself.
    let mut add_path = move |folder: String| {
        run_scan(Box::pin(async move {
            with_store(move |s| s.add_folder(&folder, &FsScanner)).await.map(ScanReport::from)
        }));
    };
    let mut rescan = move || run_scan(Box::pin(with_store(|s| s.scan_all(&FsScanner))));
    let remove_folder = move |folder: String| {
        spawn(async move {
            match with_store(move |s| s.remove_folder(&folder)).await {
                Ok(()) => load(),
                Err(e) => error.set(Some(e)),
            }
        });
    };

    // Reload when a cover fetch finishes so the new art shows without a
    // manual rescan. A memo, so progress ticks don't retrigger the effect.
    let covers_running =
        use_memo(move || jobs.jobs.read().iter().any(|j| j.kind == JobKind::Covers && !j.status.is_finished()));
    let pictures_running =
        use_memo(move || jobs.jobs.read().iter().any(|j| j.kind == JobKind::ArtistPictures && !j.status.is_finished()));
    // Memos, so the page re-renders when a flag flips, not on every progress tick of a job.
    let audio_running =
        use_memo(move || jobs.jobs.read().iter().any(|j| j.kind == JobKind::Audio && !j.status.is_finished()));
    let lyrics_running =
        use_memo(move || jobs.jobs.read().iter().any(|j| j.kind == JobKind::Lyrics && !j.status.is_finished()));
    // The Tracks / Favorites list: filtered and turned into its Play queue once per change of what it depends on.
    let favorites_for_list = favorites;
    let track_list = use_memo(move || {
        let tab_now = tab();
        if !matches!(tab_now, Tab::Tracks | Tab::Favorites) {
            return TrackList { tracks: Arc::new(Vec::new()), queue: SharedQueue(Arc::new(Vec::new())) };
        }
        let only_favorites = tab_now == Tab::Favorites;
        let needle = query().trim().to_lowercase();
        let tracks: Vec<Track> = library
            .read()
            .albums
            .iter()
            .flat_map(|al| al.tracks.iter())
            .filter(|t| !only_favorites || favorites_for_list.contains(&t.id))
            .filter(|t| needle.is_empty() || matches_needle(&needle, &[&t.title, &t.artist, t.album.as_deref().unwrap_or("")]))
            .cloned()
            .collect();
        let queue = SharedQueue::of(&tracks);
        TrackList { tracks: Arc::new(tracks), queue }
    });
    let mut was_running = use_signal(|| false);
    use_effect(move || {
        let running = covers_running() || pictures_running();
        if *was_running.peek() && !running {
            load();
        }
        was_running.set(running);
    });

    if loading() {
        return rsx! { p { class: "text-muted-sm", "Loading…" } };
    }

    let local_count = library.read().track_count();
    let album_count = library.read().albums.len();
    let artist_count = library.read().artists.len();
    let audio_queued = audio_running();
    let covers_queued = covers_running();
    let pictures_queued = pictures_running();
    let lyrics_queued = lyrics_running();

    let header = rsx! {
        div { class: "library__header",
            div { class: "library__heading",
                div { class: "library__title-row",
                    h1 { class: "library__title", "Library" }
                    button {
                        class: "library__tools-toggle",
                        title: "Library tools",
                        "aria-label": "Library tools",
                        "aria-expanded": "{tools_open()}",
                        onclick: move |_| tools_open.set(true),
                        GearIcon { size: 22 }
                    }
                }
                if local_count > 0 {
                    p { class: "text-muted-xs",
                        "{plural(artist_count, \"artist\")} · {plural(album_count, \"album\")} · {plural(local_count, \"track\")}"
                    }
                }
            }
            if tools_open() {
                div { class: "library__drawer-scrim", onclick: move |_| tools_open.set(false) }
            }
            // Choosing a tool closes the drawer (clicks bubble up to here).
            div {
                class: if tools_open() { "library__tools library__tools--open" } else { "library__tools" },
                onclick: move |_| tools_open.set(false),
                button {
                    class: "library__tool-btn",
                    onclick: move |_| {
                        add_on_open.set(false);
                        show_folders.set(true);
                    },
                    FolderIcon { size: 18 }
                    span { class: "tool-label", if folders().is_empty() { "Folders" } else { "Folders ({folders().len()})" } }
                }
                if !folders().is_empty() {
                    button { class: "library__tool-btn", disabled: scanning(), onclick: move |_| rescan(), RefreshIcon { size: 18 }, span { class: "tool-label", if scanning() { "Scanning…" } else { "Rescan" } } }
                    button {
                        class: "library__tool-btn",
                        disabled: covers_queued,
                        title: "Look up cover art on iTunes for albums that have none",
                        onclick: move |_| {
                            spawn(async move {
                                match with_store(|s| s.albums_missing_cover()).await {
                                    Ok(albums) if albums.is_empty() => status.set(Some("Every album already has a cover".to_string())),
                                    Ok(albums) => {
                                        jobs.enqueue(fetch_covers_job(albums));
                                        // Covers land as the job runs; the page refreshes on the next Rescan or visit.
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                            });
                        },
                        ImageIcon { size: 18 }
                        span { class: "tool-label", if covers_queued { "Queued…" } else { "Fetch covers" } }
                    }
                    button {
                        class: "library__tool-btn",
                        disabled: pictures_queued,
                        title: "Look up each artist's picture on YouTube Music (exact name matches only)",
                        onclick: move |_| {
                            let have = artist_pictures.read().clone();
                            let missing: Vec<String> = library
                                .read()
                                .artists
                                .iter()
                                .filter(|a| !have.contains_key(&group_key(&a.name)))
                                .map(|a| a.name.clone())
                                .collect();
                            if missing.is_empty() {
                                status.set(Some("Every artist already has a picture".to_string()));
                            } else {
                                jobs.enqueue(fetch_artist_pictures_job(missing));
                            }
                        },
                        UserIcon { size: 18 }
                        span { class: "tool-label", if pictures_queued { "Queued…" } else { "Artist pictures" } }
                    }
                    if features.audio {
                    button {
                        class: "library__tool-btn",
                        disabled: audio_queued,
                        title: "Measure every track and suggest an EQ for it",
                        // Stay in the drawer: Cancel should land back in it.
                        onclick: move |e| {
                            e.stop_propagation();
                            confirm.set(Some(BulkJob::Audio));
                        },
                        PulseIcon { size: 18 }
                        span { class: "tool-label", if audio_queued { "Queued…" } else { "Analyze audio" } }
                    }
                    }
                    if features.lyrics_ai {
                    button {
                        class: "library__tool-btn",
                        disabled: lyrics_queued,
                        title: "Run the AI lyrics analysis on every track",
                        onclick: move |e| {
                            e.stop_propagation();
                            confirm.set(Some(BulkJob::Lyrics));
                        },
                        LyricsIcon { size: 18 }
                        span { class: "tool-label", if lyrics_queued { "Queued…" } else { "Analyze lyrics" } }
                    }
                    }
                }
            }
        }
        if scanning() {
            p { class: "text-muted-xs", "Scanning folders…" }
        }
        if let Some(s) = status() {
            p { class: "library__import-ok", "{s}" }
        }
        if needs_permission() {
            div { class: "alert-error library__alert",
                p { "Tanqi can't read your music: access to music and audio was refused. Allow it in Settings, then come back." }
                button {
                    class: "library__tool-btn",
                    onclick: move |_| {
                        document::eval("TqAndroid.openAppSettings()");
                    },
                    "Open settings"
                }
            }
        }
        if let Some(e) = error() {
            div { class: "alert-error library__alert", "{e}" }
        }
        if let Some(kind) = confirm() {
            {
                let n = library.read().all_tracks().len();
                let (title, mut lines, label) = match kind {
                    BulkJob::Audio => (
                        format!("Analyze audio for {n} tracks?"),
                        vec![
                            "Every track is decoded and measured for bass, mid and treble levels, loudness, tempo and key, and given a suggested EQ. Tracks already analyzed with the current version are reused, so only new ones and older analyses take time.".to_string(),
                            "This keeps the processor busy and can take a while on a big Library (and drains the battery on a phone). It runs in the background, and you can cancel it from the jobs list.".to_string(),
                        ],
                        "Analyze audio",
                    ),
                    BulkJob::Lyrics => (
                        format!("Analyze lyrics for {n} tracks?"),
                        vec![
                            if ai_remote { "Every track's lyrics are looked up and sent to your AI provider, which reads them: a vibe summary".to_string() } else { "Every track's lyrics are looked up and read by the local AI model (Ollama): a vibe summary, annotations, topics and entities. Tracks already analyzed are skipped, and so are those with no lyrics or only instrumental.".to_string() },
                            if ai_remote { "This sends every track's title, artist and lyrics to your provider and uses its credit or quota: one request per track, which adds up on a big Library. It runs in the background, and you can cancel it from the jobs list.".to_string() } else { "This is much heavier than audio analysis: one model run per track, which can take hours on a big Library. It runs in the background, and you can cancel it from the jobs list.".to_string() },
                        ],
                        "Analyze lyrics",
                    ),
                };
                if cfg!(target_os = "android") && kind == BulkJob::Lyrics && !ai_remote {
                    lines.push("On a phone, point Settings -> AI provider at a computer that is running Ollama. If it can't be reached, every track will fail.".to_string());
                }
                rsx! {
                    ConfirmModal {
                        title,
                        lines,
                        confirm_label: label.to_string(),
                        on_cancel: move |_| confirm.set(None),
                        on_confirm: move |_| {
                            confirm.set(None);
                            tools_open.set(false);
                            let tracks = library.read().all_tracks();
                            jobs.enqueue(match kind {
                                BulkJob::Audio => bulk_audio_analysis_job(tracks),
                                BulkJob::Lyrics => bulk_lyrics_analysis_job(tracks),
                            });
                        },
                    }
                }
            }
        }
        if show_folders() {
            FoldersModal {
                folders: folders(),
                busy: scanning(),
                note: error().or(status()),
                start_adding: add_on_open(),
                on_close: move |_| show_folders.set(false),
                on_add_native: move |_| add_native(),
                on_add_path: move |p: String| add_path(p),
                on_remove: move |p: String| remove_folder(p),
                on_rescan: move |_| rescan(),
            }
        }
    };

    if folders().is_empty() {
        return rsx! {
            {header}
            div { class: "library__empty",
                h2 { class: "library__empty-title", "No music yet" }
                p { class: "library__empty-text",
                    "Add the folder your music lives in. It's scanned recursively and organised by the artist, album and track tags in your files."
                }
                button {
                    class: "btn-solid",
                    disabled: scanning(),
                    onclick: move |_| {
                        if cfg!(target_os = "android") {
                            add_on_open.set(true);
                            show_folders.set(true);
                        } else {
                            add_native();
                        }
                    },
                    "Add music folder"
                }
            }
        };
    }

    let current_page = stack.read().last().cloned();
    let body = match current_page {
        Some(Page::Artist(name)) => {
            let artist = library.read().artists.iter().find(|a| group_key(&a.name) == group_key(&name)).cloned();
            match artist {
                Some(artist) => rsx! {
                    ArtistPage {
                        picture: artist_pictures.read().get(&group_key(&artist.name)).cloned(),
                        artist,
                        on_back: move |_| { stack.write().pop(); },
                        on_open_album: move |id: String| stack.write().push(Page::Album(id)),
                        on_info: move |t| info_track.set(Some(t)),
                    }
                },
                None => rsx! { Missing { on_back: move |_| { stack.write().pop(); } } },
            }
        }
        Some(Page::Playlist(id)) => rsx! {
            PlaylistPage { id, tracks: library.read().all_tracks(), on_back: move |_| { stack.write().pop(); } }
        },
        Some(Page::Album(id)) => {
            let album = library.read().albums.iter().find(|a| a.id == id).cloned();
            match album {
                Some(album) => rsx! {
                    AlbumPage {
                        album,
                        on_back: move |_| { stack.write().pop(); },
                        on_open_artist: move |name: String| stack.write().push(Page::Artist(name)),
                        on_info: move |t| info_track.set(Some(t)),
                    }
                },
                None => rsx! { Missing { on_back: move |_| { stack.write().pop(); } } },
            }
        }
        None => {
            let q = query();
            let artists: Vec<Artist> = library
                .read()
                .artists
                .iter()
                .filter(|a| matches(&q, &[&a.name]))
                .cloned()
                .collect();
            let albums: Vec<Album> = library
                .read()
                .albums
                .iter()
                .filter(|al| matches(&q, &[&al.title, &al.artist]))
                .cloned()
                .collect();
            rsx! {
                div { class: "library__filters",
                    for (t, label) in [(Tab::Artists, "Artists"), (Tab::Albums, "Albums"), (Tab::Tracks, "Tracks"), (Tab::Favorites, "Favorites"), (Tab::Playlists, "Playlists")] {
                        FilterPill { key: "{label}", label: label.to_string(), active: tab() == t, on_click: move |_| tab.set(t) }
                    }
                    input {
                        class: "library__search",
                        r#type: "search",
                        placeholder: "Filter…",
                        value: "{query}",
                        oninput: move |e| {
                            query.set(e.value());
                            songs_shown.set(SONGS_PAGE);
                        },
                    }
                }
                match tab() {
                    Tab::Artists => rsx! {
                        div { class: "library__grid",
                            for artist in artists {
                                button {
                                    key: "{artist.name}",
                                    class: "library__card",
                                    onclick: {
                                        let name = artist.name.clone();
                                        move |_| stack.write().push(Page::Artist(name.clone()))
                                    },
                                    TrackThumbnail {
                                        url: artist_pictures.read().get(&group_key(&artist.name)).cloned().or_else(|| artist.cover().map(str::to_string)),
                                        class: "library__card-img",
                                        fallback: None,
                                        kind: ThumbKind::Artist,
                                    }
                                    p { class: "library__card-title", "{artist.name}" }
                                    p { class: "library__card-sub",
                                        "{plural(artist.albums.len(), \"album\")} · {plural(artist.track_count(), \"track\")}"
                                    }
                                }
                            }
                        }
                    },
                    Tab::Albums => rsx! {
                        div { class: "library__grid",
                            for album in albums {
                                AlbumCard {
                                    key: "{album.artist}/{album.title}",
                                    album: album.clone(),
                                    on_open: move |id: String| stack.write().push(Page::Album(id)),
                                }
                            }
                        }
                    },
                    Tab::Playlists => rsx! {
                        PlaylistList { on_open: move |id: i64| stack.write().push(Page::Playlist(id)) }
                    },
                    Tab::Tracks | Tab::Favorites => {
                        let only_favorites = tab() == Tab::Favorites;
                        let list = track_list();
                        let all = list.tracks.clone();
                        let shown = songs_shown().min(all.len());
                        rsx! {
                            if only_favorites && all.is_empty() {
                                p { class: "empty-state", "No favorites yet. Tap the heart on a track." }
                            }
                            div { class: "library__tracks",
                                for t in all.iter().take(shown) {
                                    TrackListRow {
                                        key: "{t.id}",
                                        track: t.clone(),
                                        number: None,
                                        show_artist: true,
                                        show_album: true,
                                        queue: list.queue.clone(),
                                        on_info: move |t| info_track.set(Some(t)),
                                    }
                                }
                            }
                            if shown < all.len() {
                                button {
                                    class: "library__tool-btn library__more",
                                    onclick: move |_| songs_shown.set(songs_shown() + SONGS_PAGE),
                                    "Show more ({all.len() - shown} left)"
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        {header}
        {body}
        if let Some(t) = info_track() {
            SongInfoModal { track: t, on_close: move |_| info_track.set(None) }
        }
    }
}

#[component]
fn Missing(on_back: EventHandler<()>) -> Element {
    rsx! {
        button { class: "library__back", onclick: move |_| on_back.call(()),
            BackIcon { size: 12 }
            "Back" }
        p { class: "text-muted-sm", "That item is no longer in the library." }
    }
}

#[component]
fn AlbumCard(album: Album, on_open: EventHandler<String>) -> Element {
    let id = album.id.clone();
    rsx! {
        button { class: "library__card", onclick: move |_| on_open.call(id.clone()),
            TrackThumbnail { url: album.cover.clone(), class: "library__card-img", fallback: None }
            p { class: "library__card-title", "{album.title}" }
            if let Some(y) = album.year {
                p { class: "library__card-sub", "{y}" }
            }
        }
    }
}

/// An artist's discography: every album, oldest first, each with its tracks.
#[component]
fn ArtistPage(
    artist: Artist,
    /// The artist's own photo, when one has been fetched.
    picture: Option<String>,
    on_back: EventHandler<()>,
    on_open_album: EventHandler<String>,
    on_info: EventHandler<TrackSummary>,
) -> Element {
    let player = use_player();
    let all: Vec<Track> = artist.albums.iter().flat_map(|a| a.tracks.iter().cloned()).collect();
    let queue = SharedQueue::of(&all);
    rsx! {
        button { class: "library__back", onclick: move |_| on_back.call(()),
            BackIcon { size: 12 }
            "Back" }
        div { class: "library__hero",
            TrackThumbnail { url: picture.or_else(|| artist.cover().map(str::to_string)), class: "library__hero-img", fallback: None, kind: ThumbKind::Artist }
            div {
                h2 { class: "library__hero-title", "{artist.name}" }
                p { class: "text-muted-xs", "{plural(artist.albums.len(), \"album\")} · {plural(artist.track_count(), \"track\")}" }
                button {
                    class: "btn-solid library__play-all",
                    onclick: {
                        let queue = queue.clone();
                        move |_| {
                            if let Some(first) = queue.first() {
                                player.play_track(first.clone(), Some(queue.to_vec()));
                            }
                        }
                    },
                    PlayIcon { size: 12 }
                    "Play all"
                }
            }
        }
        for album in artist.albums.iter() {
            div { key: "{album.title}", class: "library__album-block",
                div { class: "library__album-head",
                    TrackThumbnail { url: album.cover.clone(), class: "library__album-thumb", fallback: None }
                    div {
                        button {
                            class: "library__album-link",
                            onclick: {
                                let id = album.id.clone();
                                move |_| on_open_album.call(id.clone())
                            },
                            "{album.title}"
                        }
                        p { class: "text-muted-xs",
                            if album.partial {
                                "On {album.artist} · "
                            }
                            "{album_meta(album)}"
                        }
                    }
                }
                AlbumTracks { album: album.clone(), on_info }
            }
        }
    }
}

#[component]
fn AlbumPage(
    album: Album,
    on_back: EventHandler<()>,
    on_open_artist: EventHandler<String>,
    on_info: EventHandler<TrackSummary>,
) -> Element {
    let player = use_player();
    let queue = SharedQueue::of(&album.tracks);
    let artists = album.artists();
    rsx! {
        button { class: "library__back", onclick: move |_| on_back.call(()),
            BackIcon { size: 12 }
            "Back" }
        div { class: "library__hero",
            TrackThumbnail { url: album.cover.clone(), class: "library__hero-img", fallback: None }
            div {
                h2 { class: "library__hero-title", "{album.title}" }
                p { class: "library__album-artists",
                    for (i, name) in artists.iter().enumerate() {
                        span { key: "{name}",
                            if i > 0 {
                                ", "
                            }
                            button {
                                class: "library__album-link",
                                onclick: {
                                    let name = name.clone();
                                    move |_| on_open_artist.call(name.clone())
                                },
                                "{name}"
                            }
                        }
                    }
                }
                p { class: "text-muted-xs", "{album_meta(&album)}" }
                div { class: "library__album-actions",
                    button {
                        class: "btn-solid library__play-all",
                        onclick: {
                            let queue = queue.clone();
                            move |_| {
                                if let Some(first) = queue.first() {
                                    player.play_track(first.clone(), Some(queue.to_vec()));
                                }
                            }
                        },
                        PlayIcon { size: 12 }
                        "Play album"
                    }
                    AddToPlaylist {
                        track_ids: album.tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
                        class: "library__tool-btn playlists__album-add",
                    }
                }
            }
        }
        AlbumTracks { album, on_info }
    }
}

/// An album's track list, with a "Disc n" divider when it spans several.
#[component]
fn AlbumTracks(album: Album, on_info: EventHandler<TrackSummary>) -> Element {
    let queue = SharedQueue::of(&album.tracks);
    let multi = album.multi_disc;
    // With several artists on the album, every track names its own.
    let multi_artist = album.artists().len() > 1;
    let mut last_disc = None;
    rsx! {
        div { class: "library__tracks",
            for t in album.tracks.iter() {
                {
                    let disc = t.disc_no.unwrap_or(1);
                    let divider = multi && last_disc != Some(disc);
                    last_disc = Some(disc);
                    rsx! {
                        div { key: "{t.id}",
                            if divider {
                                p { class: "library__disc", "Disc {disc}" }
                            }
                            TrackListRow {
                            track: t.clone(),
                            number: t.track_no,
                            show_artist: multi_artist,
                            show_album: false,
                            queue: queue.clone(),
                            on_info,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TrackListRow(
    track: Track,
    number: Option<i64>,
    /// Show the track's artist (and album) under the title - needed on the
    /// flat Songs list and for guests on a compilation.
    show_artist: bool,
    /// Also show the album name after the artist (the flat Songs list; the
    /// album page already says which album it is).
    show_album: bool,
    queue: SharedQueue,
    on_info: EventHandler<TrackSummary>,
) -> Element {
    let player = use_player();
    let favorites = use_favorites();
    // Phone: the secondary actions open as a bottom sheet from the `⋯` button (CSS hides it at >= 960px).
    let mut menu_open = use_signal(|| false);
    let is_fav = favorites.contains(&track.id);
    let summary = to_summary(&track);
    let is_current = player.current_track().map(|t| t.id == track.id).unwrap_or(false);
    let is_playing = (player.is_playing)();
    let sub = match (show_artist, show_album, &track.album) {
        (true, true, Some(album)) => format!("{} · {album}", track.artist),
        (true, _, _) => track.artist.clone(),
        _ => String::new(),
    };
    let (play_t, info_t, next_t, add_t, tap_t) = (summary.clone(), summary.clone(), summary.clone(), summary.clone(), summary);
    let fav_id = track.id.clone();
    rsx! {
        div {
            // `--sheet`: this row keeps only the heart and `⋯` on a phone; its other actions are in the sheet.
            class: if is_current { "library__row library__row--sheet library__row--current" } else { "library__row library__row--sheet" },
            ondoubleclick: {
                let (t, q) = (play_t.clone(), queue.clone());
                move |_| player.play_track(t.clone(), Some(q.to_vec()))
            },
            span { class: "library__row-num",
                if is_current {
                    if is_playing { PauseIcon { size: 11 } } else { PlayIcon { size: 11 } }
                } else if let Some(n) = number {
                    "{n}"
                }
            }
            // A tap plays on touch screens, where there is no double-click.
            div {
                class: "library__row-body",
                onclick: {
                    let (t, q) = (tap_t.clone(), queue.clone());
                    move |_| player.play_track(t.clone(), Some(q.to_vec()))
                },
                p { class: "library__row-title", "{track.title}" }
                if !sub.is_empty() {
                    p { class: "library__row-sub", "{sub}" }
                }
            }
            if let Some(ms) = track.duration_ms {
                span { class: "library__row-time", "{format_duration(ms)}" }
            }
            button {
                class: if is_fav { "library__row-info library__row-fav library__row-fav--on" } else { "library__row-info library__row-fav" },
                title: if is_fav { "Remove from favorites" } else { "Add to favorites" },
                "aria-label": "Favorite",
                "aria-pressed": "{is_fav}",
                onclick: move |_| favorites.toggle(fav_id.clone()),
                HeartIcon { size: 14, filled: is_fav }
            }
            button {
                class: "library__row-info",
                title: "Add to queue",
                "aria-label": "Add to queue",
                onclick: move |_| player.enqueue(add_t.clone()),
                QueueIcon { size: 14 }
            }
            AddToPlaylist { track_ids: vec![track.id.clone()] }
            button {
                class: "library__row-info library__row-next",
                title: "Play next",
                "aria-label": "Play next",
                onclick: move |_| player.play_after_current(next_t.clone()),
                "NEXT"
            }
            button {
                class: "library__row-info",
                title: "Track info",
                "aria-label": "Track info",
                onclick: move |_| on_info.call(info_t.clone()),
                InfoIcon { size: 14 }
            }
            button {
                class: "library__row-more",
                title: "More actions",
                "aria-label": "More actions",
                "aria-expanded": "{menu_open()}",
                onclick: move |_| menu_open.set(true),
                "⋯"
            }
            button {
                class: "library__row-play",
                title: "Play",
                "aria-label": "Play",
                onclick: move |_| player.play_track(play_t.clone(), Some(queue.to_vec())),
                PlayIcon { size: 11 }
            }
        }
        if menu_open() {
            TrackActionsSheet {
                track: to_summary(&track),
                on_info: move |t| on_info.call(t),
                on_close: move |_| menu_open.set(false),
            }
        }
    }
}
