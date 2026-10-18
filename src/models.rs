use serde::{Deserialize, Serialize};

/// Outcome of scanning Library folders (see `LibraryStore::add_folder` and
/// `scan_all`): files new to the library, files whose tags were re-read
/// because they changed, and Tracks dropped because their file is gone.
/// Files that haven't changed are counted only in `total`.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Audio files currently present across the scanned folders.
    pub total: usize,
}

pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Which service reads the lyrics: a model on this or another machine (Ollama), or a hosted one reached
/// with an API key.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiProvider {
    /// Ollama's own API.
    #[default]
    Ollama,
    /// Anything that speaks the OpenAI chat-completions API: OpenAI, Gemini's compatible endpoint, OpenRouter,
    /// Groq, LM Studio, Ollama's `/v1`...
    OpenAi,
    /// Anthropic's Messages API (Claude).
    Anthropic,
}

impl AiProvider {
    pub const ALL: [AiProvider; 3] = [AiProvider::Ollama, AiProvider::OpenAi, AiProvider::Anthropic];

    pub fn label(self) -> &'static str {
        match self {
            AiProvider::Ollama => "Ollama (local)",
            AiProvider::OpenAi => "OpenAI-compatible",
            AiProvider::Anthropic => "Claude (Anthropic)",
        }
    }

    /// The stored / form value.
    pub fn id(self) -> &'static str {
        match self {
            AiProvider::Ollama => "ollama",
            AiProvider::OpenAi => "openai",
            AiProvider::Anthropic => "anthropic",
        }
    }

    pub fn from_id(id: &str) -> Option<AiProvider> {
        AiProvider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn default_url(self) -> &'static str {
        match self {
            AiProvider::Ollama => DEFAULT_OLLAMA_URL,
            AiProvider::OpenAi => "https://api.openai.com/v1",
            AiProvider::Anthropic => "https://api.anthropic.com",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            AiProvider::Ollama => "qwen3:8b",
            AiProvider::OpenAi => "gpt-4o-mini",
            AiProvider::Anthropic => "claude-haiku-5-5",
        }
    }

    /// Whether the provider is reached with an API key.
    pub fn needs_key(self) -> bool {
        self != AiProvider::Ollama
    }
}

/// Ready-made addresses for services that speak the OpenAI API: `(name, address, a model to start with)`.
pub const OPENAI_COMPATIBLE_PRESETS: [(&str, &str, &str); 5] = [
    ("OpenAI", "https://api.openai.com/v1", "gpt-4o-mini"),
    ("Gemini", "https://generativelanguage.googleapis.com/v1beta/openai", "gemini-2.0-flash"),
    ("OpenRouter", "https://openrouter.ai/api/v1", "openai/gpt-4o-mini"),
    ("Groq", "https://api.groq.com/openai/v1", "llama-3.3-70b-versatile"),
    ("LM Studio (this machine)", "http://localhost:1234/v1", ""),
];

/// The address of an AI provider as a person types it, made into one that can be called. Ollama keeps its
/// special handling (port, scheme); the others get `https://` if no scheme is given and no trailing slash.
/// Empty means the provider's default.
pub fn clean_ai_url(provider: AiProvider, raw: &str) -> String {
    if provider == AiProvider::Ollama {
        return clean_ollama_url(raw);
    }
    let t = raw.trim().trim_end_matches('/');
    if t.is_empty() {
        return provider.default_url().to_string();
    }
    if t.contains("://") { t.to_string() } else { format!("https://{t}") }
}

fn default_ollama_url() -> String {
    DEFAULT_OLLAMA_URL.to_string()
}

