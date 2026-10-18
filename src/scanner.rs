//! The filesystem side of a Scan: walks a Library folder, reads tags and
//! cover art, and hands the results to the Library store through
//! `FolderScanner`. The store decides what is new, changed or gone.

use crate::library_store::{FolderScanner, Found, ScannedTrack};
use crate::models::TrackSummary;
use crate::{audio, local_ref, path_tags};

/// Audio file extensions recognized when scanning a local folder (and when
/// browsing folders to add) - kept in one place so they can't drift apart.
pub const LOCAL_AUDIO_EXTENSIONS: &[&str] = &["flac", "wav", "mp3", "m4a", "aac"];

static STORAGE_READABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Android hides audio files from an app without the media permission but
/// still lets it list the folder, so a Scan would read as "every file was
/// deleted" and the store would drop every Track. While this is `false`
/// every Scan is refused instead. Always `true` elsewhere.
pub fn set_storage_readable(readable: bool) {
    STORAGE_READABLE.store(readable, std::sync::atomic::Ordering::SeqCst);
}

/// The production `FolderScanner`: real files, real tags.
pub struct FsScanner;

impl FolderScanner for FsScanner {
    /// A folder that isn't a directory (an unmounted drive, say) is an
    /// error, so the store won't treat its absence as "all files deleted".
    fn walk(&self, folder: &str) -> Result<Vec<Found>, String> {
        if !STORAGE_READABLE.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(format!("{folder} can't be read: access to music was not allowed"));
        }
        let root = std::path::Path::new(folder);
        if !root.is_dir() {
            return Err(format!("{folder} is not available"));
        }
        // A directory we may not list (Android without the media permission)
        // would read as empty, and the store would then drop every Track in it.
        if let Err(e) = std::fs::read_dir(root) {
            return Err(format!("{folder} can't be read: {e}"));
        }
        Ok(scan_audio_folder(root)
            .into_iter()
            .map(|path| {
                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                Found { id: local_ref::encode(&path), mtime, path }
            })
            .collect())
    }

    fn read(&self, folder: &str, found: &Found) -> Result<ScannedTrack, String> {
        let tags = audio::read_local_track_tags(&found.path);
        // Embedded art first, then a cover file in the track's folder.
        let cover_path = tags
            .embedded_cover
            .as_ref()
            .and_then(cache_embedded_cover)
            .or_else(|| found.path.parent().and_then(find_local_cover_art));
        Ok(resolve_track(std::path::Path::new(folder), &found.path, tags, cover_path))
    }
}

/// Recursively walks `root` collecting paths with a recognized audio
/// extension (`LOCAL_AUDIO_EXTENSIONS`). Runs on a blocking thread (see
/// caller) since it's synchronous file I/O. A directory that fails to read
/// (permissions, a broken symlink, etc.) is skipped entirely rather than
/// failing the whole scan.
fn scan_audio_folder(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_audio = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| LOCAL_AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
                .unwrap_or(false);
            if is_audio {
                found.push(path);
            }
        }
    }

    found
}

/// Filenames (case-insensitive, checked in this order) recognized as
/// album/folder artwork sitting alongside a track's audio files - the
/// common convention for ripped/downloaded music libraries that ship
/// artwork as a separate image rather than (or in addition to) embedding
/// it in each file's own tags.
const LOCAL_COVER_STEMS: &[&str] = &["cover", "folder", "front", "album", "albumart", "artwork"];
const LOCAL_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Looks for a recognized cover-art file directly inside `dir` (a track's
/// own parent folder - not recursive, so a cover several levels up doesn't
/// get matched against every track under it). `None` on any read error or
/// if nothing matches, same "best effort, never fatal" spirit as tag
/// reading.
fn find_local_cover_art(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut matches: Vec<(usize, std::path::PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_lowercase();
            let ext = p.extension()?.to_str()?;
            if !LOCAL_COVER_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
                return None;
            }
            // Earlier stems win, so "cover" beats "album" if both exist.
            let rank = LOCAL_COVER_STEMS.iter().position(|s| *s == stem)?;
            Some((rank, p))
        })
        .collect();
    matches.sort();
    matches.into_iter().next().map(|(_, p)| p)
}

/// Writes an embedded cover into the app's cover cache, named by a hash of
/// its bytes so every track of an album (which all carry the same image)
/// shares one file. Returns the stored path.
pub fn cache_embedded_cover(cover: &audio::EmbeddedCover) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cover.data.hash(&mut hasher);
    let dir = crate::app::app_data_dir().join("covers");
    let path = dir.join(format!("{:016x}.{}", hasher.finish(), cover.ext));
    if !path.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, &cover.data).ok()?;
    }
    Some(path)
}

