use crate::models::{AppSettings, AudioAnalysis, HeadphoneProfile, LyricsAnalysis, LyricsTranslation, Playlist};
use crate::{audio, db};

pub fn get_settings() -> Result<AppSettings, String> {
    crate::app::db().read(|c| db::get_settings(c)).map_err(String::from)
}

pub fn set_settings(settings: AppSettings) -> Result<(), String> {
    crate::app::db().with(|c| db::set_settings(c, &settings)).map_err(String::from)
}

/// Lets the job queue announce a finished job, if the user has
/// notifications turned on.
/// Whether the user has background-job notifications switched on (Settings).
pub fn notifications_enabled() -> bool {
    crate::app::db().read(|c| db::get_settings(c)).map(|s| s.notifications_enabled).unwrap_or(true)
}

pub fn send_notification(title: String, body: String, target: Option<String>) -> Result<(), String> {
    let enabled = {
        crate::app::db().read(|c| db::get_settings(c)).map(|s| s.notifications_enabled).unwrap_or(true)
    };
    if !enabled {
        return Ok(());
    }
    crate::app::notify(&title, &body, target)
}

pub fn list_favorites() -> Result<Vec<String>, String> {
    crate::app::db().read(|c| db::list_favorite_ids(c)).map_err(String::from)
}

pub fn set_favorite(track_id: String, favorite: bool) -> Result<(), String> {
    crate::app::db().with(|c| db::set_favorite(c, &track_id, favorite)).map_err(String::from)
}

// ---------- artist pictures ----------

/// Saved artist pictures by artist key (see `library::group_key`).
pub fn list_artist_pictures() -> Result<std::collections::HashMap<String, String>, String> {
    crate::app::db().read(|c| db::list_artist_pictures(c)).map(|v| v.into_iter().collect()).map_err(String::from)
}

/// Looks an artist's picture up on YouTube Music, saves it next to the
/// covers and remembers it. `Ok(false)` when there is no exact match.
pub async fn fetch_artist_picture(artist: String) -> Result<bool, String> {
    let Some(bytes) = crate::app::metadata().artist_picture(&artist).await? else {
        return Ok(false);
    };
    let cover = audio::EmbeddedCover { ext: "jpg", data: bytes };
    let path = crate::scanner::cache_embedded_cover(&cover).ok_or("couldn't save the picture")?;
    let thumb = crate::local_ref::encode(&path);
    let key = crate::library::group_key(&artist);
    crate::app::db().with(|c| db::set_artist_picture(c, &key, &thumb)).map_err(String::from)?;
    Ok(true)
}

// ---------- data size ----------

