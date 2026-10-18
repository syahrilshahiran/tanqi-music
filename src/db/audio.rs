//! Audio analyses.

use super::*;

pub(super) fn row_to_audio_analysis(row: &rusqlite::Row) -> rusqlite::Result<AudioAnalysis> {
    let eq_bands: String = row.get(5)?;
    Ok(AudioAnalysis {
        track_id: row.get(0)?,
        bass_db: row.get(1)?,
        mid_db: row.get(2)?,
        treble_db: row.get(3)?,
        overall_db: row.get(4)?,
        eq_bands: serde_json::from_str(&eq_bands).unwrap_or_default(),
        preamp_db: row.get(6)?,
        title: row.get(7)?,
        artist: row.get(8)?,
        thumbnail_url: row.get(9)?,
        created_at: row.get(10)?,
        bpm: row.get(11)?,
        key: row.get(12)?,
        loudness_lufs: row.get(13)?,
        true_peak_db: row.get(14)?,
        spectrum_db: row
            .get::<_, Option<String>>(15)?
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default(),
        lowpass_hz: row.get(16)?,
        analysis_version: row.get::<_, i64>(17)?.max(0) as u32,
    })
}

/// `title`/`artist`/`thumbnail_url` are backfilled from `tracks` when
/// the analysis's own snapshot is blank, same reasoning as
/// `LYRICS_ANALYSIS_SELECT`: analyses saved before those columns existed
/// (or a track that has since left the library) would otherwise render as an
/// empty, unclickable row wherever they're listed.
const AUDIO_ANALYSIS_SELECT: &str = "
    SELECT aa.track_id, aa.bass_db, aa.mid_db, aa.treble_db, aa.overall_db, aa.eq_bands, aa.preamp_db,
           COALESCE(NULLIF(s.title, ''), NULLIF(aa.title, ''), '') AS title,
           COALESCE(NULLIF(s.artist, ''), NULLIF(aa.artist, ''), '') AS artist,
           COALESCE(s.thumbnail_url, aa.thumbnail_url) AS thumbnail_url,
           aa.created_at, aa.bpm, aa.key, aa.loudness_lufs, aa.true_peak_db, aa.spectrum, aa.lowpass_hz,
           aa.analysis_version
    FROM audio_analysis aa
    LEFT JOIN tracks s ON s.id = aa.track_id";

/// Every saved artist picture, as (artist key, `local:` reference).

pub fn get_audio_analysis(conn: &Connection, track_id: &str) -> rusqlite::Result<Option<AudioAnalysis>> {
    conn.query_row(
        &format!("{AUDIO_ANALYSIS_SELECT} WHERE aa.track_id = ?1"),
        params![track_id],
        row_to_audio_analysis,
    )
    .optional()
}

/// Every track that's had its audio analyzed, most recent first - the same
/// browsing pattern as the lyrics analyses had, just for EQ data instead of
/// lyrics. Rows that still have no title after backfilling (an analysis
/// from before that was tracked, on a track that isn't in the library) are
/// dropped rather than surfaced as a blank entry.
pub fn list_analyzed_audio(conn: &Connection) -> rusqlite::Result<Vec<AudioAnalysis>> {
    let mut stmt = conn.prepare(&format!("{AUDIO_ANALYSIS_SELECT} ORDER BY aa.created_at DESC"))?;
    let rows = stmt.query_map([], row_to_audio_analysis)?;
    let all: Vec<AudioAnalysis> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(all.into_iter().filter(|a| !a.title.is_empty()).collect())
}

pub fn save_audio_analysis(conn: &Connection, analysis: &AudioAnalysis) -> rusqlite::Result<()> {
    let eq_bands = serde_json::to_string(&analysis.eq_bands).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO audio_analysis (track_id, bass_db, mid_db, treble_db, overall_db, eq_bands, preamp_db, title, artist, thumbnail_url, created_at, bpm, key, loudness_lufs, true_peak_db, spectrum, lowpass_hz, analysis_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT(track_id) DO UPDATE SET
            bass_db = excluded.bass_db,
            mid_db = excluded.mid_db,
            treble_db = excluded.treble_db,
            overall_db = excluded.overall_db,
            eq_bands = excluded.eq_bands,
            preamp_db = excluded.preamp_db,
            title = excluded.title,
            artist = excluded.artist,
            thumbnail_url = excluded.thumbnail_url,
            created_at = excluded.created_at,
            bpm = excluded.bpm,
            key = excluded.key,
            loudness_lufs = excluded.loudness_lufs,
            true_peak_db = excluded.true_peak_db,
            spectrum = excluded.spectrum,
            lowpass_hz = excluded.lowpass_hz,
            analysis_version = excluded.analysis_version",
        params![
            analysis.track_id,
            analysis.bass_db,
            analysis.mid_db,
            analysis.treble_db,
            analysis.overall_db,
            eq_bands,
            analysis.preamp_db,
            analysis.title,
            analysis.artist,
            analysis.thumbnail_url,
            analysis.created_at,
            analysis.bpm,
            analysis.key,
            analysis.loudness_lufs,
            analysis.true_peak_db,
            if analysis.spectrum_db.is_empty() { None } else { serde_json::to_string(&analysis.spectrum_db).ok() },
            analysis.lowpass_hz,
            analysis.analysis_version as i64
        ],
    )?;
    Ok(())
}
