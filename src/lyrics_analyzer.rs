//! The Lyrics analyzer: turns a track's lyrics into a Lyrics analysis (vibe,
//! line annotations, topics, entities) or a Lyrics translation through the
//! user's local Ollama model, and saves the result (see GLOSSARY.md).
//!
//! Owns everything between "I have a track" and "the result is saved": the
//! cache check, the model setting, the request and the persisting. Callers
//! that already hold the lyrics use `analyze` / `translate`; the bulk job,
//! which doesn't, uses `analyze_track`.

use crate::db::{self, Db};
use crate::http::Http;
use crate::metadata::Metadata;
use crate::models::{LyricsAnalysis, LyricsAnnotation, LyricsTranslation, Track, TrackSummary};
use crate::ai::{self, AiConfig, ConnectionStatus};
use crate::lyrics_ai;

/// The song a Lyrics analysis or translation is about. `title`, `artist` and
/// `thumbnail_url` are snapshotted into the saved analysis, so it still
/// lists properly if the track is later removed.
#[derive(Debug, Clone, PartialEq)]
pub struct LyricsTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub thumbnail_url: Option<String>,
    pub duration_ms: Option<i64>,
}

impl From<&TrackSummary> for LyricsTrack {
    fn from(t: &TrackSummary) -> Self {
        LyricsTrack {
            id: t.id.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            thumbnail_url: t.thumbnail_url.clone(),
            duration_ms: t.duration_ms,
        }
    }
}

impl From<&Track> for LyricsTrack {
    fn from(t: &Track) -> Self {
        LyricsTrack {
            id: t.id.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            thumbnail_url: t.thumbnail_url.clone(),
            duration_ms: t.duration_ms,
        }
    }
}

pub struct LyricsAnalyzer<H: Http> {
    db: Db,
    http: H,
}

impl<H: Http> LyricsAnalyzer<H> {
    pub fn new(db: Db, http: H) -> Self {
        LyricsAnalyzer { db, http }
    }

    /// Whether the AI provider is reachable (and its key accepted) and the chosen model is available.
    pub async fn test_connection(&self) -> Result<ConnectionStatus, String> {
        let (_, cfg) = self.config().await?;
        ai::check_connection(&self.http, &cfg).await
    }

