//! The schema, and the steps that bring a database written by an earlier version up to it.

use super::*;

pub(super) fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    rename_old_track_table(conn)?;
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tracks (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT,
            duration_ms INTEGER,
            thumbnail_url TEXT,
            added_at TEXT NOT NULL,
            album_artist TEXT,
            track_no INTEGER,
            disc_no INTEGER,
            year INTEGER,
            -- The library folder this file was scanned from, and the file's
            -- mtime at scan time (skip re-reading tags when unchanged).
            root TEXT,
            mtime INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_tracks_root ON tracks(root);

        CREATE TABLE IF NOT EXISTS library_folders (
            path TEXT PRIMARY KEY,
            added_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS lyrics_analysis (
            track_id TEXT PRIMARY KEY,
            vibe_summary TEXT NOT NULL,
            annotations TEXT NOT NULL,
            lines TEXT NOT NULL,
            topics TEXT NOT NULL DEFAULT '[]',
            entities TEXT NOT NULL DEFAULT '[]',
            title TEXT NOT NULL DEFAULT '',
            artist TEXT NOT NULL DEFAULT '',
            thumbnail_url TEXT,
            model TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS lyrics_translation (
            track_id TEXT PRIMARY KEY,
            lines TEXT NOT NULL,
            translations TEXT NOT NULL,
            model TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS audio_analysis (
            track_id TEXT PRIMARY KEY,
            bass_db REAL NOT NULL,
            mid_db REAL NOT NULL,
            treble_db REAL NOT NULL,
            overall_db REAL NOT NULL,
            eq_bands TEXT NOT NULL DEFAULT '[]',
            preamp_db REAL NOT NULL DEFAULT 0,
            bpm REAL,
            key TEXT,
            loudness_lufs REAL,
            true_peak_db REAL,
            spectrum TEXT,
            lowpass_hz REAL,
            analysis_version INTEGER NOT NULL DEFAULT 1,
            title TEXT NOT NULL DEFAULT '',
            artist TEXT NOT NULL DEFAULT '',
            thumbnail_url TEXT,
            created_at TEXT NOT NULL
        );

        -- Tracks the user has hearted. Keyed by track id, not tied to the
        -- tracks table, so a favorite survives a Scan that briefly drops
        -- and re-adds its file.
        CREATE TABLE IF NOT EXISTS favorites (
            track_id TEXT PRIMARY KEY,
            added_at TEXT NOT NULL
        );

        -- Answers from online lookups (track info, lyrics...), so reopening a
        -- track doesn't ask the same services again. Each row says when it
        -- stops being trusted; errors are never stored.
        CREATE TABLE IF NOT EXISTS api_cache (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            expires_at INTEGER NOT NULL
        );

        -- An artist's picture, kept separately from their albums' covers. Keyed
        -- by `library::group_key` of the name, so spelling variants share one.
        CREATE TABLE IF NOT EXISTS artist_pictures (
            artist_key TEXT PRIMARY KEY,
            thumbnail_url TEXT NOT NULL,
            fetched_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS playlists (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        -- `position` is 0-based and kept gapless by the functions below.
        CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            track_id TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_playlist_tracks ON playlist_tracks(playlist_id, position);

        -- Saved headphone corrections: the pasted parametric EQ export, kept verbatim and named for the
        -- earphone or headphone it is for.
        CREATE TABLE IF NOT EXISTS headphone_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            preset TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )?;
    rename_old_added_at_column(conn)?;
    drop_unused_track_columns(conn)?;
    migrate_video_id_to_id(conn)?;
    key_analysis_tables_by_track_id(conn)?;
    add_loudness_columns(conn)?;
    Ok(())
}

/// The tracks table was called `saved_tracks` back when tracks were saved
/// rather than scanned. A database from that time is renamed when it is
/// opened (this has to run before the `CREATE TABLE IF NOT EXISTS` above, or
/// an empty `tracks` would appear next to the real data). Does nothing for a
/// database that already has `tracks`.
pub(super) fn rename_old_track_table(conn: &Connection) -> rusqlite::Result<()> {
    let has_table = |name: &str| -> rusqlite::Result<bool> {
        conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get::<_, i64>(0))
            .map(|n| n > 0)
    };
    if has_table("saved_tracks")? && !has_table("tracks")? {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch("ALTER TABLE saved_tracks RENAME TO tracks; DROP INDEX IF EXISTS idx_saved_tracks_root;")?;
        tx.commit()?;
    }
    Ok(())
}

/// Columns `tracks` once had for tracks from a streaming service.
/// Nothing reads or writes them any more, so a database created by an earlier
/// version has them dropped when it is opened. Safe to run every time: only
/// columns that are still there are touched.
pub(super) const UNUSED_TRACK_COLUMNS: &[&str] = &["artist_browse_id", "album_browse_id", "plays", "explicit"];

pub(super) fn drop_unused_track_columns(conn: &Connection) -> rusqlite::Result<()> {
    let existing = track_column_names(conn)?;
    let stale: Vec<&&str> = UNUSED_TRACK_COLUMNS.iter().filter(|c| existing.iter().any(|e| e == **c)).collect();
    if stale.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for column in stale {
        tx.execute(&format!("ALTER TABLE tracks DROP COLUMN {column}"), [])?;
    }
    tx.commit()
}

pub(super) fn column_names(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT name FROM pragma_table_info(?1)")?
        .query_map([table], |r| r.get(0))?
        .collect()
}

pub(super) fn track_column_names(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    column_names(conn, "tracks")
}

/// `added_at` (when a track's row was first created) was called `saved_at`
/// back when tracks were saved rather than scanned. Renamed on open for a
/// database from that time; does nothing once it has `added_at`.
pub(super) fn rename_old_added_at_column(conn: &Connection) -> rusqlite::Result<()> {
    let columns = track_column_names(conn)?;
    if columns.iter().any(|c| c == "saved_at") && !columns.iter().any(|c| c == "added_at") {
        conn.execute("ALTER TABLE tracks RENAME COLUMN saved_at TO added_at", [])?;
    }
    Ok(())
}

/// A track used to carry two keys: a generated `id` nothing ever read, and
/// `video_id` (a name from when tracks came from YouTube). Now the track's own
/// key is `id`. SQLite can't change a primary key in place, so `tracks` is
/// rebuilt, dropping the unused generated key. Does nothing once the table has
/// no `video_id` column.
pub(super) fn migrate_video_id_to_id(conn: &Connection) -> rusqlite::Result<()> {
    if !column_names(conn, "tracks")?.iter().any(|c| c == "video_id") {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    // Same columns as the `tracks` definition in `create_schema`.
    tx.execute_batch(
        "CREATE TABLE tracks_new (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT,
            duration_ms INTEGER,
            thumbnail_url TEXT,
            added_at TEXT NOT NULL,
            album_artist TEXT,
            track_no INTEGER,
            disc_no INTEGER,
            year INTEGER,
            root TEXT,
            mtime INTEGER
         );
         INSERT INTO tracks_new (id, title, artist, album, duration_ms, thumbnail_url, added_at,
                                 album_artist, track_no, disc_no, year, root, mtime)
            SELECT video_id, title, artist, album, duration_ms, thumbnail_url, added_at,
                   album_artist, track_no, disc_no, year, root, mtime FROM tracks;
         DROP TABLE tracks;
         ALTER TABLE tracks_new RENAME TO tracks;
         CREATE INDEX IF NOT EXISTS idx_tracks_root ON tracks(root);",
    )?;
    tx.commit()
}

/// The lyrics and audio analysis tables are keyed by the id of the track they
/// describe, and call that column `track_id`. Databases from earlier versions
/// named it `video_id`, or (briefly) `id`; the column is renamed in place,
/// keeping its rows. Does nothing once a table has `track_id`.
pub(super) fn key_analysis_tables_by_track_id(conn: &Connection) -> rusqlite::Result<()> {
    let mut renames = Vec::new();
    for table in ["lyrics_analysis", "lyrics_translation", "audio_analysis"] {
        let columns = column_names(conn, table)?;
        if columns.iter().any(|c| c == "track_id") {
            continue;
        }
        if let Some(old) = ["video_id", "id"].into_iter().find(|old| columns.iter().any(|c| c == old)) {
            renames.push((table, old));
        }
    }
    if renames.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for (table, old) in renames {
        tx.execute(&format!("ALTER TABLE {table} RENAME COLUMN {old} TO track_id"), [])?;
    }
    tx.commit()
}

/// Adds the loudness and true-peak columns to an `audio_analysis` table made before they existed.
pub(super) fn add_loudness_columns(conn: &Connection) -> rusqlite::Result<()> {
    let has: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('audio_analysis') WHERE name = 'loudness_lufs'")?
        .exists([])?;
    if !has {
        conn.execute_batch("ALTER TABLE audio_analysis ADD COLUMN loudness_lufs REAL; ALTER TABLE audio_analysis ADD COLUMN true_peak_db REAL;")?;
    }
    let has_spectrum: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('audio_analysis') WHERE name = 'spectrum'")?
        .exists([])?;
    if !has_spectrum {
        conn.execute_batch(
            "ALTER TABLE audio_analysis ADD COLUMN spectrum TEXT;
             ALTER TABLE audio_analysis ADD COLUMN lowpass_hz REAL;
             ALTER TABLE audio_analysis ADD COLUMN analysis_version INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    Ok(())
}