fn dir_bytes(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries
        .filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// What the app's own files take on disk (never the music itself).
pub fn get_data_size() -> crate::models::DataSize {
    let dir = crate::app::app_data_dir();
    let file = |name: &str| std::fs::metadata(dir.join(name)).map(|m| m.len()).unwrap_or(0);
    crate::models::DataSize {
        database_bytes: file("tanqi-music.db") + file("tanqi-music.db-wal") + file("tanqi-music.db-shm"),
        covers_bytes: dir_bytes(&dir.join("covers")),
        cache_entries: crate::app::db().read(|c| db::cache_stats(c)).map(|(n, _)| n.max(0) as u64).unwrap_or(0),
    }
}

/// Forgets every remembered lookup answer; the next look-up asks again.
pub fn clear_api_cache() -> Result<(), String> {
    crate::app::db().with(|c| db::cache_clear(c)).map_err(String::from)
}

/// Deletes the chosen kinds of the app's own data (never the music files).
pub fn delete_data(parts: Vec<crate::models::DataPart>) -> Result<(), String> {
    crate::app::db().with(|c| db::delete_data(c, &parts)).map_err(String::from)
}

// ---------- headphone profiles ----------

pub fn list_headphone_profiles() -> Result<Vec<HeadphoneProfile>, String> {
    crate::app::db().read(|c| db::list_headphone_profiles(c)).map_err(String::from)
}

pub fn save_headphone_profile(name: String, preset: String) -> Result<i64, String> {
    crate::app::db().with(|c| db::save_headphone_profile(c, name.trim(), &preset)).map_err(String::from)
}

pub fn update_headphone_profile(id: i64, name: String, preset: String) -> Result<(), String> {
    crate::app::db().with(|c| db::update_headphone_profile(c, id, name.trim(), &preset)).map_err(String::from)
}

pub fn delete_headphone_profile(id: i64) -> Result<(), String> {
    crate::app::db().with(|c| db::delete_headphone_profile(c, id)).map_err(String::from)
}

pub fn get_active_headphone() -> Result<Option<i64>, String> {
    crate::app::db().read(|c| db::get_active_headphone(c)).map_err(String::from)
}

pub fn set_active_headphone(id: Option<i64>) -> Result<(), String> {
    crate::app::db().with(|c| db::set_active_headphone(c, id)).map_err(String::from)
}

// ---------- playlists ----------

pub fn list_playlists() -> Result<Vec<Playlist>, String> {
    crate::app::db().read(|c| db::list_playlists(c)).map_err(String::from)
}

pub fn create_playlist(name: String, track_ids: Vec<String>) -> Result<i64, String> {
    crate::app::db()
        .with(|c| {
            let id = db::create_playlist(c, name.trim())?;
            db::add_to_playlist(c, id, &track_ids)?;
            Ok::<_, rusqlite::Error>(id)
        })
        .map_err(String::from)
}

pub fn rename_playlist(id: i64, name: String) -> Result<(), String> {
    crate::app::db().with(|c| db::rename_playlist(c, id, name.trim())).map_err(String::from)
}

pub fn delete_playlist(id: i64) -> Result<(), String> {
    crate::app::db().with(|c| db::delete_playlist(c, id)).map_err(String::from)
}

pub fn add_to_playlist(id: i64, track_ids: Vec<String>) -> Result<(), String> {
    crate::app::db().with(|c| db::add_to_playlist(c, id, &track_ids)).map_err(String::from)
}

pub fn remove_from_playlist(id: i64, position: i64) -> Result<(), String> {
    crate::app::db().with(|c| db::remove_from_playlist(c, id, position)).map_err(String::from)
}

pub fn move_in_playlist(id: i64, position: i64, delta: i64) -> Result<(), String> {
    crate::app::db().with(|c| db::move_in_playlist(c, id, position, delta)).map_err(String::from)
}


/// Native file dialogs. Android has none (`rfd` doesn't build there), so the
/// pickers answer "cancelled", except the folder picker, which offers the
/// device's Music folder.
#[cfg(not(target_os = "android"))]
mod dialog {
    use std::path::PathBuf;

    pub fn save_file(name: String, filter: (&'static str, &'static [&'static str])) -> Option<PathBuf> {
        rfd::FileDialog::new().set_file_name(&name).add_filter(filter.0, filter.1).save_file()
    }
    pub fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }
}

#[cfg(target_os = "android")]
mod dialog {
    use std::path::PathBuf;

    pub fn save_file(_name: String, _filter: (&'static str, &'static [&'static str])) -> Option<PathBuf> {
        None
    }
    pub fn pick_folder() -> Option<PathBuf> {
        Some(PathBuf::from("/storage/emulated/0/Music"))
    }
}

/// One level of the device's folders, for the in-app folder browser: Android
/// has no native folder dialog, so the Library's "Add folder" walks the
/// storage itself.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderListing {
    pub path: String,
    /// `None` at the top (`/storage`), where going up would leave the volumes.
    pub parent: Option<String>,
    /// `(name, path)` of each sub-folder, A-Z, hidden ones left out.
    pub folders: Vec<(String, String)>,
    /// Audio files directly in this folder (not in its sub-folders).
    pub audio_files: usize,
}

pub async fn list_subfolders(path: String) -> Result<FolderListing, String> {
    tokio::task::spawn_blocking(move || {
        let dir = std::path::PathBuf::from(&path);
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("can't open {path}: {e}"))?;
        let (mut folders, mut audio_files) = (Vec::new(), 0);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                folders.push((name, p.to_string_lossy().into_owned()));
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| crate::scanner::LOCAL_AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
            {
                audio_files += 1;
            }
        }
        folders.sort_by_key(|(name, _)| name.to_lowercase());
        let parent = if path == "/storage" || path == "/" {
            None
        } else {
            dir.parent().map(|p| p.to_string_lossy().into_owned())
        };
        Ok(FolderListing { path, parent, folders, audio_files })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- artwork ----------

/// Prompts the user with a native save dialog, then downloads the chosen
/// artwork resolution to that path. `Ok(false)` means the user cancelled
/// the dialog, not a failure.
pub async fn download_itunes_artwork(url: String, suggested_name: String) -> Result<bool, String> {
    let picked = tokio::task::spawn_blocking(move || {
        dialog::save_file(suggested_name, ("Image", &["jpg", "jpeg", "png"]))
    })
    .await
    .map_err(|e| e.to_string())?;

    let Some(path) = picked else {
        return Ok(false);
    };

    let bytes = crate::app::metadata().download(&url).await?;

    std::fs::write(&path, &bytes).map_err(|e| format!("failed to write file: {e}"))?;
    Ok(true)
}

pub fn get_lyrics_analysis(track_id: String) -> Result<Option<LyricsAnalysis>, String> {
    crate::app::db().read(|c| db::get_lyrics_analysis(c, &track_id)).map_err(String::from)
}

pub fn list_analyzed_lyrics() -> Result<Vec<LyricsAnalysis>, String> {
    crate::app::db().read(|c| db::list_analyzed_lyrics(c)).map_err(String::from)
}

pub fn get_lyrics_translation(track_id: String) -> Result<Option<LyricsTranslation>, String> {
    crate::app::db().read(|c| db::get_lyrics_translation(c, &track_id)).map_err(String::from)
}

/// Measures a song's bass/mid/treble/overall levels directly from its audio
/// file - no Ollama involved, unlike the Lyrics analyzer. Only Tracks whose
/// `id` is a Local reference (see `local_ref`) can be analyzed; the
/// file is read from where it already lives on disk. This can take a while
/// for a full song - callers should show a loading state, not block on it
/// eagerly. `title`/`artist`/`thumbnail_url` are snapshotted, so the track
/// shows up in the EQ browser (`list_analyzed_audio`) whether or not it's
/// still in the library.
pub async fn analyze_audio(
    id: String,
    title: String,
    artist: String,
    thumbnail_url: Option<String>,
    force: bool,
) -> Result<AudioAnalysis, String> {
    if !force {
        // An analysis from an older version of the algorithm (no stored spectrum, say) is measured again.
        if let Some(cached) = crate::app::db().read(|c| db::get_audio_analysis(c, &id))? {
            if cached.analysis_version >= crate::models::ANALYSIS_VERSION {
                return Ok(audio::with_current_suggestion(cached));
            }
        }
    }

    let path = crate::local_ref::decode(&id).ok_or_else(|| "only local files can be analyzed".to_string())?.to_string();
    // CPU-bound for seconds: on a blocking thread, so the async workers keep serving lyrics and network calls,
    // and the file is read as it decodes instead of being held whole.
    let levels = tokio::task::spawn_blocking(move || audio::analyze_file(&path))
        .await
        .map_err(|e| format!("analysis task failed: {e}"))??;

    let analysis = levels.into_analysis(id, title, artist, thumbnail_url);

    crate::app::db().with(|c| db::save_audio_analysis(c, &analysis))?;

    Ok(audio::with_current_suggestion(analysis))
}

pub fn get_audio_analysis(track_id: String) -> Result<Option<AudioAnalysis>, String> {
    crate::app::db()
        .read(|c| db::get_audio_analysis(c, &track_id))
        .map(|a| a.map(audio::with_current_suggestion))
        .map_err(String::from)
}

/// Every track that's had its audio analyzed, most recent first - the list
/// behind the EQ page's browser.
pub fn list_analyzed_audio() -> Result<Vec<AudioAnalysis>, String> {
    crate::app::db()
        .read(|c| db::list_analyzed_audio(c))
        .map(|v| v.into_iter().map(audio::with_current_suggestion).collect())
        .map_err(String::from)
}

/// Prompts a native "choose folder" dialog. `Ok(None)` means the user
/// cancelled, not a failure. Doesn't scan the folder itself - see
/// `LibraryStore::add_folder`, which takes the path this returns.
pub async fn pick_local_audio_folder() -> Result<Option<String>, String> {
    let picked = tokio::task::spawn_blocking(dialog::pick_folder)
        .await
        .map_err(|e| e.to_string())?;

    let Some(path) = picked else {
        return Ok(None);
    };
    path.into_os_string()
        .into_string()
        .map(Some)
        .map_err(|_| "folder path is not valid UTF-8".to_string())
}

/// Looks the album up on iTunes, downloads its cover into the app's cover
/// cache and attaches it to `ids`. Ok(false) when iTunes has no
/// confident match - not an error.
pub async fn fetch_album_cover(
    artist: String,
    album: String,
    ids: Vec<String>,
) -> Result<bool, String> {
    let Some(bytes) = crate::app::metadata().album_cover(&artist, &album).await? else {
        return Ok(false);
    };
    let cover = audio::EmbeddedCover { ext: "jpg", data: bytes };
    let path = crate::scanner::cache_embedded_cover(&cover).ok_or("couldn't save the cover")?;
    let thumb = crate::local_ref::encode(&path);

    crate::app::store().set_thumbnail(&ids, &thumb)?;
    Ok(true)
}

// ---------- now playing ----------

/// Pushes the webview's current playback state out to MPRIS (see
/// `mpris::start`). Called by the player (`ui/player.rs`) whenever anything a desktop
/// media widget would display or act on changes - track, play/pause,
/// position, volume, shuffle/repeat, or whether next/previous are possible.
///
/// Infallible by design: no D-Bus session (a bare TTY, some containers)
/// means no media-key integration, which shouldn't surface to the user as a
/// playback error.
pub fn update_now_playing(state: crate::mpris::NowPlaying) {
    if let Some(mpris) = crate::app::mpris() {
        mpris.update(state);
    }
}

// ---------- the AI provider's API key ----------

/// Whether a key is saved for the provider (never the key itself).
pub async fn ai_key_saved(provider: crate::models::AiProvider) -> bool {
    tokio::task::spawn_blocking(move || crate::secret::ai_key(provider).is_some()).await.unwrap_or(false)
}

/// Saves the provider's API key in the system keychain (or, where there is none, a private file). The reply
/// says which, for the Settings page to tell the user. An empty key removes it.
pub async fn set_ai_key(provider: crate::models::AiProvider, key: String) -> Result<String, String> {
    let empty = key.trim().is_empty();
    let stored = tokio::task::spawn_blocking(move || crate::secret::set_ai_key(provider, &key))
        .await
        .map_err(|e| e.to_string())??;
    Ok(match (empty, stored) {
        (true, _) => "Key removed.".to_string(),
        (_, crate::secret::Stored::Keychain) => "Key saved in the system keychain.".to_string(),
        (_, crate::secret::Stored::File) => "Key saved in a private file in the app's data folder (no system keychain was available).".to_string(),
    })
}
