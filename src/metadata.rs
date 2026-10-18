//! Metadata about a Track from the outside world: iTunes, MusicBrainz and
//! LRCLIB lookups, artwork download and album cover fetching. Every request
//! goes through one `Http` adapter (see `http.rs`), so the lookup logic -
//! variants, fallbacks, confidence cut-offs - is testable without a network.
//!
//! Answers are remembered for a while (see `Metadata::cached`) in the app's
//! database when one is attached, otherwise in memory. Images are not stored
//! here: callers get bytes back and decide where they go.

use crate::http::Http;
use crate::models::{
    ArtworkOption, ItunesTrackInfo, LyricsInfo, MusicBrainzTrackInfo,
};
use crate::{itunes, lrclib, musicbrainz};
use serde::{de::DeserializeOwned, Serialize};
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// iTunes' search API allows roughly 20 requests a minute, so cover lookups
/// (which run back to back over a whole library) are spaced out. Interactive
/// info lookups are not throttled.
const COVER_LOOKUP_GAP: Duration = Duration::from_secs(3);

const DAY: i64 = 86_400;

/// How long a lookup answer is trusted. What a lookup finds (a track's genre,
/// its lyrics) hardly ever changes, so a hit is kept long; a "found nothing"
/// is kept briefly, because a database that lacked a song today may have it
/// next week. Errors are never kept.
mod ttl {
    use super::DAY;
    pub const LYRICS_FOUND: i64 = 90 * DAY;
    pub const LYRICS_MISSING: i64 = 3 * DAY;
    pub const INFO_FOUND: i64 = 30 * DAY;
    pub const INFO_MISSING: i64 = 7 * DAY;
    pub const ARTWORK_SIZES: i64 = 30 * DAY;
}

/// Whether a result's credited artist is the one asked for: an exact,
/// case-insensitive name match. A free-text search can otherwise return a
/// tribute/cover act's recording ahead of the real one.
pub(crate) fn matches_artist(result_artist: &str, artist: &str) -> bool {
    result_artist.trim().eq_ignore_ascii_case(artist.trim())
}

/// A cache key for a title and artist, ignoring case and stray spacing.
fn lookup_key(title: &str, artist: &str) -> String {
    format!("{}|{}", title.trim().to_lowercase(), artist.trim().to_lowercase())
}

/// Lets one caller through per `gap`. Callers queue; the first after an idle
/// period goes straight away.
struct Gate {
    gap: Duration,
    next: tokio::sync::Mutex<Instant>,
}

impl Gate {
    fn new(gap: Duration) -> Self {
        Gate { gap, next: tokio::sync::Mutex::new(Instant::now()) }
    }

    async fn wait(&self) {
        let mut next = self.next.lock().await;
        tokio::time::sleep_until(*next).await;
        *next = Instant::now() + self.gap;
    }
}

pub struct Metadata<H: Http> {
    http: H,
    cover_gate: Gate,
    /// The app database, when attached, holds remembered answers across runs.
    cache_db: Option<crate::db::Db>,
    /// Remembered answers when there is no database (tests), as (json, expires_at).
    cache_mem: std::sync::Mutex<std::collections::HashMap<String, (String, i64)>>,
}

impl<H: Http> Metadata<H> {
    pub fn new(http: H) -> Self {
        Metadata { http, cover_gate: Gate::new(COVER_LOOKUP_GAP), cache_db: None, cache_mem: Default::default() }
    }

    /// Remembers answers in `db`, so they outlive this run.
    pub fn with_cache(mut self, db: crate::db::Db) -> Self {
        self.cache_db = Some(db);
        self
    }

    fn cache_read(&self, key: &str) -> Option<String> {
        match &self.cache_db {
            Some(db) => db.with(|c| crate::db::cache_get(c, key)).ok().flatten(),
            None => {
                let mem = self.cache_mem.lock().ok()?;
                mem.get(key).filter(|(_, exp)| *exp > chrono::Utc::now().timestamp()).map(|(v, _)| v.clone())
            }
        }
    }

    fn cache_write(&self, key: &str, value: &str, ttl: i64) {
        match &self.cache_db {
            Some(db) => {
                let _ = db.with(|c| crate::db::cache_set(c, key, value, ttl));
            }
            None => {
                if let Ok(mut mem) = self.cache_mem.lock() {
                    mem.insert(key.to_string(), (value.to_string(), chrono::Utc::now().timestamp() + ttl));
                }
            }
        }
    }

