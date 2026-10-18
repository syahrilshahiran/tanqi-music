use crate::http::Http;
use crate::models::MusicBrainzTrackInfo;
use serde::Deserialize;

const BASE_URL: &str = "https://musicbrainz.org/ws/2";
// A gap was observed live between exact matches (100) and clearly-wrong
// same-ish-titled results (85-91) - anything below this is treated as "no
// confident match" rather than risking showing the wrong album/song's info.
const MIN_MATCH_SCORE: i64 = 90;
const MAX_TAGS: usize = 6;

#[derive(Debug, Deserialize)]
struct TagEntry {
    name: String,
    #[serde(default)]
    count: i64,
}

/// Sorted by how many users applied each tag, most first, capped since a
/// well-tagged release can have dozens.
fn top_tags(mut tags: Vec<TagEntry>) -> Vec<String> {
    tags.sort_by(|a, b| b.count.cmp(&a.count));
    tags.into_iter().take(MAX_TAGS).map(|t| t.name).collect()
}

/// A plain `"{title} {artist}"` query matches on *any* indexed field, which
/// lets a cover/tribute recording whose title literally contains the
/// artist's name (e.g. "Bohemian Rhapsody (Queen)" by an unrelated tribute
/// act) outscore the genuine artist's own recording - confirmed live. Lucene
/// field-scoping (`{field}:"..." AND artist:"..."`) restricts the artist
/// term to the actual artist-credit, fixing that false-positive class.
fn field_scoped_query(field: &str, title: &str, artist: &str) -> String {
    format!("{field}:\"{}\" AND artist:\"{}\"", escape_lucene_phrase(title), escape_lucene_phrase(artist))
}

