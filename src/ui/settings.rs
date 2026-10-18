//! App-wide settings signal: anything that reads `use_settings()`
//! re-renders when it changes.

use super::api;
use app_lib::models::AppSettings;
use dioxus::prelude::*;

/// Provide once, near the root.
pub fn use_settings_provider() -> Signal<AppSettings> {
    let mut settings = use_context_provider(|| Signal::new(AppSettings::default()));
    use_future(move || async move {
        if let Ok(s) = api::get_settings().await {
            settings.set(s);
        }
    });
    settings
}

pub fn use_settings() -> Signal<AppSettings> {
    use_context::<Signal<AppSettings>>()
}

/// Persists new settings and updates the shared signal.
pub async fn save_settings(mut settings: Signal<AppSettings>, next: AppSettings) -> Result<(), String> {
    api::set_settings(next.clone()).await?;
    settings.set(next);
    Ok(())
}

/// Which of the optional features are on (the lyrics themselves are not optional; `lyrics_ai` is their AI analysis) (chosen in onboarding, changeable in Settings). Reading it
/// re-renders the component when a switch changes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Features {
    pub audio: bool,
    pub lyrics_ai: bool,
}

pub fn use_features() -> Features {
    let settings = use_settings();
    let s = settings.read();
    Features { audio: s.audio_analysis_enabled, lyrics_ai: s.lyrics_ai_enabled }
}
