//! The Library store: owns every Track, Library folder and Scan result
//! (see GLOSSARY.md), and the grouping of Tracks into albums.
//!
//! The store never touches the filesystem or the network. A Scan is driven
//! through a `FolderScanner` adapter that does the file work (the real one
//! is `scanner::FsScanner`); the store decides what is new, changed or gone,
//! writes in atomic chunks, and owns the scan-version rule. Track ids are
//! opaque strings (`local:<path>` for scanned files).

use crate::db::{Db, StoreError};
use crate::library::{self, Library};
use crate::models::{MissingCover, Track, ScanSummary, TrackSummary};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Bump when the scanner learns to read something new. A library last
/// scanned by an older version is re-read in full on the next `scan_all`,
/// so new abilities reach existing libraries.
/// v2: embedded cover art, more cover file names.
/// v3: tags inferred from the folder layout when a file has none.
const SCAN_VERSION: i64 = 3;

/// Tracks written per transaction. A failed chunk rolls back as a unit;
/// chunks already committed stay, so an interrupted Scan resumes by mtime.
const CHUNK: usize = 200;

/// A file the scanner saw, before any tags are read.
pub struct Found {
    pub id: String,
    pub mtime: i64,
    /// Opaque to the store; handed back to `FolderScanner::read`.
    pub path: PathBuf,
}

/// What reading one file produced. The store adds the folder and mtime.
pub struct ScannedTrack {
    pub track: TrackSummary,
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
}

/// The seam between the store and the filesystem. The store calls `read`
/// only for files it judged new or changed, and holds no lock while it runs.
pub trait FolderScanner {
    /// Every audio file under `folder`. `Err` means the folder can't be read
    /// (an unmounted drive): the store then removes nothing from it.
    fn walk(&self, folder: &str) -> Result<Vec<Found>, String>;
    fn read(&self, folder: &str, found: &Found) -> Result<ScannedTrack, String>;
}

/// Outcome of `scan_all`. Folders that couldn't be reconciled are listed in
/// `failed`; the rest still count towards `summary`.
#[derive(Debug, Default)]
pub struct ScanReport {
    pub summary: ScanSummary,
    pub failed: Vec<(String, StoreError)>,
}

impl From<ScanSummary> for ScanReport {
    fn from(summary: ScanSummary) -> Self {
        ScanReport { summary, failed: Vec::new() }
    }
}

/// A Library folder and how many Tracks it currently holds.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderInfo {
    pub path: String,
    pub tracks: usize,
}

#[derive(Clone)]
pub struct LibraryStore {
    db: Db,
}

impl LibraryStore {
    pub fn new(db: Db) -> Self {
        LibraryStore { db }
    }

    // ---------- tracks ----------

    /// Every Track, newest first.
    pub fn list_tracks(&self) -> Result<Vec<Track>, StoreError> {
        self.db.read(list_tracks)
    }

