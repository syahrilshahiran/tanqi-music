use crate::models::{AppSettings, AudioAnalysis, HeadphoneProfile, LyricsAnalysis, LyricsTranslation, Playlist};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

mod migrations;
mod settings;
mod headphones;
mod lyrics;
mod audio;
mod cache;
mod collections;
pub use settings::*;
pub use headphones::*;
pub use lyrics::*;
pub use audio::*;
pub use cache::*;
pub use collections::*;
use migrations::create_schema;
#[cfg(test)]
use migrations::*;

/// Everything the data modules can fail with. Converted to a `String` only
/// where the UI consumes it (`From<StoreError> for String`).
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    /// A thread panicked while holding the connection.
    Poisoned,
    /// A `FolderScanner` couldn't read a folder or a file in it.
    Scanner(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sqlite(e) => write!(f, "{e}"),
            StoreError::Poisoned => write!(f, "database lock poisoned"),
            StoreError::Scanner(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

impl From<StoreError> for String {
    fn from(e: StoreError) -> Self {
        e.to_string()
    }
}

/// The SQLite handle shared by every data module: one writer connection and, when the database is a file,
/// a second read-only one. Under WAL a reader sees the last committed state without waiting for a writer,
/// so listing the Library does not stall behind a Scan flush. Cheap to clone; the lock and the error
/// conversion live here so callers write neither.
#[derive(Clone)]
pub struct Db {
    writer: Arc<Mutex<Connection>>,
    reader: Option<Arc<Mutex<Connection>>>,
}

impl Db {
    /// A handle with a single connection (reads and writes share it).
    pub fn new(conn: Connection) -> Self {
        Db { writer: Arc::new(Mutex::new(conn)), reader: None }
    }

    /// A handle whose reads go to their own connection (see `open_reader`).
    pub fn with_reader(writer: Connection, reader: Connection) -> Self {
        Db { writer: Arc::new(Mutex::new(writer)), reader: Some(Arc::new(Mutex::new(reader))) }
    }

    /// A throwaway database with the real schema, for tests.
    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        create_schema(&conn)?;
        Ok(Db::new(conn))
    }

    /// Runs `f` with the writer connection locked. Don't do slow work inside `f`.
    pub fn with<T, E: Into<StoreError>>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, StoreError> {
        let conn = self.writer.lock().map_err(|_| StoreError::Poisoned)?;
        f(&conn).map_err(Into::into)
    }

    /// Runs the read-only `f` on the reader connection when there is one, else on the writer.
    pub fn read<T, E: Into<StoreError>>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, StoreError> {
        match &self.reader {
            Some(reader) => {
                let conn = reader.lock().map_err(|_| StoreError::Poisoned)?;
                f(&conn).map_err(Into::into)
            }
            None => self.with(f),
        }
    }
}

/// A read-only second connection to the database `init_db` created in `app_data_dir`.
pub fn open_reader(app_data_dir: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        app_data_dir.join("tanqi-music.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    Ok(conn)
}

pub fn init_db(app_data_dir: &Path) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(app_data_dir).ok();
    let db_path = app_data_dir.join("tanqi-music.db");
    let conn = Connection::open(db_path)?;
    set_pragmas(&conn)?;
    create_schema(&conn)?;
    Ok(conn)
}

/// Connection-level tuning, applied before any query runs.
///
/// SQLite's defaults (`journal_mode=DELETE`, `synchronous=FULL`) are built
/// for data you cannot re-derive. Everything in this file is either a cache
/// of something public (lyrics, album metadata) or a library index that is
/// rebuilt by rescanning the music folders, so the safety those defaults buy
/// is not worth their fsync cost. With `synchronous=NORMAL` under WAL an OS
/// crash or power loss can lose the most recent transactions; an application
/// crash cannot.
fn set_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        -- Nothing should contend today (one Connection, one Mutex), but a
        -- lock that does appear should wait rather than fail the query
        -- outright - e.g. if a future background job opens its own handle.
        PRAGMA busy_timeout = 5000;
        ",
    )
}

#[cfg(test)]
mod tests {

