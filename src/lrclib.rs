use crate::http::Http;
use crate::metadata::matches_artist;
use crate::models::{LyricsInfo, SyncedLine};
use serde::Deserialize;

const BASE_URL: &str = "https://lrclib.net/api";

/// LRCLIB is a free community service and regularly answers a first request
/// with a timeout, 429 or 5xx that succeeds moments later. Try this many
/// times before giving up, so a hiccup isn't mistaken for "no lyrics".
const ATTEMPTS: u32 = 3;
#[cfg(not(test))]
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(700);
#[cfg(test)]
const RETRY_DELAY: std::time::Duration = std::time::Duration::ZERO;

/// `http.get`, retried on a network error, 429 or 5xx (a 404 or any other
/// answer is final).
async fn get_retrying(http: &impl Http, url: &str, query: &[(&str, &str)]) -> Result<crate::http::Reply, crate::http::HttpError> {
    let mut attempt = 1;
    loop {
        let result = http.get(url, query).await;
        let transient = match &result {
            Err(_) => true,
            Ok(r) => r.status.is_server_error() || r.status == reqwest::StatusCode::TOO_MANY_REQUESTS,
        };
        if !transient || attempt >= ATTEMPTS {
            return result;
        }
        tokio::time::sleep(RETRY_DELAY * attempt).await;
        attempt += 1;
    }
}

#[derive(Debug, Deserialize)]
struct LrcLibResponse {
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "plainLyrics", default)]
    plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics", default)]
    synced_lyrics: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LrcLibSearchResult {
    #[serde(rename = "artistName", default)]
    artist_name: String,
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "plainLyrics", default)]
    plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics", default)]
    synced_lyrics: Option<String>,
}

/// Timed lyrics in LRC form: `[mm:ss.xx] text`, a line possibly carrying
/// several stamps (a repeated chorus). Lines come back sorted by time; empty
/// ones (instrumental gaps) are kept so the display can go blank there.
pub(crate) fn parse_lrc(lrc: &str) -> Vec<SyncedLine> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(inner) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let (stamp, after) = inner;
            let Some((m, s)) = stamp.split_once(':') else { break };
            let (Ok(m), Ok(s)) = (m.parse::<i64>(), s.parse::<f64>()) else { break };
            stamps.push(m * 60_000 + (s * 1000.0).round() as i64);
            rest = after.trim_start();
        }
        for ms in stamps {
            lines.push(SyncedLine { ms, text: rest.trim().to_string() });
        }
    }
    lines.sort_by_key(|l| l.ms);
    lines
}

/// A bilingual title is often written as "Original (English)" (e.g.
/// "별별별 (See that?)"), but LRCLIB is usually only keyed on one half - an
/// exact-match lookup against the full bilingual string finds nothing even
/// though the song is there under a shorter title. Returns candidate titles
/// to try in order: the title as given, then the part outside the trailing
/// parenthetical, then the part inside it.
fn title_variants(title: &str) -> Vec<String> {
    let mut variants = vec![title.to_string()];
    if title.ends_with(')') {
        if let Some(open) = title.rfind('(') {
            let outer = title[..open].trim();
            let inner = title[open + 1..title.len() - 1].trim();
            if !outer.is_empty() {
                variants.push(outer.to_string());
            }
            if !inner.is_empty() {
                variants.push(inner.to_string());
            }
        }
    }
    variants
}

/// Looks up lyrics by title+artist (and duration, when known, to pick the
/// right one among differently-timed recordings sharing a title - LRCLIB
/// matches with a few seconds of tolerance). Tries a few title variants
/// against the exact-match endpoint first (see `title_variants`), then
/// falls back to LRCLIB's fuzzy `/search` endpoint. `Ok(None)` means no
/// matching track was found at all, not a failure.
pub(crate) async fn lookup_lyrics(
    http: &impl Http,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
) -> Result<Option<LyricsInfo>, String> {
    for variant in title_variants(title) {
        if let Some(info) = get_exact(http, &variant, artist, duration_ms).await? {
            return Ok(Some(info));
        }
    }

    search_fallback(http, title, artist).await
}

async fn get_exact(
    http: &impl Http,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
) -> Result<Option<LyricsInfo>, String> {
    let mut query = vec![("track_name", title), ("artist_name", artist)];
    let seconds = duration_ms.map(|ms| (ms / 1000).to_string());
    if let Some(seconds) = &seconds {
        query.push(("duration", seconds));
    }

    let resp = get_retrying(http, &format!("{BASE_URL}/get"), &query)
        .await
        .map_err(|e| format!("couldn't reach LRCLIB: {e}"))?;

    if resp.status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.is_success() {
        return Err(format!("LRCLIB returned {}", resp.status));
    }

    let body: LrcLibResponse = resp.json().map_err(|e| format!("unexpected response from LRCLIB: {e}"))?;

    Ok(Some(LyricsInfo {
        synced: body.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default(),
        lyrics: body.plain_lyrics,
        instrumental: body.instrumental,
    }))
}

