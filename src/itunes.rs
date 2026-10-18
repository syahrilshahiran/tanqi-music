use crate::http::Http;
use crate::metadata::matches_artist;
use crate::models::{ArtworkOption, ItunesTrackInfo};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

const BASE_URL: &str = "https://itunes.apple.com/search";

/// Matches the "NxNbb" size token in an iTunes/mzstatic artwork URL, e.g.
/// the "100x100bb" in ".../100x100bb.jpg".
static ARTWORK_SIZE_TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d+x\d+bb").unwrap());

/// Sizes worth probing when listing what resolutions are actually
/// available - the CDN accepts any size in the URL, but silently caps out
/// at the artwork's real master resolution, so this is just a reasonable
/// ladder to sample along.
const ARTWORK_SIZE_CANDIDATES: [u32; 12] = [100, 200, 300, 500, 600, 800, 1000, 1200, 1500, 2000, 2500, 3000];

/// iTunes dates come back as full timestamps (e.g. "1975-10-31T12:00:00Z") -
/// trimmed to just the date for consistency with the other sources' format.
fn date_only(release_date: Option<String>) -> Option<String> {
    release_date.map(|d| d.chars().take(10).collect())
}

/// Swaps the size token in an artwork URL for a different one - the same
/// mzstatic CDN path serves any size, not just the "100x100bb" iTunes hands
/// back directly, up to the artwork's real master resolution.
fn resize_artwork(url: &str, size: u32) -> String {
    ARTWORK_SIZE_TOKEN.replace(url, format!("{size}x{size}bb")).to_string()
}

/// iTunes only ever gives a 100x100 artwork URL directly - upsized here to
/// "600x600bb" for a much sharper default image without an extra request.
fn upsize_artwork(artwork_url_100: Option<String>) -> Option<String> {
    artwork_url_100.map(|url| resize_artwork(&url, 600))
}

/// Probes one candidate size via HEAD, returning its resolved URL and
/// `Content-Length` if the CDN served it successfully.
async fn probe_artwork_size(http: &impl Http, base_url: &str, size: u32) -> Option<(String, Option<u64>)> {
    let url = resize_artwork(base_url, size);
    let resp = http.head(&url).await.ok()?;
    if !resp.is_success() {
        return None;
    }
    Some((url, resp.content_length))
}

/// Lists the distinct resolutions actually available for a piece of
/// artwork, probing a ladder of candidate sizes against the CDN. Once a
/// probed size comes back with the same `Content-Length` as the previous
/// one, the artwork's master resolution has been reached (the CDN just
/// keeps serving that same image for anything larger), so probing stops
/// there rather than listing duplicate entries.
pub(crate) async fn list_artwork_sizes(http: &impl Http, artwork_url: &str) -> Result<Vec<ArtworkOption>, String> {
    let mut options = Vec::new();
    let mut prev_len: Option<u64> = None;

    for &size in ARTWORK_SIZE_CANDIDATES.iter() {
        let Some((url, len)) = probe_artwork_size(http, artwork_url, size).await else {
            break;
        };
        if len.is_some() && len == prev_len {
            break;
        }
        prev_len = len.or(prev_len);
        options.push(ArtworkOption { size, url });
    }

    Ok(options)
}

#[derive(Debug, Deserialize)]
struct TrackSearchResponse {
    #[serde(default)]
    results: Vec<TrackResult>,
}

#[derive(Debug, Deserialize)]
struct TrackResult {
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "primaryGenreName", default)]
    primary_genre_name: Option<String>,
    #[serde(rename = "releaseDate", default)]
    release_date: Option<String>,
    #[serde(rename = "artworkUrl100", default)]
    artwork_url_100: Option<String>,
}