    pub fn track_count(&self) -> Result<usize, StoreError> {
        self.db.read(|c| c.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0)))
            .map(|n| n as usize)
    }

    /// Every Track grouped into artists and albums.
    pub fn library(&self) -> Result<Library, StoreError> {
        Ok(library::build_library(&self.list_tracks()?))
    }

    /// Albums none of whose Tracks has a cover, for the cover fetch job.
    pub fn albums_missing_cover(&self) -> Result<Vec<MissingCover>, StoreError> {
        Ok(library::albums_missing_cover(&self.list_tracks()?)
            .into_iter()
            .map(|(artist, album, ids)| MissingCover { artist, album, ids })
            .collect())
    }

    /// Points Tracks at a cover (a `local:` path), overwriting any existing one.
    pub fn set_thumbnail(&self, ids: &[String], thumbnail_url: &str) -> Result<(), StoreError> {
        self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            for id in ids {
                tx.execute("UPDATE tracks SET thumbnail_url = ?2 WHERE id = ?1", params![id, thumbnail_url])?;
            }
            tx.commit()
        })
    }

    /// Wipes the whole library: every Track, every Library folder, and the
    /// analyses and translations saved for them, in one transaction. Files
    /// on disk are never touched. The scan version is left alone, so a
    /// folder added back afterwards is scanned incrementally as usual.
    pub fn clear(&self) -> Result<(), StoreError> {
        self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            for table in ["tracks", "library_folders", "audio_analysis", "lyrics_analysis", "lyrics_translation", "artist_pictures"] {
                tx.execute(&format!("DELETE FROM {table}"), [])?;
            }
            tx.commit()
        })
    }

    // ---------- Library folders ----------

    pub fn list_folders(&self) -> Result<Vec<String>, StoreError> {
        self.db.with(|c| {
            let mut stmt = c.prepare("SELECT path FROM library_folders ORDER BY added_at")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()
        })
    }

    /// Every Library folder with the number of Tracks it holds, oldest first.
    pub fn folder_stats(&self) -> Result<Vec<FolderInfo>, StoreError> {
        self.db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT f.path, COUNT(t.id) FROM library_folders f
                 LEFT JOIN tracks t ON t.root = f.path
                 GROUP BY f.path ORDER BY f.added_at",
            )?;
            let rows = stmt.query_map([], |r| Ok(FolderInfo { path: r.get(0)?, tracks: r.get::<_, i64>(1)? as usize }))?;
            rows.collect::<rusqlite::Result<Vec<FolderInfo>>>()
        })
    }

    /// Forgets a folder and every Track scanned from it. Files on disk are
    /// never touched.
    pub fn remove_folder(&self, folder: &str) -> Result<(), StoreError> {
        self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let ids: Vec<String> = {
                let mut stmt = tx.prepare("SELECT id FROM tracks WHERE root = ?1")?;
                let rows = stmt.query_map(params![folder], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            delete_tracks(&tx, &ids)?;
            tx.execute("DELETE FROM library_folders WHERE path = ?1", params![folder])?;
            tx.commit()
        })
    }

    // ---------- Scan ----------

    /// Adds a Library folder (a no-op if already there) and scans it. A
    /// folder that can't be read is refused and not registered. Does not
    /// touch the scan version: one folder can't prove the rest were read.
    pub fn add_folder(&self, folder: &str, scanner: &dyn FolderScanner) -> Result<ScanSummary, StoreError> {
        let found = scanner.walk(folder).map_err(StoreError::Scanner)?;
        self.db.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO library_folders (path, added_at) VALUES (?1, ?2)",
                params![folder, chrono::Utc::now().to_rfc3339()],
            )
        })?;
        self.reconcile(folder, found, scanner)
    }

    /// Scans every Library folder, summing the results. A folder that fails
    /// is reported in `failed` and doesn't stop the others. The scan version
    /// becomes current only if every folder succeeded; otherwise the next
    /// `scan_all` re-reads everything.
    pub fn scan_all(&self, scanner: &dyn FolderScanner) -> Result<ScanReport, StoreError> {
        let mut report = ScanReport::default();
        for folder in self.list_folders()? {
            let result = scanner
                .walk(&folder)
                .map_err(StoreError::Scanner)
                .and_then(|found| self.reconcile(&folder, found, scanner));
            match result {
                Ok(s) => {
                    report.summary.added += s.added;
                    report.summary.updated += s.updated;
                    report.summary.removed += s.removed;
                    report.summary.total += s.total;
                }
                Err(e) => {
                    log::warn!("library rescan skipped {folder}: {e}");
                    report.failed.push((folder, e));
                }
            }
        }
        if report.failed.is_empty() {
            self.db.with(|c| set_scan_version(c, SCAN_VERSION))?;
        }
        Ok(report)
    }

    /// Brings one folder's rows in line with what the scanner found: reads
    /// only new or changed files (all of them if the scan version is stale),
    /// writes them in chunks, then drops rows for files that are gone.
    fn reconcile(&self, folder: &str, found: Vec<Found>, scanner: &dyn FolderScanner) -> Result<ScanSummary, StoreError> {
        let (known, stale) = self.db.with(|c| {
            Ok::<_, rusqlite::Error>((library_mtimes(c, folder)?, scan_version(c) < SCAN_VERSION))
        })?;
        let mut summary = ScanSummary { total: found.len(), ..Default::default() };
        let mut seen = HashSet::with_capacity(found.len());
        let mut pending: Vec<(i64, ScannedTrack)> = Vec::new();

        for f in &found {
            seen.insert(f.id.as_str());
            if !stale && known.get(&f.id) == Some(&f.mtime) {
                continue;
            }
            let scanned = scanner.read(folder, f).map_err(StoreError::Scanner)?;
            pending.push((f.mtime, scanned));
            if pending.len() >= CHUNK {
                self.flush(folder, &mut pending, &mut summary)?;
            }
        }
        self.flush(folder, &mut pending, &mut summary)?;

        // Only a folder the scanner could walk gets here, so a vanished
        // drive never empties its tracks.
        let gone: Vec<String> = known.into_keys().filter(|id| !seen.contains(id.as_str())).collect();
        for chunk in gone.chunks(CHUNK) {
            self.db.with(|c| {
                let tx = c.unchecked_transaction()?;
                delete_tracks(&tx, chunk)?;
                tx.commit()
            })?;
        }
        summary.removed = gone.len();
        Ok(summary)
    }

    /// Writes `pending` as one transaction; counts only once it commits.
    fn flush(&self, folder: &str, pending: &mut Vec<(i64, ScannedTrack)>, summary: &mut ScanSummary) -> Result<(), StoreError> {
        if pending.is_empty() {
            return Ok(());
        }
        let (added, updated) = self.db.with(|c| {
            let tx = c.unchecked_transaction()?;
            let (mut added, mut updated) = (0, 0);
            for (mtime, scanned) in pending.iter() {
                if upsert_scanned(&tx, folder, *mtime, scanned)? {
                    added += 1;
                } else {
                    updated += 1;
                }
            }
            tx.commit()?;
            Ok::<_, rusqlite::Error>((added, updated))
        })?;
        summary.added += added;
        summary.updated += updated;
        pending.clear();
        Ok(())
    }
}