/// An address as a person types it ("192.168.1.20", "pc.local:11434/") made into one Ollama can be
/// reached at: `http://` added if missing, `:11434` added if no port is given, no trailing slash.
/// Empty means the default.
pub fn clean_ollama_url(raw: &str) -> String {
    let t = raw.trim().trim_end_matches('/');
    if t.is_empty() {
        return default_ollama_url();
    }
    let with_scheme = if t.contains("://") { t.to_string() } else { format!("http://{t}") };
    let rest = with_scheme.split_once("://").map(|(_, r)| r).unwrap_or(&with_scheme);
    let host_and_port = rest.split('/').next().unwrap_or("");
    let has_port = if host_and_port.starts_with('[') {
        // IPv6 literal: "[::1]" has no port, "[::1]:11434" does.
        host_and_port.contains("]:")
    } else {
        host_and_port.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    };
    if has_port {
        with_scheme
    } else {
        // Insert before any path ("host/api" -> "host:11434/api").
        match rest.split_once('/') {
            Some((host, path)) => format!("{}://{host}:11434/{path}", with_scheme.split_once("://").map(|(s, _)| s).unwrap_or("http")),
            None => format!("{with_scheme}:11434"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Desktop notification when a background job (bulk analysis) finishes.
    pub notifications_enabled: bool,
    #[serde(default)]
    pub ai_provider: AiProvider,
    /// The model the lyrics AI uses (a tag for Ollama, an id for the hosted ones).
    pub ai_model: String,
    /// Where the provider is: for Ollama this machine by default, or another one (a PC on the same network,
    /// say); for the others their API address. The API key is not here: it is kept in the system keychain.
    #[serde(default = "default_ollama_url")]
    pub ai_url: String,
    /// Whether the first-run folder picker has already been shown (or was
    /// skipped because the library already had tracks).
    pub onboarding_completed: bool,
    /// The editable part of the lyrics-analysis prompt: the role and style
    /// the model writes in. The reply's shape and field rules are not part of
    /// it and can't be changed from settings. Empty means the default.
    #[serde(default)]
    pub analysis_guidance: String,
    /// The same for the lyrics translation prompt.
    #[serde(default)]
    pub translation_guidance: String,
    /// How much of the Suggested EQ is applied, 0-100 (see `GLOSSARY.md`).
    #[serde(default = "default_eq_strength_percent")]
    pub eq_strength_percent: u8,
    /// Whether the optional Audio analysis feature (analysis, Suggested EQ, loudness levelling, the EQ page) is on.
    /// Chosen in onboarding; existing installs keep it on.
    #[serde(default = "default_true")]
    pub audio_analysis_enabled: bool,
    /// Whether the optional AI lyrics features (the deep dive, the translation, the Lyrics page) are on. The
    /// lyrics themselves (lookup, display, following along) are always on.
    #[serde(default = "default_true")]
    pub lyrics_ai_enabled: bool,
}

/// The version of the audio analysis algorithm. Raise it when a change means saved analyses should be
/// measured again (the bulk "Analyze audio" redoes older ones).
pub const ANALYSIS_VERSION: u32 = 2;

pub const DEFAULT_EQ_STRENGTH_PERCENT: u8 = 70;

fn default_eq_strength_percent() -> u8 {
    DEFAULT_EQ_STRENGTH_PERCENT
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            notifications_enabled: true,
            ai_provider: AiProvider::Ollama,
            ai_model: "qwen3:8b".to_string(),
            ai_url: default_ollama_url(),
            onboarding_completed: false,
            analysis_guidance: String::new(),
            translation_guidance: String::new(),
            eq_strength_percent: DEFAULT_EQ_STRENGTH_PERCENT,
            audio_analysis_enabled: true,
            lyrics_ai_enabled: true,
        }
    }
}

/// What the lyrics analysis is told to be, unless the user edits it.
pub const DEFAULT_ANALYSIS_GUIDANCE: &str = "You are annotating song lyrics like a Genius.com contributor. Explain what each part means or refers to in plain, specific language.";

/// What the lyrics translation is told to do, unless the user edits it.
pub const DEFAULT_TRANSLATION_GUIDANCE: &str = "You are translating song lyrics into natural, idiomatic English - not a stiff word-for-word gloss.";

/// The longest a guidance text may be (in characters): enough for a few
/// sentences of style, too short to bury the real instructions.
pub const MAX_GUIDANCE_CHARS: usize = 600;

impl AppSettings {
    /// The guidance to put in the analysis prompt: the user's, or the default.
    pub fn analysis_guidance_or_default(&self) -> &str {
        guidance_or_default(&self.analysis_guidance, DEFAULT_ANALYSIS_GUIDANCE)
    }

    pub fn translation_guidance_or_default(&self) -> &str {
        guidance_or_default(&self.translation_guidance, DEFAULT_TRANSLATION_GUIDANCE)
    }
}

fn guidance_or_default<'a>(custom: &'a str, default: &'a str) -> &'a str {
    if custom.trim().is_empty() { default } else { custom.trim() }
}

/// Tidies a guidance text before it is saved: trimmed, and cut to the
/// length limit.
pub fn clean_guidance(text: &str) -> String {
    text.trim().chars().take(MAX_GUIDANCE_CHARS).collect()
}

/// How much disk the app's own data takes.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DataSize {
    /// The database (library index and every saved analysis, translation,
    /// favorite and playlist).
    pub database_bytes: u64,
    /// Cover images saved next to it.
    pub covers_bytes: u64,
    /// Remembered answers from online lookups (part of the database's size).
    #[serde(default)]
    pub cache_entries: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub thumbnail_url: Option<String>,
}

/// A user-made, ordered list of tracks. Holds track ids only; a track whose
/// file is gone stays listed until removed but can't be shown or played.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub track_ids: Vec<String>,
}