    /// Runs (or, with `force`, re-runs) the analysis for lyrics the caller
    /// already has. Without `force`, a saved analysis is returned as is.
    pub async fn analyze(&self, track: &LyricsTrack, lyrics: &str, force: bool) -> Result<LyricsAnalysis, String> {
        if !force {
            if let Some(cached) = self.db.with(|c| db::get_lyrics_analysis(c, &track.id))? {
                return Ok(cached);
            }
        }

        let (settings, cfg) = self.config().await?;
        let model = cfg.model.clone();
        let content =
            lyrics_ai::analyze_lyrics(&self.http, &cfg, &track.title, &track.artist, lyrics, settings.analysis_guidance_or_default()).await?;

        let analysis = LyricsAnalysis {
            track_id: track.id.clone(),
            vibe_summary: content.vibe_summary,
            annotations: content
                .annotations
                .into_iter()
                .map(|a| LyricsAnnotation { start_line: a.start_line, end_line: a.end_line, meaning: a.meaning })
                .collect(),
            lines: lyrics_ai::numbered_lyrics_lines(lyrics),
            topics: content.topics,
            entities: content.entities,
            title: track.title.clone(),
            artist: track.artist.clone(),
            thumbnail_url: track.thumbnail_url.clone(),
            model,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.db.with(|c| db::save_lyrics_analysis(c, &analysis))?;
        Ok(analysis)
    }

    /// Fetches the lyrics itself, then analyzes them. `Ok(None)` when there
    /// is nothing to analyze - no lyrics found, empty lyrics, or an
    /// instrumental - which is a skip, not a failure. A saved analysis is
    /// returned without fetching anything unless `force`.
    pub async fn analyze_track(
        &self,
        metadata: &Metadata<impl Http>,
        track: &LyricsTrack,
        force: bool,
    ) -> Result<Option<LyricsAnalysis>, String> {
        if !force {
            if let Some(cached) = self.db.with(|c| db::get_lyrics_analysis(c, &track.id))? {
                return Ok(Some(cached));
            }
        }
        let Some(info) = metadata.lyrics(&track.title, &track.artist, track.duration_ms).await? else {
            return Ok(None);
        };
        match info.lyrics {
            Some(lyrics) if !info.instrumental && !lyrics.is_empty() => self.analyze(track, &lyrics, force).await.map(Some),
            _ => Ok(None),
        }
    }

    /// Runs (or re-runs, with `force`) a line-by-line English translation.
    /// Lighter than `analyze`; meant for any non-English lyrics.
    pub async fn translate(&self, track: &LyricsTrack, lyrics: &str, force: bool) -> Result<LyricsTranslation, String> {
        if !force {
            if let Some(cached) = self.db.with(|c| db::get_lyrics_translation(c, &track.id))? {
                return Ok(cached);
            }
        }

        let (settings, cfg) = self.config().await?;
        let model = cfg.model.clone();
        let translations = lyrics_ai::translate_lyrics(
            &self.http,
            &cfg,
            &track.title,
            &track.artist,
            lyrics,
            settings.translation_guidance_or_default(),
        )
        .await?;

        let translation = LyricsTranslation {
            track_id: track.id.clone(),
            lines: lyrics_ai::numbered_lyrics_lines(lyrics),
            translations,
            model,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.db.with(|c| db::save_lyrics_translation(c, &translation))?;
        Ok(translation)
    }

    fn settings(&self) -> Result<crate::models::AppSettings, String> {
        Ok(self.db.with(|c| db::get_settings(c))?)
    }

    /// The settings and the provider config made from them, with the API key from the keychain (read off
    /// the async threads: a keychain can take a moment).
    async fn config(&self) -> Result<(crate::models::AppSettings, AiConfig), String> {
        let settings = self.settings()?;
        let provider = settings.ai_provider;
        let key = tokio::task::spawn_blocking(move || crate::secret::ai_key(provider)).await.map_err(|e| e.to_string())?;
        let cfg = AiConfig::new(&settings, key);
        Ok((settings, cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use serde_json::json;

    fn track() -> LyricsTrack {
        LyricsTrack {
            id: "local:/m/a.flac".into(),
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            thumbnail_url: Some("local:/covers/x.jpg".into()),
            duration_ms: Some(355_000),
        }
    }

    const LYRICS: &str = "Is this the real life?\nIs this just fantasy?\nCaught in a landslide";

    /// Ollama's `/api/generate` envelope around the model's own JSON text.
    fn generate(model_json: serde_json::Value) -> serde_json::Value {
        json!({ "response": model_json.to_string() })
    }

    fn analysis_reply() -> serde_json::Value {
        generate(json!({
            "vibeSummary": "Operatic and dramatic.",
            "topics": ["fate"],
            "entities": ["Galileo"],
            "annotations": [
                {"startLine": 1, "endLine": 2, "meaning": "Doubting reality."},
                {"startLine": 9, "endLine": 10, "meaning": "Out of range, dropped."}
            ]
        }))
    }

    fn analyzer(http: FakeHttp) -> LyricsAnalyzer<FakeHttp> {
        LyricsAnalyzer::new(Db::in_memory().unwrap(), http)
    }

    fn generate_calls(a: &LyricsAnalyzer<FakeHttp>) -> usize {
        a.http.requests().iter().filter(|r| r.contains("/api/generate")).count()
    }

    fn lyrics_metadata(body: serde_json::Value) -> Metadata<FakeHttp> {
        Metadata::new(FakeHttp::default().json("lrclib.net/api/get", body))
    }

    #[tokio::test]
    async fn analyzing_saves_the_result_with_the_chosen_model_and_drops_out_of_range_annotations() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        let result = a.analyze(&track(), LYRICS, false).await.unwrap();

        assert_eq!(result.vibe_summary, "Operatic and dramatic.");
        assert_eq!(result.annotations.len(), 1);
        assert_eq!(result.lines.len(), 3);
        assert_eq!(result.model, "qwen3:8b");
        assert_eq!(result.thumbnail_url.as_deref(), Some("local:/covers/x.jpg"));

        let saved = a.db.with(|c| db::get_lyrics_analysis(c, "local:/m/a.flac")).unwrap();
        assert_eq!(saved, Some(result));
    }

    #[tokio::test]
    async fn the_model_comes_from_settings_and_goes_into_the_request() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        let mut settings = crate::models::AppSettings::default();
        settings.ai_model = "llama3:8b".into();
        a.db.with(|c| db::set_settings(c, &settings)).unwrap();

        let result = a.analyze(&track(), LYRICS, false).await.unwrap();

        assert_eq!(result.model, "llama3:8b");
        assert!(a.http.requests()[0].contains("\"model\":\"llama3:8b\""));
    }

    #[tokio::test]
    async fn requests_go_to_the_ollama_address_in_settings() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        let mut settings = crate::models::AppSettings::default();
        settings.ai_url = "http://192.168.1.20:11434".into();
        a.db.with(|c| db::set_settings(c, &settings)).unwrap();

        a.analyze(&track(), LYRICS, false).await.unwrap();

        assert!(a.http.requests()[0].contains("http://192.168.1.20:11434/api/generate"), "{:?}", a.http.requests());
    }

    #[tokio::test]
    async fn a_saved_analysis_is_reused_unless_forced() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        a.analyze(&track(), LYRICS, false).await.unwrap();
        a.analyze(&track(), LYRICS, false).await.unwrap();
        assert_eq!(generate_calls(&a), 1);

        a.analyze(&track(), LYRICS, true).await.unwrap();
        assert_eq!(generate_calls(&a), 2);
    }

    #[tokio::test]
    async fn ollama_problems_are_reported_in_words_and_nothing_is_saved() {
        let a = analyzer(FakeHttp::default().unreachable("/api/generate", "connection refused"));
        let err = a.analyze(&track(), LYRICS, false).await.unwrap_err();
        assert_eq!(err, "couldn't reach Ollama at http://localhost:11434 - is `ollama serve` running, and is the address right? (connection refused)");

        let a = analyzer(FakeHttp::default().status("/api/generate", 404));
        let err = a.analyze(&track(), LYRICS, false).await.unwrap_err();
        assert!(err.starts_with("Ollama returned 404 Not Found - is `qwen3:8b` pulled?"), "{err}");

        let a = analyzer(FakeHttp::default().json("/api/generate", json!({"response": "sorry, no idea"})));
        let err = a.analyze(&track(), LYRICS, false).await.unwrap_err();
        assert!(err.contains("didn't return any JSON - try re-running the analysis"), "{err}");

        let a = analyzer(FakeHttp::default().json("/api/generate", json!({"response": "{\"topics\": 5}"})));
        assert!(a.analyze(&track(), LYRICS, false).await.unwrap_err().contains("returned malformed JSON"));

        assert!(a.db.with(|c| db::get_lyrics_analysis(c, "local:/m/a.flac")).unwrap().is_none());
    }

    #[tokio::test]
    async fn translating_leaves_a_missing_line_empty_and_is_cached() {
        let reply = generate(json!({"translations": [
            {"line": 1, "text": "Is this the real life?"},
            {"line": 3, "text": "Caught in a landslide"},
            {"line": 99, "text": "out of range"}
        ]}));
        let a = analyzer(FakeHttp::default().json("/api/generate", reply));

        let t = a.translate(&track(), LYRICS, false).await.unwrap();
        assert_eq!(t.translations, vec!["Is this the real life?", "", "Caught in a landslide"]);
        assert_eq!(t.lines.len(), 3);

        a.translate(&track(), LYRICS, false).await.unwrap();
        assert_eq!(generate_calls(&a), 1);
        assert!(a.translate(&track(), LYRICS, true).await.is_ok());
        assert_eq!(generate_calls(&a), 2);
    }

    #[tokio::test]
    async fn a_translation_with_no_json_says_to_retry_the_translation() {
        let a = analyzer(FakeHttp::default().json("/api/generate", json!({"response": "nope"})));
        let err = a.translate(&track(), LYRICS, false).await.unwrap_err();
        assert!(err.contains("try re-running the translation"), "{err}");
    }

    // ---------- analyze_track: fetch lyrics, then skip or analyze ----------

    #[tokio::test]
    async fn a_track_with_lyrics_is_fetched_and_analyzed() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        let m = lyrics_metadata(json!({"instrumental": false, "plainLyrics": LYRICS}));

        let result = a.analyze_track(&m, &track(), false).await.unwrap();

        assert!(result.is_some());
        assert!(m.http_requests()[0].contains("duration=355"));
    }