    /// `fetch`'s answer, or the remembered one if it is still fresh. `ttl_for`
    /// says how long to keep a new answer (it can depend on whether anything
    /// was found). An error is passed on and nothing is remembered.
    async fn cached<T, F>(&self, key: String, ttl_for: fn(&T) -> i64, fetch: F) -> Result<T, String>
    where
        T: Serialize + DeserializeOwned,
        F: Future<Output = Result<T, String>>,
    {
        if let Some(hit) = self.cache_read(&key).and_then(|json| serde_json::from_str::<T>(&json).ok()) {
            return Ok(hit);
        }
        let fresh = fetch.await?;
        if let Ok(json) = serde_json::to_string(&fresh) {
            self.cache_write(&key, &json, ttl_for(&fresh));
        }
        Ok(fresh)
    }

    #[cfg(test)]
    pub fn with_cover_gap(mut self, gap: Duration) -> Self {
        self.cover_gate = Gate::new(gap);
        self
    }

    /// `Ok(None)` from any lookup means no confident match, not a failure.
    pub async fn musicbrainz_track_info(&self, title: &str, artist: &str) -> Result<Option<MusicBrainzTrackInfo>, String> {
        self.cached(
            format!("musicbrainz:{}", lookup_key(title, artist)),
            |r: &Option<MusicBrainzTrackInfo>| if r.is_some() { ttl::INFO_FOUND } else { ttl::INFO_MISSING },
            musicbrainz::lookup_track(&self.http, title, artist),
        )
        .await
    }

    pub async fn itunes_track_info(&self, title: &str, artist: &str) -> Result<Option<ItunesTrackInfo>, String> {
        self.cached(
            format!("itunes:{}", lookup_key(title, artist)),
            |r: &Option<ItunesTrackInfo>| if r.is_some() { ttl::INFO_FOUND } else { ttl::INFO_MISSING },
            itunes::lookup_track(&self.http, title, artist),
        )
        .await
    }

    /// The resolutions actually available for a piece of iTunes artwork.
    pub async fn itunes_artwork_sizes(&self, artwork_url: &str) -> Result<Vec<ArtworkOption>, String> {
        self.cached(
            format!("artwork-sizes:{artwork_url}"),
            |_: &Vec<ArtworkOption>| ttl::ARTWORK_SIZES,
            itunes::list_artwork_sizes(&self.http, artwork_url),
        )
        .await
    }

    pub async fn lyrics(&self, title: &str, artist: &str, duration_ms: Option<i64>) -> Result<Option<LyricsInfo>, String> {
        self.cached(
            format!("lyrics:{}:{}", lookup_key(title, artist), duration_ms.unwrap_or(0) / 1000),
            |r: &Option<LyricsInfo>| if r.is_some() { ttl::LYRICS_FOUND } else { ttl::LYRICS_MISSING },
            lrclib::lookup_lyrics(&self.http, title, artist, duration_ms),
        )
        .await
    }

    /// The bytes behind an artwork URL.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self.http.get(url, &[]).await.map_err(|e| format!("couldn't reach the artwork CDN: {e}"))?;
        if !resp.is_success() {
            return Err(format!("the artwork CDN returned {}", resp.status));
        }
        Ok(resp.body)
    }

    /// An album's cover from iTunes. Waits its turn behind other cover
    /// fetches (see `COVER_LOOKUP_GAP`). `Ok(None)` when iTunes has no
    /// confident match or no artwork - not an error.
    /// An artist's picture from YouTube Music, waiting its turn like the
    /// cover fetches. `Ok(None)` when no artist of exactly that name comes up.
    pub async fn artist_picture(&self, artist: &str) -> Result<Option<Vec<u8>>, String> {
        self.cover_gate.wait().await;
        let Some(url) = crate::ytmusic::artist_picture_url(artist).await? else {
            return Ok(None);
        };
        self.download(&url).await.map(Some)
    }

    pub async fn album_cover(&self, artist: &str, album: &str) -> Result<Option<Vec<u8>>, String> {
        self.cover_gate.wait().await;
        let Some(url) = itunes::lookup_album_artwork(&self.http, album, artist).await? else {
            return Ok(None);
        };
        self.download(&url).await.map(Some)
    }
}