/// Last resort when no exact-match title variant hits - LRCLIB's fuzzy
/// `/search` endpoint, filtered down to a result actually credited to the
/// same artist (a bare title search can otherwise surface an unrelated
/// cover ahead of the real recording).
async fn search_fallback(http: &impl Http, title: &str, artist: &str) -> Result<Option<LyricsInfo>, String> {
    let resp = get_retrying(http, &format!("{BASE_URL}/search"), &[("track_name", title), ("artist_name", artist)])
        .await
        .map_err(|e| format!("couldn't reach LRCLIB: {e}"))?;

    if !resp.is_success() {
        return Err(format!("LRCLIB returned {}", resp.status));
    }

    let results: Vec<LrcLibSearchResult> =
        resp.json().map_err(|e| format!("unexpected response from LRCLIB: {e}"))?;

    let Some(best) = results.into_iter().find(|r| matches_artist(&r.artist_name, artist)) else {
        return Ok(None);
    };

    Ok(Some(LyricsInfo {
        synced: best.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default(),
        lyrics: best.plain_lyrics,
        instrumental: best.instrumental,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_variants_returns_just_the_title_when_theres_no_trailing_parenthetical() {
        assert_eq!(title_variants("Bohemian Rhapsody"), vec!["Bohemian Rhapsody".to_string()]);
    }

    #[test]
    fn title_variants_splits_a_bilingual_title_into_both_halves() {
        assert_eq!(
            title_variants("별별별 (See that?)"),
            vec!["별별별 (See that?)".to_string(), "별별별".to_string(), "See that?".to_string()]
        );
    }

    #[test]
    fn title_variants_ignores_a_parenthetical_that_isnt_at_the_end() {
        assert_eq!(title_variants("Song (feat. Someone) [Remix]"), vec!["Song (feat. Someone) [Remix]".to_string()]);
    }

    #[test]
    fn parses_timed_lyrics_including_repeated_stamps_and_gaps() {
        let lrc = "[00:12.50] First line\n[01:02.00][02:00.250] Chorus\n[00:30.00]\nnot a lyric line";
        let lines = parse_lrc(lrc);
        let got: Vec<(i64, &str)> = lines.iter().map(|l| (l.ms, l.text.as_str())).collect();
        assert_eq!(got, vec![(12_500, "First line"), (30_000, ""), (62_000, "Chorus"), (120_250, "Chorus")]);
    }

    #[test]
    fn parses_a_lyrics_response() {
        let body: LrcLibResponse = serde_json::from_value(serde_json::json!({
            "instrumental": false,
            "plainLyrics": "Is this the real life?\nIs this just fantasy?",
            "syncedLyrics": "[00:00.15] Is this the real life?"
        }))
        .unwrap();

        assert!(!body.instrumental);
        assert_eq!(body.plain_lyrics, Some("Is this the real life?\nIs this just fantasy?".to_string()));
    }

    #[test]
    fn parses_an_instrumental_response_with_no_lyrics() {
        let body: LrcLibResponse = serde_json::from_value(serde_json::json!({
            "instrumental": true,
            "plainLyrics": null
        }))
        .unwrap();

        assert!(body.instrumental);
        assert_eq!(body.plain_lyrics, None);
    }

    #[test]
    fn parses_a_search_response() {
        let results: Vec<LrcLibSearchResult> = serde_json::from_value(serde_json::json!([
            {"artistName": "Cover Band", "instrumental": false, "plainLyrics": "wrong song"},
            {"artistName": "NMIXX", "instrumental": false, "plainLyrics": "right song"}
        ]))
        .unwrap();

        let best = results.into_iter().find(|r| matches_artist(&r.artist_name, "NMIXX"));
        assert_eq!(best.unwrap().plain_lyrics, Some("right song".to_string()));
    }

    /// Hits real LRCLIB. Not run by default - `cargo test -- --ignored`.
    /// Regression test: this title is exactly the shape that used to return
    /// no lyrics - a bilingual "Original (English)" title where LRCLIB only
    /// has the track under the English half.
    #[tokio::test]
    #[ignore]
    async fn looks_up_lyrics_for_a_bilingual_title_via_real_lrclib() {
        let info = lookup_lyrics(&crate::http::ReqwestHttp, "별별별 (See that?)", "NMIXX", None)
            .await
            .expect("lookup should succeed")
            .expect("should find a match via a title variant or search fallback");

        assert!(!info.instrumental);
        assert!(info.lyrics.is_some());
    }

    /// Hits real LRCLIB. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn looks_up_real_lyrics_via_real_lrclib() {
        let info = lookup_lyrics(&crate::http::ReqwestHttp, "Bohemian Rhapsody", "Queen", Some(355_000))
            .await
            .expect("lookup should succeed")
            .expect("should find a match");

        assert!(!info.instrumental);
        let lyrics = info.lyrics.expect("should have lyrics");
        assert!(lyrics.contains("Is this the real life"));
    }

    /// Hits real LRCLIB. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn returns_none_for_a_nonexistent_track() {
        let info = lookup_lyrics(&crate::http::ReqwestHttp, "Totally Fake Song Xyz123", "Nonexistent Artist Abc", None)
            .await
            .expect("lookup should succeed, just with no match");
        assert!(info.is_none());
    }
}