/// Looks up a song by title+artist against the iTunes Search API - no API
/// key needed. `Ok(None)` means nothing credited to the same artist came
/// back, not a failure.
pub(crate) async fn lookup_track(http: &impl Http, title: &str, artist: &str) -> Result<Option<ItunesTrackInfo>, String> {
    let term = format!("{title} {artist}");
    let resp = http
        .get(BASE_URL, &[("term", &term), ("media", "music"), ("entity", "song"), ("limit", "5")])
        .await
        .map_err(|e| format!("couldn't reach iTunes: {e}"))?;

    if !resp.is_success() {
        return Err(format!("iTunes returned {}", resp.status));
    }

    let body: TrackSearchResponse = resp.json().map_err(|e| format!("unexpected response from iTunes: {e}"))?;

    let Some(best) = body.results.into_iter().find(|r| matches_artist(&r.artist_name, artist)) else {
        return Ok(None);
    };

    Ok(Some(ItunesTrackInfo {
        genre: best.primary_genre_name,
        release_date: date_only(best.release_date),
        artwork_url: upsize_artwork(best.artwork_url_100),
    }))
}

#[derive(Debug, Deserialize)]
struct AlbumSearchResponse {
    #[serde(default)]
    results: Vec<AlbumResult>,
}

#[derive(Debug, Deserialize)]
struct AlbumResult {
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(rename = "artworkUrl100", default)]
    artwork_url_100: Option<String>,
}

/// The cover art URL (upsized) of an album, looked up by title+artist
/// against the iTunes Search API. `Ok(None)` means nothing credited to the
/// same artist came back, or its result has no artwork.
pub(crate) async fn lookup_album_artwork(http: &impl Http, title: &str, artist: &str) -> Result<Option<String>, String> {
    let term = format!("{title} {artist}");
    let resp = http
        .get(BASE_URL, &[("term", &term), ("media", "music"), ("entity", "album"), ("limit", "5")])
        .await
        .map_err(|e| format!("couldn't reach iTunes: {e}"))?;

    if !resp.is_success() {
        return Err(format!("iTunes returned {}", resp.status));
    }

    let body: AlbumSearchResponse = resp.json().map_err(|e| format!("unexpected response from iTunes: {e}"))?;

    Ok(body
        .results
        .into_iter()
        .find(|r| matches_artist(&r.artist_name, artist))
        .and_then(|best| upsize_artwork(best.artwork_url_100)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_only_trims_a_full_timestamp_to_the_date() {
        assert_eq!(date_only(Some("1975-10-31T12:00:00Z".to_string())), Some("1975-10-31".to_string()));
        assert_eq!(date_only(None), None);
    }

    #[test]
    fn picks_the_first_result_credited_to_the_requested_artist() {
        let results = vec![
            TrackResult {
                artist_name: "Some Cover Band".to_string(),
                primary_genre_name: Some("Rock".to_string()),
                release_date: Some("2020-01-01T00:00:00Z".to_string()),
                artwork_url_100: None,
            },
            TrackResult {
                artist_name: "Queen".to_string(),
                primary_genre_name: Some("Rock".to_string()),
                release_date: Some("1975-10-31T12:00:00Z".to_string()),
                artwork_url_100: None,
            },
        ];
        let best = results.into_iter().find(|r| matches_artist(&r.artist_name, "Queen"));
        assert!(best.is_some());
        assert_eq!(best.unwrap().release_date, Some("1975-10-31T12:00:00Z".to_string()));
    }

    /// Hits real iTunes. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn looks_up_a_real_track_via_real_itunes() {
        let info = lookup_track(&crate::http::ReqwestHttp, "Bohemian Rhapsody", "Queen")
            .await
            .expect("lookup should succeed")
            .expect("should find a confident match");

        assert_eq!(info.genre, Some("Rock".to_string()));
        assert!(info.release_date.is_some());
        println!("{info:?}");
    }

    /// Hits real iTunes. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn looks_up_a_real_album_via_real_itunes() {
        let artwork = lookup_album_artwork(&crate::http::ReqwestHttp, "A Night at the Opera", "Queen")
            .await
            .expect("lookup should succeed")
            .expect("should find a confident match");

        assert!(artwork.contains("600x600bb"), "{artwork}");
    }

    /// Hits real iTunes. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn returns_none_for_a_nonexistent_track() {
        let info = lookup_track(&crate::http::ReqwestHttp, "Totally Fake Song Xyz123", "Nonexistent Artist Abc")
            .await
            .expect("lookup should succeed, just with no match");
        assert!(info.is_none());
    }
}
