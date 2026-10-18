//! Lyrics analyses and translations.

use super::*;

pub(super) fn row_to_lyrics_analysis(row: &rusqlite::Row) -> rusqlite::Result<LyricsAnalysis> {
    let annotations: String = row.get(2)?;
    let lines: String = row.get(3)?;
    let topics: String = row.get(6)?;
    let entities: String = row.get(7)?;
    Ok(LyricsAnalysis {
        track_id: row.get(0)?,
        vibe_summary: row.get(1)?,
        annotations: serde_json::from_str(&annotations).unwrap_or_default(),
        lines: serde_json::from_str(&lines).unwrap_or_default(),
        topics: serde_json::from_str(&topics).unwrap_or_default(),
        entities: serde_json::from_str(&entities).unwrap_or_default(),
        title: row.get(8)?,
        artist: row.get(9)?,
        thumbnail_url: row.get(10)?,
        model: row.get(4)?,
        created_at: row.get(5)?,
    })
}

/// `title`/`artist`/`thumbnail_url` are backfilled from `tracks` when
/// the analysis's own snapshot is blank - analyses cached before those
/// fields existed (or ones added via a migration) would otherwise render
/// as an empty, unclickable row wherever they're listed.
const LYRICS_ANALYSIS_SELECT: &str = "
    SELECT la.track_id, la.vibe_summary, la.annotations, la.lines, la.model, la.created_at,
           la.topics, la.entities,
           COALESCE(NULLIF(s.title, ''), NULLIF(la.title, ''), '') AS title,
           COALESCE(NULLIF(s.artist, ''), NULLIF(la.artist, ''), '') AS artist,
           COALESCE(s.thumbnail_url, la.thumbnail_url) AS thumbnail_url
    FROM lyrics_analysis la
    LEFT JOIN tracks s ON s.id = la.track_id";

pub fn get_lyrics_analysis(conn: &Connection, track_id: &str) -> rusqlite::Result<Option<LyricsAnalysis>> {
    conn.query_row(
        &format!("{LYRICS_ANALYSIS_SELECT} WHERE la.track_id = ?1"),
        params![track_id],
        row_to_lyrics_analysis,
    )
    .optional()
}

/// Every saved Lyrics analysis, newest first. One with no title (its track
/// left the library and it never snapshotted one) is dropped rather than
/// shown as a blank card.
pub fn list_analyzed_lyrics(conn: &Connection) -> rusqlite::Result<Vec<LyricsAnalysis>> {
    let mut stmt = conn.prepare(&format!("{LYRICS_ANALYSIS_SELECT} ORDER BY la.created_at DESC"))?;
    let all = stmt.query_map([], row_to_lyrics_analysis)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(all.into_iter().filter(|a| !a.title.is_empty()).collect())
}

pub fn save_lyrics_analysis(conn: &Connection, analysis: &LyricsAnalysis) -> rusqlite::Result<()> {
    let annotations = serde_json::to_string(&analysis.annotations).unwrap_or_else(|_| "[]".to_string());
    let lines = serde_json::to_string(&analysis.lines).unwrap_or_else(|_| "[]".to_string());
    let topics = serde_json::to_string(&analysis.topics).unwrap_or_else(|_| "[]".to_string());
    let entities = serde_json::to_string(&analysis.entities).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO lyrics_analysis (track_id, vibe_summary, annotations, lines, topics, entities, title, artist, thumbnail_url, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(track_id) DO UPDATE SET
            vibe_summary = excluded.vibe_summary,
            annotations = excluded.annotations,
            lines = excluded.lines,
            topics = excluded.topics,
            entities = excluded.entities,
            title = excluded.title,
            artist = excluded.artist,
            thumbnail_url = excluded.thumbnail_url,
            model = excluded.model,
            created_at = excluded.created_at",
        params![
            analysis.track_id,
            analysis.vibe_summary,
            annotations,
            lines,
            topics,
            entities,
            analysis.title,
            analysis.artist,
            analysis.thumbnail_url,
            analysis.model,
            analysis.created_at
        ],
    )?;
    Ok(())
}


pub(super) fn row_to_lyrics_translation(row: &rusqlite::Row) -> rusqlite::Result<LyricsTranslation> {
    let lines: String = row.get(1)?;
    let translations: String = row.get(2)?;
    Ok(LyricsTranslation {
        track_id: row.get(0)?,
        lines: serde_json::from_str(&lines).unwrap_or_default(),
        translations: serde_json::from_str(&translations).unwrap_or_default(),
        model: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub fn get_lyrics_translation(conn: &Connection, track_id: &str) -> rusqlite::Result<Option<LyricsTranslation>> {
    conn.query_row(
        "SELECT track_id, lines, translations, model, created_at FROM lyrics_translation WHERE track_id = ?1",
        params![track_id],
        row_to_lyrics_translation,
    )
    .optional()
}

pub fn save_lyrics_translation(conn: &Connection, translation: &LyricsTranslation) -> rusqlite::Result<()> {
    let lines = serde_json::to_string(&translation.lines).unwrap_or_else(|_| "[]".to_string());
    let translations = serde_json::to_string(&translation.translations).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO lyrics_translation (track_id, lines, translations, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(track_id) DO UPDATE SET
            lines = excluded.lines,
            translations = excluded.translations,
            model = excluded.model,
            created_at = excluded.created_at",
        params![
            translation.track_id,
            lines,
            translations,
            translation.model,
            translation.created_at
        ],
    )?;
    Ok(())
}