    #[tokio::test]
    async fn instrumental_empty_or_missing_lyrics_are_skipped_without_calling_ollama() {
        for body in [
            json!({"instrumental": true, "plainLyrics": null}),
            json!({"instrumental": true, "plainLyrics": LYRICS}),
            json!({"instrumental": false, "plainLyrics": ""}),
            json!({"instrumental": false, "plainLyrics": null}),
        ] {
            let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
            let m = lyrics_metadata(body);
            assert_eq!(a.analyze_track(&m, &track(), false).await.unwrap(), None);
            assert_eq!(generate_calls(&a), 0);
        }

        // LRCLIB knows nothing about the track at all.
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        let m = Metadata::new(FakeHttp::default().json("/search", json!([])));
        assert_eq!(a.analyze_track(&m, &track(), false).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_saved_analysis_means_no_lyrics_fetch_unless_forced() {
        let a = analyzer(FakeHttp::default().json("/api/generate", analysis_reply()));
        a.analyze(&track(), LYRICS, false).await.unwrap();
        let m = lyrics_metadata(json!({"instrumental": false, "plainLyrics": LYRICS}));

        assert!(a.analyze_track(&m, &track(), false).await.unwrap().is_some());
        assert!(m.http_requests().is_empty(), "no LRCLIB request for an already-analyzed track");

        assert!(a.analyze_track(&m, &track(), true).await.unwrap().is_some());
        assert!(!m.http_requests().is_empty());
    }

    #[tokio::test]
    async fn a_lyrics_lookup_failure_is_a_failure_not_a_skip() {
        let a = analyzer(FakeHttp::default());
        let m = Metadata::new(FakeHttp::default().status("/get", 500));
        assert!(a.analyze_track(&m, &track(), false).await.is_err());
    }

    // ---------- connection test ----------

    #[tokio::test]
    async fn the_connection_test_reports_whether_the_chosen_model_is_pulled() {
        let tags = json!({"models": [{"name": "llama3:8b"}, {"name": "qwen3:8b"}]});
        let a = analyzer(FakeHttp::default().json("/api/tags", tags.clone()));
        let status = a.test_connection().await.unwrap();
        assert!(status.model_found);
        assert_eq!(status.models, vec!["llama3:8b", "qwen3:8b"]);

        let a = analyzer(FakeHttp::default().json("/api/tags", json!({"models": [{"name": "llama3:8b"}]})));
        assert!(!a.test_connection().await.unwrap().model_found);

        let a = analyzer(FakeHttp::default().unreachable("/api/tags", "refused"));
        assert!(a.test_connection().await.unwrap_err().starts_with("couldn't reach Ollama"));
    }

    #[tokio::test]
    async fn a_hosted_provider_gets_the_saved_key_and_the_result_names_its_model() {
        use crate::models::AiProvider;
        let a = analyzer(FakeHttp::default().json(
            "/v1/messages",
            json!({"content": [{"type": "text", "text": analysis_reply()["response"].as_str().unwrap()}]}),
        ));
        let mut settings = crate::models::AppSettings::default();
        settings.ai_provider = AiProvider::Anthropic;
        settings.ai_url = AiProvider::Anthropic.default_url().into();
        settings.ai_model = "claude-test".into();
        a.db.with(|c| db::set_settings(c, &settings)).unwrap();
        crate::secret::set_ai_key(AiProvider::Anthropic, "sk-ant-test").unwrap();

        let result = a.analyze(&track(), LYRICS, false).await.unwrap();

        assert_eq!(result.model, "claude-test");
        assert_eq!(a.http.headers_sent()[0], "x-api-key: sk-ant-test");
        crate::secret::clear_ai_key(AiProvider::Anthropic);
    }
}
