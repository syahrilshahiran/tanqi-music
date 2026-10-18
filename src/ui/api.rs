//! Thin async wrappers over `commands`, runnable from any Dioxus executor.
//! Each one hops onto the app's tokio runtime (see `app_lib::run`). Tracks,
//! Library folders and Scans are not here (the UI calls the Library store
//! through `app_lib::with_store`), nor are online lookups (`with_metadata`)
//! or the Lyrics analyzer (`with_analyzer`).
use app_lib::commands;
use app_lib::models::*;
use app_lib::mpris;

pub async fn get_settings() -> Result<AppSettings, String> {
    app_lib::run(async move { commands::get_settings() }).await
}

pub async fn set_settings(settings: AppSettings) -> Result<(), String> {
    app_lib::run(async move { commands::set_settings(settings) }).await
}

pub async fn list_favorites() -> Result<Vec<String>, String> {
    app_lib::run(async move { commands::list_favorites() }).await
}

pub async fn set_favorite(track_id: String, favorite: bool) -> Result<(), String> {
    app_lib::run(async move { commands::set_favorite(track_id, favorite) }).await
}

pub async fn list_artist_pictures() -> Result<std::collections::HashMap<String, String>, String> {
    app_lib::run(async move { commands::list_artist_pictures() }).await
}

pub async fn fetch_artist_picture(artist: String) -> Result<bool, String> {
    app_lib::run(async move { commands::fetch_artist_picture(artist).await }).await
}

pub async fn clear_api_cache() -> Result<(), String> {
    app_lib::run(async move { commands::clear_api_cache() }).await
}

pub async fn get_data_size() -> DataSize {
    app_lib::run(async move { commands::get_data_size() }).await
}

pub async fn list_playlists() -> Result<Vec<Playlist>, String> {
    app_lib::run(async move { commands::list_playlists() }).await
}

pub async fn create_playlist(name: String, track_ids: Vec<String>) -> Result<i64, String> {
    app_lib::run(async move { commands::create_playlist(name, track_ids) }).await
}

pub async fn rename_playlist(id: i64, name: String) -> Result<(), String> {
    app_lib::run(async move { commands::rename_playlist(id, name) }).await
}

pub async fn delete_playlist(id: i64) -> Result<(), String> {
    app_lib::run(async move { commands::delete_playlist(id) }).await
}

pub async fn add_to_playlist(id: i64, track_ids: Vec<String>) -> Result<(), String> {
    app_lib::run(async move { commands::add_to_playlist(id, track_ids) }).await
}

pub async fn remove_from_playlist(id: i64, position: i64) -> Result<(), String> {
    app_lib::run(async move { commands::remove_from_playlist(id, position) }).await
}

pub async fn move_in_playlist(id: i64, position: i64, delta: i64) -> Result<(), String> {
    app_lib::run(async move { commands::move_in_playlist(id, position, delta) }).await
}

pub async fn fetch_album_cover(artist: String, album: String, ids: Vec<String>) -> Result<bool, String> {
    app_lib::run(async move { commands::fetch_album_cover(artist, album, ids).await }).await
}

pub async fn send_notification(title: String, body: String, target: Option<String>) -> Result<(), String> {
    // Android has no notification crate: the Kotlin side (android/MainActivity.kt) posts it.
    #[cfg(target_os = "android")]
    {
        if app_lib::run(async { commands::notifications_enabled() }).await {
            let js = format!(
                "window.TqAndroid && TqAndroid.notify({}, {}, {})",
                serde_json::to_string(&title).unwrap_or_default(),
                serde_json::to_string(&body).unwrap_or_default(),
                serde_json::to_string(&target.unwrap_or_default()).unwrap_or_default()
            );
            dioxus::prelude::document::eval(&js);
        }
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        app_lib::run(async move { commands::send_notification(title, body, target) }).await
    }
}

pub async fn download_itunes_artwork(url: String, suggested_name: String) -> Result<bool, String> {
    app_lib::run(async move { commands::download_itunes_artwork(url, suggested_name).await }).await
}

pub async fn get_lyrics_analysis(track_id: String) -> Result<Option<LyricsAnalysis>, String> {
    app_lib::run(async move { commands::get_lyrics_analysis(track_id) }).await
}

pub async fn list_analyzed_lyrics() -> Result<Vec<LyricsAnalysis>, String> {
    app_lib::run(async move { commands::list_analyzed_lyrics() }).await
}

pub async fn get_lyrics_translation(track_id: String) -> Result<Option<LyricsTranslation>, String> {
    app_lib::run(async move { commands::get_lyrics_translation(track_id) }).await
}

pub async fn analyze_audio(id: String, title: String, artist: String, thumbnail_url: Option<String>, force: bool) -> Result<AudioAnalysis, String> {
    app_lib::run(async move { commands::analyze_audio(id, title, artist, thumbnail_url, force).await }).await
}

pub async fn get_audio_analysis(track_id: String) -> Result<Option<AudioAnalysis>, String> {
    app_lib::run(async move { commands::get_audio_analysis(track_id) }).await
}

pub async fn list_analyzed_audio() -> Result<Vec<AudioAnalysis>, String> {
    app_lib::run(async move { commands::list_analyzed_audio() }).await
}

pub async fn pick_local_audio_folder() -> Result<Option<String>, String> {
    app_lib::run(async move { commands::pick_local_audio_folder().await }).await
}

pub async fn list_subfolders(path: String) -> Result<commands::FolderListing, String> {
    app_lib::run(async move { commands::list_subfolders(path).await }).await
}

pub fn update_now_playing(state: mpris::NowPlaying) {
    commands::update_now_playing(state)
}

pub async fn list_headphone_profiles() -> Result<Vec<HeadphoneProfile>, String> {
    app_lib::run(async move { commands::list_headphone_profiles() }).await
}

pub async fn save_headphone_profile(name: String, preset: String) -> Result<i64, String> {
    app_lib::run(async move { commands::save_headphone_profile(name, preset) }).await
}

pub async fn delete_headphone_profile(id: i64) -> Result<(), String> {
    app_lib::run(async move { commands::delete_headphone_profile(id) }).await
}

pub async fn get_active_headphone() -> Result<Option<i64>, String> {
    app_lib::run(async move { commands::get_active_headphone() }).await
}

pub async fn set_active_headphone(id: Option<i64>) -> Result<(), String> {
    app_lib::run(async move { commands::set_active_headphone(id) }).await
}

pub async fn update_headphone_profile(id: i64, name: String, preset: String) -> Result<(), String> {
    app_lib::run(async move { commands::update_headphone_profile(id, name, preset) }).await
}

pub async fn delete_data(parts: Vec<DataPart>) -> Result<(), String> {
    app_lib::run(async move { commands::delete_data(parts) }).await
}

pub async fn ai_key_saved(provider: app_lib::models::AiProvider) -> bool {
    app_lib::run(async move { commands::ai_key_saved(provider).await }).await
}

pub async fn set_ai_key(provider: app_lib::models::AiProvider, key: String) -> Result<String, String> {
    app_lib::run(async move { commands::set_ai_key(provider, key).await }).await
}