    /// `init_db` must actually leave the connection in WAL. This is easy to
    /// break silently: WAL can only be set on a file-backed database, so the
    /// in-memory connections the rest of these tests use would report
    /// "memory" no matter what `set_pragmas` did.
    #[test]
    fn data_is_deleted_by_kind_and_only_the_kinds_chosen() {
        use crate::models::DataPart::*;
        let dir = std::env::temp_dir().join(format!("tanqi-db-delete-{}", std::process::id()));
        let conn = init_db(&dir).unwrap();
        conn.execute_batch(
            "INSERT INTO library_folders (path, added_at) VALUES ('/m', 'x');
             INSERT INTO audio_analysis (track_id, bass_db, mid_db, treble_db, overall_db, created_at) VALUES ('a', 0, 0, 0, 0, 'x');
             INSERT INTO artist_pictures (artist_key, thumbnail_url, fetched_at) VALUES ('k', 'u', 'x');
             INSERT INTO favorites (track_id, added_at) VALUES ('a', 'x');
             INSERT INTO playlists (name, created_at) VALUES ('p', 'x');
             INSERT INTO headphone_profiles (name, preset, created_at) VALUES ('h', 'Preamp: 0 dB', 'x');
             INSERT INTO settings (key, value) VALUES ('active_headphone', '1');",
        )
        .unwrap();
        let count = |t: &str| conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap();
        delete_data(&conn, &[AudioAnalyses, HeadphoneCorrections]).unwrap();
        assert_eq!((count("audio_analysis"), count("headphone_profiles")), (0, 0));
        assert_eq!(get_active_headphone(&conn).unwrap(), None);
        // Everything else is still there.
        assert_eq!((count("library_folders"), count("artist_pictures"), count("favorites"), count("playlists")), (1, 1, 1, 1));
        delete_data(&conn, &[Music, ArtistPictures, FavoritesAndPlaylists, LyricsData]).unwrap();
        assert_eq!((count("library_folders"), count("artist_pictures"), count("favorites"), count("playlists")), (0, 0, 0, 0));
    }

    #[test]
    fn optional_features_default_off_on_a_fresh_install_and_on_for_an_existing_one() {
        let dir = std::env::temp_dir().join(format!("tanqi-db-features-{}", std::process::id()));
        let conn = init_db(&dir).unwrap();
        let fresh = get_settings(&conn).unwrap();
        assert!(!fresh.audio_analysis_enabled && !fresh.lyrics_ai_enabled);
        // Finished onboarding before the features existed: no stored choice, everything stays on.
        conn.execute("INSERT INTO settings (key, value) VALUES ('onboarding_completed', 'true')", []).unwrap();
        let existing = get_settings(&conn).unwrap();
        assert!(existing.audio_analysis_enabled && existing.lyrics_ai_enabled);
        // A stored choice wins.
        let mut chosen = existing.clone();
        chosen.audio_analysis_enabled = false;
        set_settings(&conn, &chosen).unwrap();
        let back = get_settings(&conn).unwrap();
        assert!(!back.audio_analysis_enabled && back.lyrics_ai_enabled);
    }

    #[test]
    fn init_db_enables_wal_and_relaxed_sync() {
        let dir = std::env::temp_dir().join(format!("tanqi-db-test-{}", std::process::id()));
        let conn = super::init_db(&dir).expect("init_db");

        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal.to_lowercase(), "wal");

        // 1 == NORMAL.
        let sync: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        assert_eq!(sync, 1);

