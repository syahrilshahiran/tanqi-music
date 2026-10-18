//! App settings, and deleting stored data by kind.

use super::*;

pub fn get_settings(conn: &Connection) -> rusqlite::Result<AppSettings> {
    let mut settings = AppSettings::default();
    let (mut saw_audio, mut saw_lyrics) = (false, false);
    let (mut saw_ai_model, mut saw_ai_url) = (false, false);
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        match key.as_str() {
            "notifications_enabled" => settings.notifications_enabled = value == "true",
            // `ollama_*` are the names from before there were other providers; the new names win if both exist.
            "ollama_model" if !saw_ai_model => settings.ai_model = value,
            "ollama_url" if !saw_ai_url => settings.ai_url = crate::models::clean_ollama_url(&value),
            "ai_provider" => settings.ai_provider = crate::models::AiProvider::from_id(&value).unwrap_or_default(),
            "ai_model" => {
                settings.ai_model = value;
                saw_ai_model = true;
            }
            "ai_url" => {
                settings.ai_url = value;
                saw_ai_url = true;
            }
            "onboarding_completed" => settings.onboarding_completed = value == "true",
            "analysis_guidance" => settings.analysis_guidance = value,
            "translation_guidance" => settings.translation_guidance = value,
            "audio_analysis_enabled" => {
                settings.audio_analysis_enabled = value == "true";
                saw_audio = true;
            }
            // `lyrics_enabled` is the name from when one switch covered the lyrics and their AI analysis.
            "lyrics_ai_enabled" | "lyrics_enabled" => {
                if key == "lyrics_ai_enabled" || !saw_lyrics {
                    settings.lyrics_ai_enabled = value == "true";
                }
                saw_lyrics = true;
            }
            "eq_strength_percent" => {
                settings.eq_strength_percent = value.parse::<u8>().map(|v| v.min(100)).unwrap_or(crate::models::DEFAULT_EQ_STRENGTH_PERCENT)
            }
            _ => {}
        }
    }
    settings.ai_url = crate::models::clean_ai_url(settings.ai_provider, &settings.ai_url);
    // The optional features were chosen in onboarding. An install that finished onboarding before they
    // existed keeps them on; a fresh install has them off until it chooses.
    if !saw_audio {
        settings.audio_analysis_enabled = settings.onboarding_completed;
    }
    if !saw_lyrics {
        settings.lyrics_ai_enabled = settings.onboarding_completed;
    }
    Ok(settings)
}

pub fn set_settings(conn: &Connection, settings: &AppSettings) -> rusqlite::Result<()> {
    let pairs = [
        ("notifications_enabled", settings.notifications_enabled.to_string()),
        ("ai_provider", settings.ai_provider.id().to_string()),
        ("ai_model", settings.ai_model.clone()),
        ("ai_url", crate::models::clean_ai_url(settings.ai_provider, &settings.ai_url)),
        ("onboarding_completed", settings.onboarding_completed.to_string()),
        ("analysis_guidance", crate::models::clean_guidance(&settings.analysis_guidance)),
        ("translation_guidance", crate::models::clean_guidance(&settings.translation_guidance)),
        ("eq_strength_percent", settings.eq_strength_percent.min(100).to_string()),
        ("audio_analysis_enabled", settings.audio_analysis_enabled.to_string()),
        ("lyrics_ai_enabled", settings.lyrics_ai_enabled.to_string()),
    ];
    for (key, value) in pairs {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}


/// Deletes the chosen kinds of data in one transaction. The scan version is left alone, so a folder added
/// back afterwards is scanned incrementally as usual.
pub fn delete_data(conn: &Connection, parts: &[crate::models::DataPart]) -> rusqlite::Result<()> {
    use crate::models::DataPart::*;
    let tx = conn.unchecked_transaction()?;
    for part in parts {
        let tables: &[&str] = match part {
            Music => &["tracks", "library_folders"],
            AudioAnalyses => &["audio_analysis"],
            LyricsData => &["lyrics_analysis", "lyrics_translation"],
            ArtistPictures => &["artist_pictures"],
            HeadphoneCorrections => &["headphone_profiles"],
            FavoritesAndPlaylists => &["favorites", "playlist_tracks", "playlists"],
        };
        for table in tables {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        if *part == HeadphoneCorrections {
            tx.execute("DELETE FROM settings WHERE key = 'active_headphone'", [])?;
        }
    }
    tx.commit()
}