#[cfg(test)]
impl Metadata<crate::http::fake::FakeHttp> {
    /// Requests the fake network has seen, for tests in other modules.
    pub(crate) fn http_requests(&self) -> Vec<String> {
        self.http.requests()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use serde_json::json;

    fn metadata(http: FakeHttp) -> Metadata<FakeHttp> {
        Metadata::new(http).with_cover_gap(Duration::ZERO)
    }

    #[test]
    fn matches_artist_is_case_insensitive_and_trims_whitespace() {
        assert!(matches_artist("Queen", "queen"));
        assert!(matches_artist(" Queen ", "Queen"));
        assert!(!matches_artist("Queen Tribute Band", "Queen"));
    }

    // ---------- iTunes ----------

    fn itunes_results() -> serde_json::Value {
        json!({"results": [
            {"artistName": "Some Cover Band", "primaryGenreName": "Pop", "releaseDate": "2020-01-01T00:00:00Z"},
            {"artistName": "Queen", "primaryGenreName": "Rock", "releaseDate": "1975-10-31T12:00:00Z",
             "artworkUrl100": "https://cdn.example/a/100x100bb.jpg"}
        ]})
    }

    #[tokio::test]
    async fn itunes_picks_the_first_result_credited_to_the_artist_and_upsizes_the_artwork() {
        let http = FakeHttp::default().json("entity=song", itunes_results());
        let info = metadata(http).itunes_track_info("Bohemian Rhapsody", "queen").await.unwrap().unwrap();
        assert_eq!(info.genre.as_deref(), Some("Rock"));
        assert_eq!(info.release_date.as_deref(), Some("1975-10-31"));
        assert_eq!(info.artwork_url.as_deref(), Some("https://cdn.example/a/600x600bb.jpg"));
    }

    #[tokio::test]
    async fn itunes_has_no_answer_when_nothing_is_credited_to_the_artist() {
        let http = FakeHttp::default().json("itunes", itunes_results());
        assert!(metadata(http).itunes_track_info("Song", "Someone Else").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn itunes_errors_say_what_went_wrong() {
        let m = metadata(FakeHttp::default().status("itunes", 503));
        assert_eq!(m.itunes_track_info("a", "b").await.unwrap_err(), "iTunes returned 503 Service Unavailable");

        let m = metadata(FakeHttp::default().unreachable("itunes", "dns failure"));
        assert_eq!(m.itunes_track_info("a", "b").await.unwrap_err(), "couldn't reach iTunes: dns failure");

        let m = metadata(FakeHttp::default().bytes("itunes", b"<html>"));
        assert!(m.itunes_track_info("a", "b").await.unwrap_err().starts_with("unexpected response from iTunes"));
    }

    #[tokio::test]
    async fn artwork_sizes_stop_once_the_master_resolution_repeats() {
        let http = FakeHttp::default()
            .head("100x100bb", 200, 1_000)
            .head("200x200bb", 200, 5_000)
            .head("300x300bb", 200, 5_000);
        let sizes = metadata(http).itunes_artwork_sizes("https://cdn.example/a/100x100bb.jpg").await.unwrap();
        assert_eq!(sizes.iter().map(|s| s.size).collect::<Vec<_>>(), vec![100, 200]);
        assert_eq!(sizes[1].url, "https://cdn.example/a/200x200bb.jpg");
    }

    #[tokio::test]
    async fn artwork_sizes_stop_at_the_first_size_the_cdn_refuses() {
        let http = FakeHttp::default().head("100x100bb", 200, 1_000).status("200x200bb", 404);
        let sizes = metadata(http).itunes_artwork_sizes("https://cdn.example/a/100x100bb.jpg").await.unwrap();
        assert_eq!(sizes.len(), 1);
    }

    // ---------- LRCLIB ----------

    #[tokio::test]
    async fn lyrics_try_each_title_variant_before_giving_up_on_the_exact_endpoint() {
        let http = FakeHttp::default()
            .json("track_name=See that?&", json!({"instrumental": false, "plainLyrics": "la la"}));
        let m = metadata(http);
        let info = m.lyrics("별별별 (See that?)", "NMIXX", Some(180_000)).await.unwrap().unwrap();
        assert_eq!(info.lyrics.as_deref(), Some("la la"));

        let requests = m.http.requests();
        assert_eq!(requests.len(), 3, "full title, outer half, then inner half hits");
        assert!(requests[0].contains("track_name=별별별 (See that?)"));
        assert!(requests[1].contains("track_name=별별별&"));
        assert!(requests.iter().all(|r| r.contains("duration=180")), "duration is sent in seconds");
    }

    #[tokio::test]
    async fn lyrics_fall_back_to_search_and_only_trust_the_credited_artist() {
        let http = FakeHttp::default().json(
            "/search",
            json!([
                {"artistName": "Cover Band", "plainLyrics": "wrong"},
                {"artistName": "nmixx", "instrumental": false, "plainLyrics": "right"}
            ]),
        );
        let info = metadata(http).lyrics("Song", "NMIXX", None).await.unwrap().unwrap();
        assert_eq!(info.lyrics.as_deref(), Some("right"));
    }

    #[tokio::test]
    async fn lyrics_report_an_instrumental_and_a_server_error() {
        let http = FakeHttp::default().json("/get", json!({"instrumental": true, "plainLyrics": null}));
        let info = metadata(http).lyrics("Song", "Band", None).await.unwrap().unwrap();
        assert!(info.instrumental && info.lyrics.is_none());

        let http = FakeHttp::default().status("/get", 500);
        assert_eq!(metadata(http).lyrics("Song", "Band", None).await.unwrap_err(), "LRCLIB returned 500 Internal Server Error");
    }

    #[tokio::test]
    async fn lyrics_are_none_when_nothing_matches_anywhere() {
        // Every request 404s; the search fallback 404 is an error status, so use an empty list.
        let http = FakeHttp::default().json("/search", json!([]));
        assert!(metadata(http).lyrics("Song", "Band", None).await.unwrap().is_none());
    }

    // ---------- MusicBrainz ----------

    #[tokio::test]
    async fn musicbrainz_prefers_the_earliest_of_the_tied_best_matches_and_adds_composers() {
        let http = FakeHttp::default()
            .json(
                "/recording?query=recording:\"Bohemian Rhapsody\" AND artist:\"Queen\"",
                json!({"recordings": [
                    {"id": "later", "score": 100, "first-release-date": "1991-01-01"},
                    {"id": "orig", "score": 100, "first-release-date": "1975-10-31", "tags": [{"name": "rock", "count": 9}]},
                    {"id": "weaker", "score": 80, "first-release-date": "1970-01-01"}
                ]}),
            )
            .json("/recording/orig?fmt=json&inc=work-rels", json!({"relations": [{"work": {"id": "w1"}}]}))
            .json(
                "/work/w1",
                json!({"relations": [
                    {"type": "composer", "artist": {"name": "Freddie Mercury"}},
                    {"type": "producer", "artist": {"name": "Roy Thomas Baker"}},
                    {"type": "writer", "artist": {"name": "Freddie Mercury"}}
                ]}),
            );
        let info = metadata(http).musicbrainz_track_info("Bohemian Rhapsody", "Queen").await.unwrap().unwrap();
        assert_eq!(info.first_release_date.as_deref(), Some("1975-10-31"));
        assert_eq!(info.tags, vec!["rock".to_string()]);
        assert_eq!(info.composers, vec!["Freddie Mercury".to_string()]);
    }

    #[tokio::test]
    async fn musicbrainz_ignores_a_best_match_below_the_confidence_threshold() {
        let http = FakeHttp::default().json("/recording", json!({"recordings": [{"id": "x", "score": 85}]}));
        assert!(metadata(http).musicbrainz_track_info("Song", "Band").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn musicbrainz_composer_failure_still_returns_the_match() {
        let http = FakeHttp::default()
            .json("/recording?query", json!({"recordings": [{"id": "r", "score": 100, "first-release-date": "2000"}]}))
            .status("/recording/r", 503);
        let info = metadata(http).musicbrainz_track_info("Song", "Band").await.unwrap().unwrap();
        assert!(info.composers.is_empty());
    }

    #[tokio::test]
    async fn musicbrainz_errors_say_what_went_wrong() {
        let m = metadata(FakeHttp::default().status("musicbrainz", 503));
        assert_eq!(m.musicbrainz_track_info("a", "b").await.unwrap_err(), "MusicBrainz returned 503 Service Unavailable");
    }

    // ---------- covers and downloads ----------

    #[tokio::test]
    async fn an_album_cover_is_looked_up_then_downloaded() {
        let http = FakeHttp::default()
            .json("entity=album", itunes_results())
            .bytes("600x600bb.jpg", b"JPEGDATA");
        let m = metadata(http);
        assert_eq!(m.album_cover("Queen", "A Night at the Opera").await.unwrap(), Some(b"JPEGDATA".to_vec()));
        let request = &m.http.requests()[0];
        assert!(request.contains("term=A Night at the Opera Queen") && request.contains("entity=album"), "{request}");
    }

    #[tokio::test]
    async fn no_cover_when_itunes_has_no_match_or_no_artwork() {
        let m = metadata(FakeHttp::default().json("itunes", json!({"results": []})));
        assert_eq!(m.album_cover("Queen", "Unknown").await.unwrap(), None);
        assert_eq!(m.http.requests().len(), 1, "nothing to download");

        let no_art = json!({"results": [{"artistName": "Queen"}]});
        let m = metadata(FakeHttp::default().json("itunes", no_art));
        assert_eq!(m.album_cover("Queen", "x").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_failed_download_is_an_error_not_a_saved_error_page() {
        let m = metadata(FakeHttp::default().status("cdn.example", 403));
        assert_eq!(m.download("https://cdn.example/a.jpg").await.unwrap_err(), "the artwork CDN returned 403 Forbidden");

        let m = metadata(FakeHttp::default().unreachable("cdn.example", "timed out"));
        assert_eq!(m.download("https://cdn.example/a.jpg").await.unwrap_err(), "couldn't reach the artwork CDN: timed out");
    }

    #[tokio::test]
    async fn cover_fetches_wait_their_turn_but_info_lookups_do_not() {
        let gap = Duration::from_millis(80);
        let http = FakeHttp::default().json("itunes", itunes_results()).bytes("600x600bb", b"x");
        let m = Metadata::new(http).with_cover_gap(gap);

        let start = Instant::now();
        m.album_cover("Queen", "a").await.unwrap();
        let first = start.elapsed();
        m.album_cover("Queen", "b").await.unwrap();
        m.album_cover("Queen", "c").await.unwrap();
        let three = start.elapsed();

        assert!(first < gap, "the first fetch after idle goes straight away");
        assert!(three >= gap * 2, "later fetches are spaced out: {three:?}");

        let start = Instant::now();
        for _ in 0..3 {
            m.itunes_track_info("a", "Queen").await.unwrap();
        }
        assert!(start.elapsed() < gap, "info lookups are not throttled");
    }

    #[tokio::test]
    async fn a_transient_lrclib_failure_is_retried_instead_of_reported_as_no_lyrics() {
        // First reply is a 503; the fake serves one scripted answer per
        // route, so cover retry with an unreachable route that never
        // recovers: all attempts are made, then the error surfaces.
        let http = FakeHttp::default().status("lrclib.net/api/get", 503);
        let m = metadata(http);
        assert!(m.lyrics("Song", "Band", None).await.is_err());
        let gets = m.http_requests().iter().filter(|r| r.contains("/api/get")).count();
        assert_eq!(gets, 3);
    }

    #[tokio::test]
    async fn found_lyrics_are_cached_for_the_session() {
        let http = FakeHttp::default().json(
            "lrclib.net/api/get",
            serde_json::json!({"instrumental": false, "plainLyrics": "la la"}),
        );
        let m = metadata(http);
        assert!(m.lyrics("Song", "Band", None).await.unwrap().is_some());
        assert!(m.lyrics("song", "BAND", None).await.unwrap().is_some());
        assert_eq!(m.http_requests().len(), 1);
    }

    #[tokio::test]
    async fn answers_outlive_the_run_when_a_database_is_attached() {
        let db = crate::db::Db::in_memory().unwrap();
        let found = || FakeHttp::default().json("lrclib.net/api/get", serde_json::json!({"instrumental": false, "plainLyrics": "la la"}));

        let first = Metadata::new(found()).with_cache(db.clone());
        assert!(first.lyrics("Song", "Band", Some(180_000)).await.unwrap().is_some());
        assert_eq!(first.http_requests().len(), 1);

        // A new run (a fresh Metadata over the same database) asks nobody.
        let second = Metadata::new(found()).with_cache(db);
        assert!(second.lyrics("song", "BAND", Some(180_400)).await.unwrap().is_some());
        assert!(second.http_requests().is_empty());
    }

    #[tokio::test]
    async fn a_found_nothing_is_remembered_too_but_an_expired_answer_is_asked_again() {
        let db = crate::db::Db::in_memory().unwrap();
        let m = Metadata::new(FakeHttp::default().json("/search", serde_json::json!([]))).with_cache(db.clone());
        assert!(m.lyrics("Nope", "Nobody", None).await.unwrap().is_none());
        assert!(m.lyrics("Nope", "Nobody", None).await.unwrap().is_none());
        assert_eq!(m.http_requests().iter().filter(|r| r.contains("/api/get")).count(), 1);

        // Make it stale: the next lookup goes out again.
        db.with(|c| c.execute("UPDATE api_cache SET expires_at = 0", [])).unwrap();
        assert!(m.lyrics("Nope", "Nobody", None).await.unwrap().is_none());
        assert_eq!(m.http_requests().iter().filter(|r| r.contains("/api/get")).count(), 2);

        // Pruning drops what has expired.
        db.with(|c| c.execute("UPDATE api_cache SET expires_at = 0", [])).unwrap();
        assert_eq!(db.with(|c| crate::db::cache_prune(c)).unwrap(), 1);
    }
}