        drop(conn);
        std::fs::remove_dir_all(&dir).ok();
    }
    use super::*;
    use crate::models::{EqBand, LyricsAnnotation};

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn settings_roundtrip_includes_ai_model() {
        let conn = test_conn();
        let mut settings = AppSettings::default();
        settings.ai_model = "qwen3:8b".to_string();
        set_settings(&conn, &settings).unwrap();

        let loaded = get_settings(&conn).unwrap();
        assert_eq!(loaded.ai_model, "qwen3:8b");
    }

    /// A database written by an earlier version, with the streaming-service
    /// columns still in `saved_tracks` (the table's old name).
    fn old_shaped_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE saved_tracks (
                id TEXT PRIMARY KEY, video_id TEXT NOT NULL UNIQUE, title TEXT NOT NULL, artist TEXT NOT NULL,
                artist_browse_id TEXT, album TEXT, album_browse_id TEXT, duration_ms INTEGER, thumbnail_url TEXT,
                plays TEXT, explicit INTEGER NOT NULL DEFAULT 0, saved_at TEXT NOT NULL,
                album_artist TEXT, track_no INTEGER, disc_no INTEGER, year INTEGER, root TEXT, mtime INTEGER
             );
             CREATE INDEX idx_saved_tracks_root ON saved_tracks(root);
             INSERT INTO saved_tracks (id, video_id, title, artist, album, duration_ms, explicit, saved_at, track_no, root, mtime)
                VALUES ('t1', 'local:/m/a.flac', 'A Song', 'An Artist', 'An Album', 1000, 1, '2026-01-01T00:00:00Z', 3, '/m', 42);
             CREATE TABLE audio_analysis (
                video_id TEXT PRIMARY KEY, bass_db REAL NOT NULL, mid_db REAL NOT NULL, treble_db REAL NOT NULL,
                overall_db REAL NOT NULL, eq_bands TEXT NOT NULL DEFAULT '[]', preamp_db REAL NOT NULL DEFAULT 0,
                bpm REAL, key TEXT, title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '',
                thumbnail_url TEXT, created_at TEXT NOT NULL
             );
             INSERT INTO audio_analysis (video_id, bass_db, mid_db, treble_db, overall_db, created_at)
                VALUES ('local:/m/a.flac', 1.5, 2.5, 3.5, 4.5, '2026-01-02T00:00:00Z');
             CREATE TABLE lyrics_analysis (
                video_id TEXT PRIMARY KEY, vibe_summary TEXT NOT NULL, annotations TEXT NOT NULL, lines TEXT NOT NULL,
                topics TEXT NOT NULL DEFAULT '[]', entities TEXT NOT NULL DEFAULT '[]', title TEXT NOT NULL DEFAULT '',
                artist TEXT NOT NULL DEFAULT '', thumbnail_url TEXT, model TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TABLE lyrics_translation (
                video_id TEXT PRIMARY KEY, lines TEXT NOT NULL, translations TEXT NOT NULL, model TEXT NOT NULL,
                created_at TEXT NOT NULL
             );
             INSERT INTO lyrics_translation (video_id, lines, translations, model, created_at)
                VALUES ('local:/m/a.flac', '[]', '[]', 'qwen3:8b', '2026-01-03T00:00:00Z');",
        )
        .unwrap();
        conn
    }

    fn track_columns(conn: &Connection, table: &str) -> Vec<String> {
        conn.prepare("SELECT name FROM pragma_table_info(?1)")
            .unwrap()
            .query_map([table], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn opening_an_old_database_renames_the_table_and_columns_drops_the_unused_ones_and_keeps_the_rows() {
        let conn = old_shaped_conn();
        assert!(track_columns(&conn, "saved_tracks").contains(&"explicit".to_string()));

        create_schema(&conn).unwrap();

        let columns = track_columns(&conn, "tracks");
        assert!(columns.contains(&"added_at".to_string()) && !columns.contains(&"saved_at".to_string()), "{columns:?}");
        for gone in UNUSED_TRACK_COLUMNS {
            assert!(!columns.iter().any(|c| c == gone), "{gone} should be dropped: {columns:?}");
        }
        let row: (String, String, i64, i64, String, String) = conn
            .query_row("SELECT title, album, track_no, mtime, root, added_at FROM tracks WHERE id = 'local:/m/a.flac'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .unwrap();
        assert_eq!(
            row,
            ("A Song".to_string(), "An Album".to_string(), 3, 42, "/m".to_string(), "2026-01-01T00:00:00Z".to_string()),
            "the timestamp moved with its column"
        );

        // Opening it again changes nothing.
        create_schema(&conn).unwrap();
        assert_eq!(track_columns(&conn, "tracks"), columns);
    }

    fn schema_names(conn: &Connection, kind: &str) -> Vec<String> {
        conn.prepare("SELECT name FROM sqlite_master WHERE type = ?1")
            .unwrap()
            .query_map([kind], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn opening_an_old_database_keys_tracks_by_id_and_analyses_by_track_id() {
        let conn = old_shaped_conn();
        assert!(track_columns(&conn, "saved_tracks").contains(&"video_id".to_string()));

        create_schema(&conn).unwrap();

        // The tracks table is rebuilt: the old `video_id` is now the key, and
        // the generated key nothing used is gone.
        let columns = track_columns(&conn, "tracks");
        assert_eq!(columns.iter().filter(|c| *c == "id").count(), 1, "{columns:?}");
        assert!(!columns.contains(&"video_id".to_string()), "{columns:?}");
        let (id, rows): (String, i64) = conn
            .query_row("SELECT id, (SELECT COUNT(*) FROM tracks) FROM tracks", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((id.as_str(), rows), ("local:/m/a.flac", 1));
        let pk: String = conn
            .query_row("SELECT name FROM pragma_table_info('tracks') WHERE pk = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pk, "id");
        assert!(schema_names(&conn, "index").contains(&"idx_tracks_root".to_string()));

        // The analysis tables keep their rows under `track_id`.
        for table in ["audio_analysis", "lyrics_analysis", "lyrics_translation"] {
            let columns = track_columns(&conn, table);
            assert!(columns.contains(&"track_id".to_string()), "{table}: {columns:?}");
            assert!(!columns.contains(&"video_id".to_string()) && !columns.contains(&"id".to_string()), "{table}: {columns:?}");
        }
        let (bass, created): (f64, String) = conn
            .query_row("SELECT bass_db, created_at FROM audio_analysis WHERE track_id = 'local:/m/a.flac'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((bass, created.as_str()), (1.5, "2026-01-02T00:00:00Z"));
        let model: String = conn
            .query_row("SELECT model FROM lyrics_translation WHERE track_id = 'local:/m/a.flac'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(model, "qwen3:8b");

        // The lookups the app runs still work on the migrated shape, and
        // opening it again changes nothing.
        assert!(get_audio_analysis(&conn, "local:/m/a.flac").unwrap().is_some());
        assert!(get_lyrics_translation(&conn, "local:/m/a.flac").unwrap().is_some());
        create_schema(&conn).unwrap();
        assert_eq!(track_columns(&conn, "tracks"), columns);
    }

    #[test]
    fn analysis_tables_keyed_by_id_are_renamed_to_track_id_keeping_their_rows() {
        // The shape the previous version wrote: tracks already keyed by `id`,
        // analyses keyed by `id` too.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT, duration_ms INTEGER,
                thumbnail_url TEXT, added_at TEXT NOT NULL, album_artist TEXT, track_no INTEGER, disc_no INTEGER,
                year INTEGER, root TEXT, mtime INTEGER
             );
             INSERT INTO tracks (id, title, artist, added_at) VALUES ('local:/m/a.flac', 'A Song', 'An Artist', '2026-01-01T00:00:00Z');
             CREATE TABLE audio_analysis (
                id TEXT PRIMARY KEY, bass_db REAL NOT NULL, mid_db REAL NOT NULL, treble_db REAL NOT NULL,
                overall_db REAL NOT NULL, eq_bands TEXT NOT NULL DEFAULT '[]', preamp_db REAL NOT NULL DEFAULT 0,
                bpm REAL, key TEXT, title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '',
                thumbnail_url TEXT, created_at TEXT NOT NULL
             );
             INSERT INTO audio_analysis (id, bass_db, mid_db, treble_db, overall_db, created_at)
                VALUES ('local:/m/a.flac', 1.5, 2.5, 3.5, 4.5, '2026-01-02T00:00:00Z');",
        )
        .unwrap();

        create_schema(&conn).unwrap();

        let columns = track_columns(&conn, "audio_analysis");
        assert!(columns.contains(&"track_id".to_string()) && !columns.contains(&"id".to_string()), "{columns:?}");
        let analysis = get_audio_analysis(&conn, "local:/m/a.flac").unwrap().unwrap();
        assert_eq!((analysis.track_id.as_str(), analysis.bass_db), ("local:/m/a.flac", 1.5));
        // The analysis joins to its track, which is still keyed by `id`.
        assert_eq!(analysis.title, "A Song");
        assert!(track_columns(&conn, "tracks").contains(&"id".to_string()));
        // The tables that did not exist yet are created fresh, already keyed by `track_id`.
        assert!(track_columns(&conn, "lyrics_analysis").contains(&"track_id".to_string()));

        create_schema(&conn).unwrap();
        assert_eq!(track_columns(&conn, "audio_analysis"), columns);
    }

    #[test]
    fn the_old_table_and_index_names_are_gone_after_opening_and_nothing_is_duplicated() {
        let conn = old_shaped_conn();
        create_schema(&conn).unwrap();

        let tables = schema_names(&conn, "table");
        assert!(tables.contains(&"tracks".to_string()), "{tables:?}");
        assert!(!tables.contains(&"saved_tracks".to_string()), "{tables:?}");
        let indexes = schema_names(&conn, "index");
        assert!(indexes.contains(&"idx_tracks_root".to_string()), "{indexes:?}");
        assert!(!indexes.contains(&"idx_saved_tracks_root".to_string()), "{indexes:?}");

        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1, "the one existing row moved with the table");
    }

    #[test]
    fn a_new_database_never_has_the_unused_track_columns() {
        let conn = test_conn();
        let columns = track_columns(&conn, "tracks");
        assert!(UNUSED_TRACK_COLUMNS.iter().all(|c| !columns.iter().any(|e| e == c)), "{columns:?}");
        assert!(columns.contains(&"root".to_string()));
    }

    #[test]
    fn lyrics_analysis_roundtrip() {
        let conn = test_conn();
        assert!(get_lyrics_analysis(&conn, "vid1").unwrap().is_none());

        let analysis = LyricsAnalysis {
            track_id: "vid1".to_string(),
            vibe_summary: "Melancholic and theatrical.".to_string(),
            annotations: vec![LyricsAnnotation {
                start_line: 1,
                end_line: 2,
                meaning: "Opens with existential questioning.".to_string(),
            }],
            lines: vec!["Is this the real life?".to_string(), "Is this just fantasy?".to_string()],
            topics: vec!["existentialism".to_string()],
            entities: vec!["Queen".to_string()],
            title: "Bohemian Rhapsody".to_string(),
            artist: "Queen".to_string(),
            thumbnail_url: None,
            model: "qwen3:8b".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        save_lyrics_analysis(&conn, &analysis).unwrap();

        let loaded = get_lyrics_analysis(&conn, "vid1").unwrap().unwrap();
        assert_eq!(loaded.vibe_summary, "Melancholic and theatrical.");
        assert_eq!(loaded.lines.len(), 2);
        assert_eq!(loaded.annotations.len(), 1);
        assert_eq!(loaded.annotations[0].meaning, "Opens with existential questioning.");
        assert_eq!(loaded.topics, vec!["existentialism".to_string()]);
        assert_eq!(loaded.entities, vec!["Queen".to_string()]);
        assert_eq!(loaded.title, "Bohemian Rhapsody");
        assert_eq!(loaded.artist, "Queen");

        // Re-saving (e.g. a "Re-analyze" click) updates in place rather than
        // erroring on the existing primary key.
        let mut updated = analysis.clone();
        updated.vibe_summary = "Updated vibe.".to_string();
        save_lyrics_analysis(&conn, &updated).unwrap();
        assert_eq!(get_lyrics_analysis(&conn, "vid1").unwrap().unwrap().vibe_summary, "Updated vibe.");
    }

    #[test]
    fn a_blank_snapshot_is_backfilled_from_the_tracks_row_on_lookup() {
        let conn = test_conn();

        // Simulates a `lyrics_analysis` row left over from before
        // title/artist were tracked (an ALTER TABLE default of '') - the
        // track being in the library means it can still be backfilled.
        let blank = LyricsAnalysis {
            track_id: "vid1".to_string(),
            vibe_summary: "Vibe.".to_string(),
            annotations: vec![],
            lines: vec![],
            topics: vec![],
            entities: vec![],
            title: "".to_string(),
            artist: "".to_string(),
            thumbnail_url: None,
            model: "qwen3:8b".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        save_lyrics_analysis(&conn, &blank).unwrap();
        conn.execute(
            "INSERT INTO tracks (id, title, artist, thumbnail_url, added_at)
             VALUES ('vid1', 'Backfilled Title', 'Backfilled Artist', 'https://example.com/a.jpg', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let single = get_lyrics_analysis(&conn, "vid1").unwrap().unwrap();
        assert_eq!(single.title, "Backfilled Title");
        assert_eq!(single.artist, "Backfilled Artist");
        assert_eq!(single.thumbnail_url, Some("https://example.com/a.jpg".to_string()));

        // A track that isn't in the library has nothing to backfill from: the
        // row stays directly readable by track id, just with a blank title.
        let orphaned = LyricsAnalysis { track_id: "vid2".to_string(), ..blank.clone() };
        save_lyrics_analysis(&conn, &orphaned).unwrap();
        assert_eq!(get_lyrics_analysis(&conn, "vid2").unwrap().unwrap().title, "");

        // The browse list backfills the first and drops the untitled one.
        let listed = list_analyzed_lyrics(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].track_id, "vid1");
    }

    #[test]
    fn audio_analysis_roundtrip() {
        let conn = test_conn();
        assert!(get_audio_analysis(&conn, "vid1").unwrap().is_none());

        let analysis = AudioAnalysis {
            track_id: "vid1".to_string(),
            bass_db: 29.2,
            mid_db: 20.4,
            treble_db: 3.8,
            overall_db: -17.2,
            loudness_lufs: Some(-13.4),
            true_peak_db: Some(-0.8),
            spectrum_db: vec![Some(-40.0), None, Some(-35.5)],
            lowpass_hz: Some(16000.0),
            analysis_version: 2,
            eq_bands: vec![EqBand {
                hz: 31.0,
                measured_db: 7.8,
                suggested_gain_db: 4.6,
                q: 1.4,
            }],
            preamp_db: -6.0,
            bpm: Some(128.0),
            key: None,
            title: "Heavy Serenade".to_string(),
            artist: "NMIXX".to_string(),
            thumbnail_url: Some("https://example.com/a.jpg".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        save_audio_analysis(&conn, &analysis).unwrap();

        let loaded = get_audio_analysis(&conn, "vid1").unwrap().unwrap();
        assert_eq!(loaded.bass_db, 29.2);
        assert_eq!(loaded.treble_db, 3.8);
        assert_eq!(loaded.preamp_db, -6.0);
        assert_eq!(loaded.eq_bands.len(), 1);
        assert_eq!(loaded.eq_bands[0].hz, 31.0);
        assert_eq!(loaded.title, "Heavy Serenade");
        assert_eq!(loaded.artist, "NMIXX");
        assert_eq!(loaded.bpm, Some(128.0));

        // Re-running (force) replaces the row rather than accumulating history.
        let updated = AudioAnalysis {
            bass_db: 10.0,
            ..analysis
        };
        save_audio_analysis(&conn, &updated).unwrap();
        assert_eq!(get_audio_analysis(&conn, "vid1").unwrap().unwrap().bass_db, 10.0);
    }

    #[test]
    fn list_analyzed_audio_joins_and_backfills_from_tracks() {
        let conn = test_conn();
        assert_eq!(list_analyzed_audio(&conn).unwrap().len(), 0);

        // An analysis with no title/artist snapshot of its own (as if saved
        // before those columns existed) should backfill from tracks.
        conn.execute(
            "INSERT INTO tracks (id, title, artist, added_at) VALUES ('vid1', 'Backfilled Title', 'Backfilled Artist', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let blank_snapshot = AudioAnalysis {
            track_id: "vid1".to_string(),
            bass_db: 1.0,
            mid_db: 1.0,
            treble_db: 1.0,
            overall_db: 1.0,
            loudness_lufs: None,
            true_peak_db: None,
            spectrum_db: vec![],
            lowpass_hz: None,
            analysis_version: 1,
            eq_bands: vec![],
            preamp_db: 0.0,
            bpm: None,
            key: None,
            title: String::new(),
            artist: String::new(),
            thumbnail_url: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        save_audio_analysis(&conn, &blank_snapshot).unwrap();

        // An orphaned analysis (no matching tracks row, and no
        // snapshot of its own) should be excluded from the browse list
        // rather than shown as a blank entry.
        let orphaned = AudioAnalysis {
            track_id: "vid2".to_string(),
            ..blank_snapshot.clone()
        };
        save_audio_analysis(&conn, &orphaned).unwrap();

        let all = list_analyzed_audio(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].track_id, "vid1");
        assert_eq!(all[0].title, "Backfilled Title");
        assert_eq!(all[0].artist, "Backfilled Artist");
    }

    #[test]
    fn playlists_keep_order_skip_duplicates_and_compact_on_remove() {
        let conn = test_conn();
        let id = create_playlist(&conn, "Road").unwrap();
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        add_to_playlist(&conn, id, &ids).unwrap();
        add_to_playlist(&conn, id, &["b".to_string(), "d".to_string()]).unwrap();
        assert_eq!(list_playlists(&conn).unwrap()[0].track_ids, ["a", "b", "c", "d"]);

        move_in_playlist(&conn, id, 3, -1).unwrap();
        assert_eq!(list_playlists(&conn).unwrap()[0].track_ids, ["a", "b", "d", "c"]);

        remove_from_playlist(&conn, id, 1).unwrap();
        add_to_playlist(&conn, id, &["e".to_string()]).unwrap();
        assert_eq!(list_playlists(&conn).unwrap()[0].track_ids, ["a", "d", "c", "e"]);

        delete_playlist(&conn, id).unwrap();
        assert!(list_playlists(&conn).unwrap().is_empty());
    }

    #[test]
    fn guidance_is_saved_trimmed_and_capped_and_empty_means_default() {
        let conn = test_conn();
        let mut settings = AppSettings::default();
        assert_eq!(settings.analysis_guidance_or_default(), crate::models::DEFAULT_ANALYSIS_GUIDANCE);

        settings.analysis_guidance = format!("  {}  ", "x".repeat(2000));
        settings.translation_guidance = "   ".to_string();
        set_settings(&conn, &settings).unwrap();

        let back = get_settings(&conn).unwrap();
        assert_eq!(back.analysis_guidance.chars().count(), crate::models::MAX_GUIDANCE_CHARS);
        assert!(back.translation_guidance.is_empty());
        assert_eq!(back.translation_guidance_or_default(), crate::models::DEFAULT_TRANSLATION_GUIDANCE);
    }

    #[test]
    fn the_reader_connection_sees_what_the_writer_committed() {
        let dir = std::env::temp_dir().join(format!("tanqi-db-reader-{}", std::process::id()));
        let db = Db::with_reader(init_db(&dir).unwrap(), open_reader(&dir).unwrap());
        db.with(|c| set_favorite(c, "a", true)).unwrap();
        assert_eq!(db.read(|c| list_favorite_ids(c)).unwrap(), vec!["a".to_string()]);
        // The reader is read-only.
        assert!(db.read(|c| c.execute("DELETE FROM favorites", [])).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_ai_settings_roundtrip_and_the_old_ollama_names_are_still_read() {
        use crate::models::AiProvider;
        let conn = test_conn();
        // A database from before there were other providers.
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('ollama_model', 'llama3:8b');
             INSERT INTO settings (key, value) VALUES ('ollama_url', '192.168.1.20');",
        )
        .unwrap();
        let old = get_settings(&conn).unwrap();
        assert_eq!((old.ai_provider, old.ai_model.as_str(), old.ai_url.as_str()), (AiProvider::Ollama, "llama3:8b", "http://192.168.1.20:11434"));

        let mut next = old;
        next.ai_provider = AiProvider::OpenAi;
        next.ai_model = "gpt-4o-mini".into();
        next.ai_url = "api.openai.com/v1/".into();
        set_settings(&conn, &next).unwrap();
        let back = get_settings(&conn).unwrap();
        assert_eq!((back.ai_provider, back.ai_model.as_str(), back.ai_url.as_str()), (AiProvider::OpenAi, "gpt-4o-mini", "https://api.openai.com/v1"));
    }

    #[test]
    fn the_old_lyrics_switch_becomes_the_ai_lyrics_switch() {
        let conn = test_conn();
        conn.execute_batch("INSERT INTO settings (key, value) VALUES ('lyrics_enabled', 'false');").unwrap();
        assert!(!get_settings(&conn).unwrap().lyrics_ai_enabled);
        // The new name wins once it exists.
        conn.execute_batch("INSERT INTO settings (key, value) VALUES ('lyrics_ai_enabled', 'true');").unwrap();
        assert!(get_settings(&conn).unwrap().lyrics_ai_enabled);
    }
}