// ---------- SQL ----------

fn row_to_track(row: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        duration_ms: row.get(4)?,
        thumbnail_url: row.get(5)?,
        added_at: row.get(6)?,
        album_artist: row.get(7)?,
        track_no: row.get(8)?,
        disc_no: row.get(9)?,
        year: row.get(10)?,
    })
}

const TRACK_COLUMNS: &str = "id, title, artist, album, duration_ms, thumbnail_url, added_at, album_artist, track_no, disc_no, year";

fn get_track(conn: &Connection, id: &str) -> rusqlite::Result<Option<Track>> {
    conn.query_row(
        &format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1"),
        params![id],
        row_to_track,
    )
    .optional()
}

fn list_tracks(conn: &Connection) -> rusqlite::Result<Vec<Track>> {
    let mut stmt = conn.prepare(&format!("SELECT {TRACK_COLUMNS} FROM tracks ORDER BY added_at DESC"))?;
    let rows = stmt.query_map([], row_to_track)?;
    rows.collect()
}

/// Inserts a scanned file, or refreshes every scanned field if its
/// `id` (the `local:` path) is already there. Returns true when the
/// row was new. A scan that finds no cover keeps the one already set.
fn upsert_scanned(conn: &Connection, root: &str, mtime: i64, scanned: &ScannedTrack) -> rusqlite::Result<bool> {
    let t = &scanned.track;
    let existed = get_track(conn, &t.id)?.is_some();
    if existed {
        conn.execute(
            "UPDATE tracks SET title = ?2, artist = ?3, album = ?4, duration_ms = ?5, thumbnail_url = COALESCE(?6, thumbnail_url),
                album_artist = ?7, track_no = ?8, disc_no = ?9, year = ?10, root = ?11, mtime = ?12
             WHERE id = ?1",
            params![
                t.id, t.title, t.artist, t.album, t.duration_ms, t.thumbnail_url,
                scanned.album_artist, scanned.track_no, scanned.disc_no, scanned.year, root, mtime
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO tracks (id, title, artist, album, duration_ms, thumbnail_url, added_at,
                album_artist, track_no, disc_no, year, root, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                t.id, t.title, t.artist, t.album,
                t.duration_ms, t.thumbnail_url, chrono::Utc::now().to_rfc3339(),
                scanned.album_artist, scanned.track_no, scanned.disc_no, scanned.year, root, mtime
            ],
        )?;
    }
    Ok(!existed)
}

/// `id -> mtime` for every file previously scanned from `root`.
fn library_mtimes(conn: &Connection, root: &str) -> rusqlite::Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare("SELECT id, COALESCE(mtime, -1) FROM tracks WHERE root = ?1")?;
    let rows = stmt.query_map(params![root], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect()
}

/// Which version of the scanner last completed a full pass over every folder.
fn scan_version(conn: &Connection) -> i64 {
    conn.query_row("SELECT value FROM settings WHERE key = 'scan_version'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn set_scan_version(conn: &Connection, version: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('scan_version', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![version.to_string()],
    )?;
    Ok(())
}

/// Drops Tracks (and their analyses) for files that no longer exist.
fn delete_tracks(conn: &Connection, ids: &[String]) -> rusqlite::Result<()> {
    for id in ids {
        conn.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM audio_analysis WHERE track_id = ?1", params![id])?;
        conn.execute("DELETE FROM lyrics_analysis WHERE track_id = ?1", params![id])?;
        conn.execute("DELETE FROM lyrics_translation WHERE track_id = ?1", params![id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn store() -> LibraryStore {
        LibraryStore::new(Db::in_memory().unwrap())
    }

    fn version(store: &LibraryStore) -> i64 {
        store.db.with(|c| Ok::<_, rusqlite::Error>(scan_version(c))).unwrap()
    }

    fn set_version(store: &LibraryStore, v: i64) {
        store.db.with(|c| set_scan_version(c, v)).unwrap();
    }

    /// A row as an earlier version of the app left it, with a chosen timestamp.
    fn insert_track(store: &LibraryStore, id: &str, added_at: &str) {
        store
            .db
            .with(|c| {
                c.execute(
                    "INSERT INTO tracks (id, title, artist, added_at) VALUES (?1, 'A Song', 'An Artist', ?2)",
                    params![id, added_at],
                )
            })
            .unwrap();
    }

    fn sample_track(id: &str) -> TrackSummary {
        TrackSummary {
            id: id.to_string(),
            title: "A Test Song".to_string(),
            artist: "Test Artist".to_string(),
            album: Some("Test Album".to_string()),
            duration_ms: Some(180_000),
            thumbnail_url: Some("https://example.com/thumb.jpg".to_string()),
        }
    }

    /// Scripted filesystem: per folder, either a walk error or `(name, mtime)`
    /// files. Records every `read` so tests can assert what was re-read.
    #[derive(Default)]
    struct FakeScanner {
        folders: RefCell<HashMap<String, Result<Vec<(String, i64)>, String>>>,
        reads: RefCell<Vec<String>>,
        cover: RefCell<Option<String>>,
    }

    impl FakeScanner {
        fn with(folder: &str, files: &[(&str, i64)]) -> Self {
            let s = FakeScanner::default();
            s.set(folder, files);
            s
        }
        fn set(&self, folder: &str, files: &[(&str, i64)]) {
            self.folders
                .borrow_mut()
                .insert(folder.to_string(), Ok(files.iter().map(|(n, m)| (n.to_string(), *m)).collect()));
        }
        fn unmount(&self, folder: &str) {
            self.folders.borrow_mut().insert(folder.to_string(), Err(format!("{folder} is not available")));
        }
        fn take_reads(&self) -> Vec<String> {
            std::mem::take(&mut *self.reads.borrow_mut())
        }
        fn id(folder: &str, name: &str) -> String {
            crate::local_ref::encode(format!("{folder}/{name}"))
        }
    }

    impl FolderScanner for FakeScanner {
        fn walk(&self, folder: &str) -> Result<Vec<Found>, String> {
            self.folders.borrow()[folder].clone().map(|files| {
                files
                    .into_iter()
                    .map(|(name, mtime)| Found {
                        id: FakeScanner::id(folder, &name),
                        mtime,
                        path: PathBuf::from(format!("{folder}/{name}")),
                    })
                    .collect()
            })
        }
        fn read(&self, _folder: &str, found: &Found) -> Result<ScannedTrack, String> {
            self.reads.borrow_mut().push(found.id.clone());
            let name = found.path.file_name().unwrap().to_string_lossy().to_string();
            let mut track = sample_track(&found.id);
            track.title = name;
            track.thumbnail_url = self.cover.borrow().clone();
            Ok(ScannedTrack { track, album_artist: Some("Band".into()), track_no: Some(2), disc_no: Some(1), year: Some(2001) })
        }
    }

    fn titles(store: &LibraryStore) -> Vec<String> {
        let mut t: Vec<_> = store.list_tracks().unwrap().into_iter().map(|t| t.title).collect();
        t.sort();
        t
    }

    // ---------- tracks ----------

    #[test]
    fn tracks_list_newest_first() {
        let store = store();
        insert_track(&store, "vid1", "2026-01-01T00:00:00Z");
        insert_track(&store, "vid2", "2099-01-01T00:00:00Z");
        let tracks = store.list_tracks().unwrap();
        assert_eq!((tracks[0].id.as_str(), tracks[1].id.as_str()), ("vid2", "vid1"));
    }

    // ---------- folders ----------

    #[test]
    fn folder_stats_count_each_folders_tracks() {
        let store = store();
        let scanner = FakeScanner::with("/a", &[("one.mp3", 1), ("two.mp3", 1)]);
        scanner.set("/b", &[]);
        store.add_folder("/a", &scanner).unwrap();
        store.add_folder("/b", &scanner).unwrap();
        assert_eq!(
            store.folder_stats().unwrap(),
            vec![FolderInfo { path: "/a".into(), tracks: 2 }, FolderInfo { path: "/b".into(), tracks: 0 }]
        );
    }

    #[test]
    fn adding_the_same_folder_twice_is_a_no_op() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[]);
        store.add_folder("/m", &scanner).unwrap();
        store.add_folder("/m", &scanner).unwrap();
        assert_eq!(store.list_folders().unwrap(), vec!["/m".to_string()]);
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_not_registered() {
        let store = store();
        let scanner = FakeScanner::default();
        scanner.unmount("/gone");
        assert!(matches!(store.add_folder("/gone", &scanner), Err(StoreError::Scanner(_))));
        assert!(store.list_folders().unwrap().is_empty());
    }

    #[test]
    fn removing_a_folder_drops_only_its_tracks_and_their_analyses() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1)]);
        scanner.set("/n", &[("b.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();
        store.add_folder("/n", &scanner).unwrap();
        store
            .db
            .with(|c| {
                c.execute("INSERT INTO audio_analysis (track_id, bass_db, mid_db, treble_db, overall_db, created_at) VALUES ('local:/m/a.flac', 0, 0, 0, 0, 'x')", [])
            })
            .unwrap();

        store.remove_folder("/m").unwrap();

        assert_eq!(store.list_folders().unwrap(), vec!["/n".to_string()]);
        assert_eq!(titles(&store), vec!["b.flac".to_string()]);
        let analyses: i64 = store.db.with(|c| c.query_row("SELECT COUNT(*) FROM audio_analysis", [], |r| r.get(0))).unwrap();
        assert_eq!(analyses, 0);
    }

    // ---------- Scan ----------

    #[test]
    fn a_changed_file_is_refreshed_in_place_and_an_unchanged_one_is_not_read() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 10), ("b.flac", 10)]);
        let first = store.add_folder("/m", &scanner).unwrap();
        assert_eq!((first.added, first.updated), (2, 0));
        store.scan_all(&scanner).unwrap(); // brings the library up to the current scan version
        scanner.take_reads();

        scanner.set("/m", &[("a.flac", 20), ("b.flac", 10)]);
        let report = store.scan_all(&scanner).unwrap();

        assert_eq!((report.summary.added, report.summary.updated, report.summary.total), (0, 1, 2));
        assert_eq!(scanner.take_reads(), vec!["local:/m/a.flac".to_string()]);
        let row = store.list_tracks().unwrap().into_iter().find(|t| t.title == "a.flac").unwrap();
        assert_eq!((row.track_no, row.year), (Some(2), Some(2001)));
        assert_eq!(store.track_count().unwrap(), 2);
    }

    #[test]
    fn a_stale_scan_version_rereads_every_file_once() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 10), ("b.flac", 10)]);
        store.add_folder("/m", &scanner).unwrap();
        assert_eq!(version(&store), 0, "add_folder never marks the version");
        scanner.take_reads();

        let upgrade = store.scan_all(&scanner).unwrap();
        assert_eq!((upgrade.summary.added, upgrade.summary.updated), (0, 2));
        assert_eq!(scanner.take_reads().len(), 2);
        assert_eq!(version(&store), SCAN_VERSION);

        store.scan_all(&scanner).unwrap();
        assert!(scanner.take_reads().is_empty());
    }

    #[test]
    fn a_vanished_file_is_removed() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1), ("b.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();

        scanner.set("/m", &[("b.flac", 1)]);
        let report = store.scan_all(&scanner).unwrap();

        assert_eq!((report.summary.removed, report.summary.total), (1, 1));
        assert_eq!(titles(&store), vec!["b.flac".to_string()]);
    }

    #[test]
    fn an_unreadable_folder_keeps_its_tracks_and_leaves_the_version_alone() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();
        store.scan_all(&scanner).unwrap();
        assert_eq!(version(&store), SCAN_VERSION);
        set_version(&store, SCAN_VERSION - 1);

        scanner.unmount("/m");
        let report = store.scan_all(&scanner).unwrap();

        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.summary.removed, 0);
        assert_eq!(store.track_count().unwrap(), 1);
        assert_eq!(version(&store), SCAN_VERSION - 1, "a skipped folder must not mark the scan complete");
    }

    #[test]
    fn one_failed_folder_does_not_stop_the_others_or_mark_the_version() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1)]);
        scanner.set("/n", &[("b.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();
        store.add_folder("/n", &scanner).unwrap();

        scanner.unmount("/m");
        scanner.set("/n", &[("b.flac", 1), ("c.flac", 1)]);
        let report = store.scan_all(&scanner).unwrap();

        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, "/m");
        assert_eq!(report.summary.added, 1);
        assert_eq!(titles(&store), vec!["a.flac", "b.flac", "c.flac"]);
        assert_eq!(version(&store), 0);
    }

    #[test]
    fn a_rescan_that_finds_no_cover_keeps_the_one_already_set() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();
        store.set_thumbnail(&["local:/m/a.flac".to_string()], "local:/covers/x.jpg").unwrap();

        scanner.set("/m", &[("a.flac", 2)]);
        store.scan_all(&scanner).unwrap();

        let row = store.list_tracks().unwrap().remove(0);
        assert_eq!(row.thumbnail_url.as_deref(), Some("local:/covers/x.jpg"));
    }

    fn many(n: usize) -> Vec<(String, i64)> {
        (0..n).map(|i| (format!("t{i:03}"), 1)).collect()
    }

    fn set_many(scanner: &FakeScanner, n: usize) {
        let files = many(n);
        let refs: Vec<(&str, i64)> = files.iter().map(|(s, m)| (s.as_str(), *m)).collect();
        scanner.set("/m", &refs);
    }

    #[test]
    fn a_failed_chunk_rolls_back_alone_and_the_next_scan_resumes_where_it_stopped() {
        let store = store();
        let scanner = FakeScanner::default();
        set_many(&scanner, 450);
        set_version(&store, SCAN_VERSION); // so resuming is by mtime, not a full re-read
        // The write of t300 (second chunk) fails.
        store
            .db
            .with(|c| {
                c.execute_batch(
                    "CREATE TRIGGER boom BEFORE INSERT ON tracks WHEN NEW.title = 't300'
                     BEGIN SELECT RAISE(ABORT, 'boom'); END;",
                )
            })
            .unwrap();

        let err = store.add_folder("/m", &scanner).unwrap_err();
        assert!(matches!(err, StoreError::Sqlite(_)));
        assert_eq!(store.track_count().unwrap(), 200, "first chunk committed, failed chunk rolled back");

        store.db.with(|c| c.execute_batch("DROP TRIGGER boom")).unwrap();
        scanner.take_reads();
        let resumed = store.add_folder("/m", &scanner).unwrap();

        assert_eq!((resumed.added, resumed.updated, resumed.total), (250, 0, 450));
        assert_eq!(scanner.take_reads().len(), 250, "the 200 committed files are not read again");
        assert_eq!(store.track_count().unwrap(), 450);
    }

    #[test]
    fn a_scanner_failure_on_one_file_fails_the_folder() {
        struct Broken;
        impl FolderScanner for Broken {
            fn walk(&self, _: &str) -> Result<Vec<Found>, String> {
                Ok(vec![Found { id: "local:/m/a".into(), mtime: 1, path: PathBuf::from("/m/a") }])
            }
            fn read(&self, _: &str, _: &Found) -> Result<ScannedTrack, String> {
                Err("unreadable".into())
            }
        }
        let store = store();
        assert!(matches!(store.add_folder("/m", &Broken), Err(StoreError::Scanner(_))));
        assert_eq!(store.track_count().unwrap(), 0);
    }

    // ---------- clear ----------

    #[test]
    fn clearing_wipes_tracks_folders_and_every_analysis_but_not_the_scan_version() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();
        insert_track(&store, "vid1", "2026-01-01T00:00:00Z");
        store.scan_all(&scanner).unwrap();
        store
            .db
            .with(|c| {
                c.execute_batch(
                    "INSERT INTO audio_analysis (track_id, bass_db, mid_db, treble_db, overall_db, created_at)
                         VALUES ('vid1', 0, 0, 0, 0, 'x');
                     INSERT INTO lyrics_analysis (track_id, vibe_summary, annotations, lines, model, created_at)
                         VALUES ('vid1', 'Upbeat.', '[]', '[]', 'qwen3:8b', 'x');
                     INSERT INTO lyrics_translation (track_id, lines, translations, model, created_at)
                         VALUES ('vid1', '[]', '[]', 'qwen3:8b', 'x');",
                )
            })
            .unwrap();
        let rows = |table: &str| -> i64 {
            store.db.with(|c| c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))).unwrap()
        };
        assert_eq!((rows("tracks"), rows("library_folders")), (2, 1));
        assert_eq!((rows("audio_analysis"), rows("lyrics_analysis"), rows("lyrics_translation")), (1, 1, 1));

        store.clear().unwrap();

        for table in ["tracks", "library_folders", "audio_analysis", "lyrics_analysis", "lyrics_translation"] {
            assert_eq!(rows(table), 0, "{table}");
        }
        assert_eq!(version(&store), SCAN_VERSION);
        assert!(store.list_folders().unwrap().is_empty());
    }

    // ---------- grouping ----------

    #[test]
    fn the_library_groups_scanned_tracks_and_lists_albums_without_a_cover() {
        let store = store();
        let scanner = FakeScanner::with("/m", &[("a.flac", 1), ("b.flac", 1)]);
        store.add_folder("/m", &scanner).unwrap();

        let library = store.library().unwrap();
        assert_eq!(library.track_count(), 2);
        assert_eq!(library.albums.len(), 1);

        let missing = store.albums_missing_cover().unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].album, "Test Album");
        assert_eq!(missing[0].ids.len(), 2);

        *scanner.cover.borrow_mut() = Some("local:/covers/x.jpg".to_string());
        scanner.set("/m", &[("a.flac", 2), ("b.flac", 2)]);
        store.scan_all(&scanner).unwrap();
        assert!(store.albums_missing_cover().unwrap().is_empty());
    }
}