fn escape_lucene_phrase(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Among results tied at the best score, MusicBrainz's ordering otherwise
/// isn't meaningful for "which one is the original" - a title as
/// well-covered as "Bohemian Rhapsody" can return dozens of live/remaster/
/// re-release recordings all scoring 100. Preferring the earliest dated one
/// among the tie is the closest available proxy for "the original release."
fn best_by_earliest_date<T>(items: Vec<T>, score: impl Fn(&T) -> i64, date: impl Fn(&T) -> &Option<String>) -> Option<T> {
    let top_score = items.iter().map(&score).max()?;
    items
        .into_iter()
        .filter(|item| score(item) == top_score)
        .min_by_key(|item| date(item).clone().unwrap_or_else(|| "9999".to_string()))
}

#[derive(Debug, Deserialize)]
struct RecordingSearchResponse {
    #[serde(default)]
    recordings: Vec<RecordingResult>,
}

#[derive(Debug, Deserialize)]
struct RecordingResult {
    #[serde(default)]
    id: String,
    #[serde(default)]
    score: i64,
    #[serde(rename = "first-release-date", default)]
    first_release_date: Option<String>,
    #[serde(default)]
    tags: Vec<TagEntry>,
}

/// Looks up a song by title+artist against MusicBrainz's recording search.
/// `Ok(None)` means no confidently-matching recording was found. `tags` is
/// usually empty even on a good match - see `MusicBrainzTrackInfo`'s doc
/// comment - that's expected, not a parsing bug.
pub(crate) async fn lookup_track(http: &impl Http, title: &str, artist: &str) -> Result<Option<MusicBrainzTrackInfo>, String> {
    let query = field_scoped_query("recording", title, artist);
    let resp = http
        .get(&format!("{BASE_URL}/recording"), &[("query", &query), ("fmt", "json"), ("limit", "10")])
        .await
        .map_err(|e| format!("couldn't reach MusicBrainz: {e}"))?;

    if !resp.is_success() {
        return Err(format!("MusicBrainz returned {}", resp.status));
    }

    let body: RecordingSearchResponse = resp
        .json()
        .map_err(|e| format!("unexpected response from MusicBrainz: {e}"))?;

    let Some(best) = best_by_earliest_date(body.recordings, |r| r.score, |r| &r.first_release_date) else {
        return Ok(None);
    };
    if best.score < MIN_MATCH_SCORE {
        return Ok(None);
    }

    // Best-effort on top of best-effort: composer/lyricist credits live on
    // the recording's linked "work" entity, not the recording itself, so
    // this is a separate two-hop lookup (recording -> work -> its credits).
    // Any failure along the way just yields no composers, same as an
    // absent match elsewhere in this module - never fails the whole call.
    let composers = lookup_composers(http, &best.id).await;

    Ok(Some(MusicBrainzTrackInfo {
        tags: top_tags(best.tags),
        first_release_date: best.first_release_date,
        composers,
    }))
}

#[derive(Debug, Deserialize)]
struct RecordingRelationsResponse {
    #[serde(default)]
    relations: Vec<RecordingRelation>,
}

#[derive(Debug, Deserialize)]
struct RecordingRelation {
    work: Option<WorkRef>,
}

#[derive(Debug, Deserialize)]
struct WorkRef {
    id: String,
}

#[derive(Debug, Deserialize)]
struct WorkRelationsResponse {
    #[serde(default)]
    relations: Vec<WorkRelation>,
}

#[derive(Debug, Deserialize)]
struct WorkRelation {
    #[serde(rename = "type", default)]
    rel_type: String,
    artist: Option<ArtistRef>,
}

#[derive(Debug, Deserialize)]
struct ArtistRef {
    name: String,
}

const SONGWRITING_RELATION_TYPES: &[&str] = &["composer", "lyricist", "writer"];

/// Composer/lyricist/writer credits for a recording, via its linked work -
/// confirmed live that this is where MusicBrainz actually stores them
/// (recording-level relations were empty even for well-documented hits).
/// Returns an empty list rather than an error on any failure - this is
/// enrichment on top of enrichment, never worth failing the caller over.
async fn lookup_composers(http: &impl Http, recording_mbid: &str) -> Vec<String> {
    async {
        let resp = http
            .get(&format!("{BASE_URL}/recording/{recording_mbid}"), &[("fmt", "json"), ("inc", "work-rels")])
            .await
            .ok()?;
        if !resp.is_success() {
            return None;
        }
        let body: RecordingRelationsResponse = resp.json().ok()?;
        let work_id = body.relations.into_iter().find_map(|r| r.work.map(|w| w.id))?;

        let resp = http
            .get(&format!("{BASE_URL}/work/{work_id}"), &[("fmt", "json"), ("inc", "artist-rels")])
            .await
            .ok()?;
        if !resp.is_success() {
            return None;
        }
        let work: WorkRelationsResponse = resp.json().ok()?;

        let mut seen = std::collections::HashSet::new();
        let names: Vec<String> = work
            .relations
            .into_iter()
            .filter(|r| SONGWRITING_RELATION_TYPES.contains(&r.rel_type.as_str()))
            .filter_map(|r| r.artist.map(|a| a.name))
            .filter(|name| seen.insert(name.clone()))
            .collect();
        Some(names)
    }
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, count: i64) -> TagEntry {
        TagEntry { name: name.to_string(), count }
    }

    #[test]
    fn field_scopes_the_query_to_the_actual_artist_credit() {
        let query = field_scoped_query("recording", "Bohemian Rhapsody", "Queen");
        assert_eq!(query, "recording:\"Bohemian Rhapsody\" AND artist:\"Queen\"");
    }

    #[test]
    fn escapes_quotes_and_backslashes_in_titles() {
        let query = field_scoped_query("recording", "She Said \"Hi\"", "Some\\Artist");
        assert_eq!(query, "recording:\"She Said \\\"Hi\\\"\" AND artist:\"Some\\\\Artist\"");
    }

    #[test]
    fn parentheses_pass_through_unescaped() {
        // Confirmed live: a quoted Lucene phrase treats parentheses as
        // literal characters, no escaping needed.
        let query = field_scoped_query("recording", "Bohemian Rhapsody (Live Aid)", "Queen");
        assert_eq!(query, "recording:\"Bohemian Rhapsody (Live Aid)\" AND artist:\"Queen\"");
    }

    fn recording(score: i64, first_release_date: Option<&str>) -> RecordingResult {
        RecordingResult {
            id: "id".to_string(),
            score,
            first_release_date: first_release_date.map(|s| s.to_string()),
            tags: vec![],
        }
    }

    #[test]
    fn picks_the_earliest_date_among_tied_top_scores() {
        let items = vec![
            recording(100, Some("1993-01-01")),
            recording(100, Some("1979-12-26")),
            recording(85, Some("1975-10-31")),
        ];
        let best = best_by_earliest_date(items, |r| r.score, |r| &r.first_release_date).unwrap();
        // The 1975 date belongs to a lower-scored (less confident) match -
        // tie-breaking only applies among the *best* score, never trading
        // away confidence for an earlier-looking date.
        assert_eq!(best.first_release_date, Some("1979-12-26".to_string()));
    }

    #[test]
    fn falls_back_to_any_tied_result_when_all_dates_are_missing() {
        let items = vec![recording(100, None), recording(100, None)];
        let best = best_by_earliest_date(items, |r| r.score, |r| &r.first_release_date);
        assert!(best.is_some());
    }

    #[test]
    fn returns_none_for_an_empty_result_set() {
        let items: Vec<RecordingResult> = vec![];
        assert!(best_by_earliest_date(items, |r| r.score, |r| &r.first_release_date).is_none());
    }

    #[test]
    fn parses_work_relations_filtering_to_songwriting_types_and_deduping() {
        let work: WorkRelationsResponse = serde_json::from_value(serde_json::json!({
            "relations": [
                {"type": "composer", "artist": {"name": "Freddie Mercury"}},
                {"type": "lyricist", "artist": {"name": "Freddie Mercury"}},
                {"type": "producer", "artist": {"name": "Someone Else"}},
                {"type": "performance", "artist": {"name": "A Performer"}}
            ]
        }))
        .unwrap();

        let mut seen = std::collections::HashSet::new();
        let names: Vec<String> = work
            .relations
            .into_iter()
            .filter(|r| SONGWRITING_RELATION_TYPES.contains(&r.rel_type.as_str()))
            .filter_map(|r| r.artist.map(|a| a.name))
            .filter(|name| seen.insert(name.clone()))
            .collect();

        // "producer" and "performance" relations are excluded, and the
        // duplicate "Freddie Mercury" (composer + lyricist) collapses to one.
        assert_eq!(names, vec!["Freddie Mercury".to_string()]);
    }

    #[test]
    fn sorts_tags_by_count_descending_and_caps_at_six() {
        let tags = vec![
            tag("electronic", 0),
            tag("rock", 14),
            tag("pop", 3),
            tag("progressive rock", 4),
            tag("hard rock", 2),
            tag("glam rock", 1),
            tag("art rock", 1),
            tag("baroque pop", 1),
        ];
        let top = top_tags(tags);
        assert_eq!(top.len(), 6);
        assert_eq!(top[0], "rock");
        assert_eq!(top[1], "progressive rock");
        assert_eq!(top[2], "pop");
    }

    #[test]
    fn treats_score_below_threshold_as_no_match() {
        assert!(89 < MIN_MATCH_SCORE);
        assert!(90 >= MIN_MATCH_SCORE);
    }

    /// Hits real MusicBrainz. Not run by default - `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn looks_up_a_real_track_via_real_musicbrainz() {
        let info = lookup_track(&crate::http::ReqwestHttp, "Bohemian Rhapsody", "Queen")
            .await
            .expect("lookup should succeed")
            .expect("should find a confident match");

        assert!(info.first_release_date.is_some());
        assert!(info.composers.contains(&"Freddie Mercury".to_string()));
        println!("{info:?}");
    }
}