/// One kind of the app's own data, which Settings can delete on its own (your music files are never touched).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum DataPart {
    /// The Library folders and every Track in the index.
    Music,
    /// The saved Audio analyses (loudness, spectrum, Suggested EQ).
    AudioAnalyses,
    /// The saved Lyrics analyses and Lyrics translations.
    LyricsData,
    /// Artist pictures.
    ArtistPictures,
    /// The saved, named Headphone corrections.
    HeadphoneCorrections,
    /// The Favorites and the Playlists.
    FavoritesAndPlaylists,
}

/// A saved headphone correction: the parametric EQ export as pasted, named for the earphone or
/// headphone it was made for.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeadphoneProfile {
    pub id: i64,
    pub name: String,
    pub preset: String,
}

/// A Track in the library: the fields a scan read from the file (the same
/// ones `TrackSummary` carries) plus `added_at`, when its row was first
/// added.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub thumbnail_url: Option<String>,
    pub added_at: String,
    /// Library fields read from the file's tags during a folder scan; all
    /// `None` for rows that didn't come from a scan.
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
}

/// Best-effort enrichment for a song, matched by title+artist against
/// MusicBrainz - `None` from a lookup means "no confident match", not an
/// error. `tags` is usually empty here - MusicBrainz users tag releases far
/// more often than individual recordings - but `first_release_date` is still
/// useful for a file whose own tags carry no year. `composers` covers
/// composer/lyricist/writer credits (often empty too - coverage is
/// inconsistent); producer/engineer/mixer credits aren't included since
/// they weren't reliably available from MusicBrainz even for well-documented
/// songs, confirmed by hands-on testing.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MusicBrainzTrackInfo {
    pub tags: Vec<String>,
    pub first_release_date: Option<String>,
    pub composers: Vec<String>,
}

/// Best-effort enrichment for a song, matched by title+artist against the
/// iTunes Search API - `None` means no result was credited to the same
/// artist, not an error. `genre` is a single curated Apple Music category
/// (unlike MusicBrainz's noisy community tags).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItunesTrackInfo {
    pub genre: Option<String>,
    pub release_date: Option<String>,
    pub artwork_url: Option<String>,
}

/// One resolution of a piece of iTunes artwork that was confirmed to
/// actually exist on the CDN (see `itunes::list_artwork_sizes`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkOption {
    pub size: u32,
    pub url: String,
}

/// Best-effort lyrics lookup against LRCLIB, matched by title+artist (and
/// duration, when known, to disambiguate between different recordings of
/// the same title). `None` from a lookup means no match at all; a match
/// with `instrumental: true` and `lyrics: None` means "found the track, it
/// genuinely has no lyrics" - distinct from "we just don't know."
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricsInfo {
    pub lyrics: Option<String>,
    pub instrumental: bool,
    /// Timed lines, when the service has them; empty means plain lyrics only.
    #[serde(default)]
    pub synced: Vec<SyncedLine>,
}

/// One line of timed lyrics: it starts at `ms` into the track.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncedLine {
    pub ms: i64,
    pub text: String,
}

/// One Genius-style annotation covering a contiguous span of lyrics lines
/// (1-indexed, inclusive) - `start_line == end_line` for a single-line
/// annotation.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricsAnnotation {
    pub start_line: i64,
    pub end_line: i64,
    pub meaning: String,
}

/// An AI deep-dive on a song's lyrics - overall vibe plus line-grouped
/// annotations. `lines` is the exact numbered line list the model was
/// prompted with and validated against, so the frontend renders line
/// numbers that are guaranteed to match what each annotation refers to.
/// `topics` and `entities` are the themes and named things the lyrics
/// mention. `title`, `artist`, and `thumbnail_url` are a snapshot taken at
/// analysis time, so the analysis can be listed on its own, independent of
/// whether the track is still in the library.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricsAnalysis {
    pub track_id: String,
    pub vibe_summary: String,
    pub annotations: Vec<LyricsAnnotation>,
    pub lines: Vec<String>,
    pub topics: Vec<String>,
    pub entities: Vec<String>,
    pub title: String,
    pub artist: String,
    pub thumbnail_url: Option<String>,
    pub model: String,
    pub created_at: String,
}

/// A per-line English translation of a song's lyrics, for hovering a
/// non-English line to see what it means - lighter-weight than
/// `LyricsAnalysis` (no vibe/topics/annotations, just a 1:1 translated
/// line list) since it's meant to run automatically for non-Latin-script
/// lyrics rather than requiring an explicit "deep dive" click. `lines` is
/// the same numbered split `ollama::numbered_lyrics_lines` produces for
/// `LyricsAnalysis`, so a translation lines up with the lyrics view by
/// index regardless of whether a deep dive has also been run.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricsTranslation {
    pub track_id: String,
    pub lines: Vec<String>,
    pub translations: Vec<String>,
    pub model: String,
    pub created_at: String,
}