/// Everything the scan knows about one file, from its own tags, where it
/// sits under the Library folder `root`, and the cover found for it. Pure,
/// so the precedence is testable without audio files:
///
/// 1. tags written in the file win;
/// 2. a field the tags lack is guessed from the folder layout
///    (`path_tags::infer`);
/// 3. what is still missing falls back to the filename (title) and
///    `"Unknown Artist"` - lyrics lookup needs a real artist name to have
///    any chance of matching, so this is worth doing even though it's
///    best-effort.
///
/// The id and the cover are Local references (see `local_ref`); the UI loads
/// the cover from disk through the `tq://` file route.
fn resolve_track(
    root: &std::path::Path,
    path: &std::path::Path,
    mut tags: audio::LocalTrackTags,
    cover_path: Option<std::path::PathBuf>,
) -> ScannedTrack {
    tags.fill_missing(path_tags::infer(root, path));

    let title = tags.title.take().unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string())
    });
    let artist = tags.artist.take().unwrap_or_else(|| "Unknown Artist".to_string());

    ScannedTrack {
        track: TrackSummary {
            id: local_ref::encode(path),
            title,
            artist,
            album: tags.album,
            duration_ms: tags.duration_ms,
            thumbnail_url: cover_path.map(local_ref::encode),
        },
        album_artist: tags.album_artist,
        track_no: tags.track_no,
        disc_no: tags.disc_no,
        year: tags.year,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tanqi-test-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("should create temp dir");
        dir
    }

    #[test]
    fn scan_finds_audio_files_recursively_and_ignores_others() {
        let root = temp_dir("scan-recursive");
        let nested = root.join("Artist").join("Album");
        std::fs::create_dir_all(&nested).unwrap();

        std::fs::write(root.join("top-level.mp3"), b"fake").unwrap();
        std::fs::write(nested.join("track.flac"), b"fake").unwrap();
        std::fs::write(nested.join("cover.jpg"), b"fake").unwrap();
        std::fs::write(nested.join("notes.txt"), b"fake").unwrap();

        let found = scan_audio_folder(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|p| p.file_name().unwrap() == "top-level.mp3"));
        assert!(found.iter().any(|p| p.file_name().unwrap() == "track.flac"));
    }

    #[test]
    fn scan_extension_matching_is_case_insensitive() {
        let root = temp_dir("scan-case");
        std::fs::write(root.join("shout.MP3"), b"fake").unwrap();

        let found = scan_audio_folder(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(found.len(), 1);
    }

    #[test]
    fn scan_of_a_nonexistent_folder_returns_empty_rather_than_panicking() {
        let found = scan_audio_folder(std::path::Path::new("/nonexistent/tanqi-test-path"));
        assert!(found.is_empty());
    }

    fn resolve(root: &str, path: &str, tags: audio::LocalTrackTags, cover: Option<&str>) -> ScannedTrack {
        resolve_track(
            std::path::Path::new(root),
            std::path::Path::new(path),
            tags,
            cover.map(std::path::PathBuf::from),
        )
    }

    #[test]
    fn tags_in_the_file_win_over_everything_else() {
        let tags = audio::LocalTrackTags {
            title: Some("Real Title".to_string()),
            artist: Some("Real Artist".to_string()),
            album: Some("Real Album".to_string()),
            album_artist: Some("Real Band".to_string()),
            track_no: Some(7),
            disc_no: Some(2),
            year: Some(1999),
            duration_ms: Some(210_000),
            ..Default::default()
        };
        // The folder layout says something different on every point.
        let scanned = resolve("/lib", "/lib/Folder Artist/2014 - Folder Album/03 - Folder Song.mp3", tags, None);

        let t = &scanned.track;
        assert_eq!(t.id, "local:/lib/Folder Artist/2014 - Folder Album/03 - Folder Song.mp3");
        assert_eq!((t.title.as_str(), t.artist.as_str()), ("Real Title", "Real Artist"));
        assert_eq!(t.album.as_deref(), Some("Real Album"));
        assert_eq!(t.duration_ms, Some(210_000));
        assert_eq!(t.thumbnail_url, None);
        assert_eq!(scanned.album_artist.as_deref(), Some("Real Band"));
        assert_eq!((scanned.track_no, scanned.disc_no, scanned.year), (Some(7), Some(2), Some(1999)));
    }

    #[test]
    fn an_untagged_file_is_described_by_where_it_sits() {
        let scanned = resolve("/lib", "/lib/Siti Nurhaliza 1996-2014/2014 - Fragmen/03 - Song.flac", Default::default(), None);

        let t = &scanned.track;
        assert_eq!((t.title.as_str(), t.artist.as_str()), ("Song", "Siti Nurhaliza"));
        assert_eq!(t.album.as_deref(), Some("Fragmen"));
        assert_eq!((scanned.track_no, scanned.year), (Some(3), Some(2014)));
    }

    #[test]
    fn a_missing_tag_is_filled_from_the_path_without_touching_the_ones_present() {
        let tags = audio::LocalTrackTags { title: Some("Tagged".to_string()), ..Default::default() };
        let scanned = resolve("/lib", "/lib/Artist/Album/04 - Other.flac", tags, None);

        assert_eq!(scanned.track.title, "Tagged");
        assert_eq!(scanned.track.artist, "Artist");
        assert_eq!(scanned.track.album.as_deref(), Some("Album"));
        assert_eq!(scanned.track_no, Some(4));
    }

    #[test]
    fn a_file_at_the_library_root_falls_back_to_its_name_and_unknown_artist() {
        let scanned = resolve("/music", "/music/My Cool Song.flac", Default::default(), None);

        assert_eq!(scanned.track.id, "local:/music/My Cool Song.flac");
        assert_eq!(scanned.track.title, "My Cool Song");
        assert_eq!(scanned.track.artist, "Unknown Artist");
        assert_eq!(scanned.track.album, None);
    }

    #[test]
    fn a_found_cover_becomes_a_local_reference_like_the_track_id() {
        let scanned = resolve("/music", "/music/song.mp3", Default::default(), Some("/music/cover.jpg"));
        assert_eq!(scanned.track.thumbnail_url, Some("local:/music/cover.jpg".to_string()));
    }

    #[test]
    fn finds_cover_art_case_insensitively_among_recognized_filenames() {
        let dir = temp_dir("find-cover");
        std::fs::write(dir.join("Folder.JPG"), b"fake").unwrap();
        std::fs::write(dir.join("track.mp3"), b"fake").unwrap();

        let found = find_local_cover_art(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(found, Some(dir.join("Folder.JPG")));
    }

    #[test]
    fn an_earlier_cover_name_beats_a_later_one() {
        let dir = temp_dir("cover-rank");
        std::fs::write(dir.join("album.png"), b"fake").unwrap();
        std::fs::write(dir.join("Front.webp"), b"fake").unwrap();
        std::fs::write(dir.join("cover.jpg"), b"fake").unwrap();

        let found = find_local_cover_art(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(found, Some(dir.join("cover.jpg")));
    }

    #[test]
    fn embedded_covers_are_cached_once_per_distinct_image() {
        let cover = audio::EmbeddedCover { ext: "png", data: vec![1, 2, 3, 4, 5, 6, 7, 8] };
        let first = cache_embedded_cover(&cover).expect("should write");
        let second = cache_embedded_cover(&cover).expect("should reuse");
        assert_eq!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), cover.data);
        assert_eq!(first.extension().unwrap(), "png");
        let _ = std::fs::remove_file(&first);
    }

    #[test]
    fn no_cover_art_found_when_none_of_the_recognized_names_are_present() {
        let dir = temp_dir("no-cover");
        std::fs::write(dir.join("track.mp3"), b"fake").unwrap();
        std::fs::write(dir.join("booklet.jpg"), b"fake").unwrap(); // not a recognized name

        let found = find_local_cover_art(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(found, None);
    }

    /// The real scanner feeding the real store (in-memory SQLite, no
    /// process-wide handle): scan, upgrade rescan, no-op rescan, new file,
    /// deleted file, vanished folder. Files are fake, so tags fall back to
    /// the filename - what's under test is the incremental bookkeeping.
    #[test]
    fn scanning_adds_new_files_skips_unchanged_ones_and_drops_deleted_ones() {
        use crate::db::Db;
        use crate::library_store::LibraryStore;

        let store = LibraryStore::new(Db::in_memory().unwrap());
        let root = temp_dir("scan-lib");
        std::fs::write(root.join("a.mp3"), b"fake").unwrap();
        std::fs::write(root.join("b.flac"), b"fake").unwrap();
        let folder = root.to_string_lossy().to_string();
        let counts = |s: &crate::models::ScanSummary| (s.added, s.updated, s.removed, s.total);

        let first = store.add_folder(&folder, &FsScanner).unwrap();
        assert_eq!(counts(&first), (2, 0, 0, 2));

        // The first full rescan of a library scanned by an older scanner
        // version re-reads every file once...
        let upgrade = store.scan_all(&FsScanner).unwrap();
        assert_eq!(counts(&upgrade.summary), (0, 2, 0, 2));
        // ...and after that unchanged files are skipped.
        let again = store.scan_all(&FsScanner).unwrap();
        assert_eq!(counts(&again.summary), (0, 0, 0, 2));

        std::fs::write(root.join("c.wav"), b"fake").unwrap();
        std::fs::remove_file(root.join("a.mp3")).unwrap();
        let changed = store.scan_all(&FsScanner).unwrap();
        assert_eq!((changed.summary.added, changed.summary.removed, changed.summary.total), (1, 1, 2));
        let titles: Vec<_> = store.list_tracks().unwrap().into_iter().map(|t| t.title).collect();
        assert!(titles.contains(&"b".to_string()) && titles.contains(&"c".to_string()) && !titles.contains(&"a".to_string()));

        // A folder that's gone (unmounted drive) must not wipe its tracks.
        std::fs::remove_dir_all(&root).unwrap();
        let missing = store.scan_all(&FsScanner).unwrap();
        assert_eq!(missing.summary.removed, 0);
        assert_eq!(missing.failed.len(), 1);
        assert_eq!(store.list_tracks().unwrap().len(), 2);

        store.remove_folder(&folder).unwrap();
        assert!(store.list_tracks().unwrap().is_empty());
    }
}