/// One band of the suggested EQ - see `audio::eq::suggest` for how its
/// frequency/gain/Q are derived (adaptive to the track, not a fixed grid).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub hz: f32,
    pub measured_db: f32,
    pub suggested_gain_db: f32,
    pub q: f32,
}

/// Bass/mid/treble/overall loudness levels for a song, measured directly
/// from its audio (FFT band energy - see `audio::spectrum::measure`), not
/// inferred by an LLM. Meant as raw material for an EQ suggestion, e.g. a
/// song with a much higher `bass_db` than `treble_db` reads as bass-heavy.
/// The `_db` fields are relative to each other, not to any absolute
/// loudness standard - useful for comparing bands within one song, not for
/// comparing loudness across songs. `eq_bands`/`preamp_db` are ready-to-use
/// suggested settings for a real 10-band EQ.
/// `title`/`artist`/`thumbnail_url` are a snapshot taken at analysis time -
/// same reasoning as `LyricsAnalysis`'s - so a track shows up in the EQ
/// browser whether or not it's still in the library.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
    pub track_id: String,
    pub bass_db: f32,
    pub mid_db: f32,
    pub treble_db: f32,
    pub overall_db: f32,
    /// Integrated loudness in LUFS and true peak in dBTP (BS.1770, see `audio::loudness`). `None` for an
    /// analysis made before they were measured, which falls back to `overall_db`.
    #[serde(default)]
    pub loudness_lufs: Option<f32>,
    #[serde(default)]
    pub true_peak_db: Option<f32>,
    /// The measured spectrum behind the suggestion: one level per third-octave band (`audio::FINE_BAND_CENTERS_HZ`),
    /// `None` for a band with nothing usable (above Nyquist, or above a lossy file's cut-off). Empty for an
    /// analysis made before it was stored.
    #[serde(default)]
    pub spectrum_db: Vec<Option<f32>>,
    /// Where a lossy encoder cut the top of the spectrum off, in Hz, if it did.
    #[serde(default)]
    pub lowpass_hz: Option<f32>,
    /// Which version of the analysis made this (see `ANALYSIS_VERSION`); 1 for the first, spectrum-less one.
    #[serde(default)]
    pub analysis_version: u32,
    pub eq_bands: Vec<EqBand>,
    pub preamp_db: f32,
    /// Estimated tempo in beats per minute (see `audio::tempo::detect`) -
    /// `None` for tracks too short or too rhythmically flat to estimate,
    /// not a failed measurement.
    pub bpm: Option<f32>,
    /// Detected musical key in shorthand ("C", "Am", "F#m"), or `None` when
    /// detection wasn't confident enough - see `MIN_KEY_CONFIDENCE` in `audio::tempo`.
    pub key: Option<String>,
    pub title: String,
    pub artist: String,
    pub thumbnail_url: Option<String>,
    pub created_at: String,
}

/// An album whose tracks have no cover yet, with the tracks to attach one to.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MissingCover {
    pub artist: String,
    pub album: String,
    pub ids: Vec<String>,
}

#[cfg(test)]
mod ollama_url_tests {
    use super::clean_ollama_url as c;

    #[test]
    fn fills_in_what_a_person_leaves_out() {
        assert_eq!(c(""), "http://localhost:11434");
        assert_eq!(c("  192.168.1.20 "), "http://192.168.1.20:11434");
        assert_eq!(c("pc.local:11434/"), "http://pc.local:11434");
        assert_eq!(c("http://pc.local"), "http://pc.local:11434");
        assert_eq!(c("https://ollama.example.com:8443/"), "https://ollama.example.com:8443");
        assert_eq!(c("[::1]"), "http://[::1]:11434");
        assert_eq!(c("[::1]:9999"), "http://[::1]:9999");
        assert_eq!(c("host/path"), "http://host:11434/path");
    }
}

#[cfg(test)]
mod ai_url_tests {
    use super::{clean_ai_url, AiProvider};

    #[test]
    fn hosted_addresses_get_https_and_lose_the_trailing_slash() {
        assert_eq!(clean_ai_url(AiProvider::OpenAi, " api.groq.com/openai/v1/ "), "https://api.groq.com/openai/v1");
        assert_eq!(clean_ai_url(AiProvider::OpenAi, "http://localhost:1234/v1"), "http://localhost:1234/v1");
        assert_eq!(clean_ai_url(AiProvider::Anthropic, ""), "https://api.anthropic.com");
        assert_eq!(clean_ai_url(AiProvider::Ollama, "pc.local"), "http://pc.local:11434");
    }

    #[test]
    fn providers_have_distinct_ids_that_round_trip() {
        for p in AiProvider::ALL {
            assert_eq!(AiProvider::from_id(p.id()), Some(p));
        }
        assert_eq!(AiProvider::from_id("nope"), None);
    }
}
