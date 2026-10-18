//! Guesses missing tags from where a file sits in the library, for music
//! that was never tagged. Assumes the common `Artist/Album/NN - Title.ext`
//! layout; real tags always win over these guesses.

use crate::audio::LocalTrackTags;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

/// "Siti Nurhaliza 1996-2014" -> "Siti Nurhaliza": a trailing year or year range.
static TRAILING_YEARS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[\s\-_]*[\[(]?\d{4}(?:\s*-\s*\d{4})?[\])]?\s*$").unwrap());
/// "2014 - Fragmen" / "[2014] Fragmen" / "2014. Fragmen" -> year + "Fragmen".
static LEADING_YEAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\[(]?(\d{4})[\])]?\s*[-–._]?\s*(.+)$").unwrap());
/// "01 - Title" / "01. Title" / "01_Title" -> number + "Title". A bare space
/// isn't a separator, so "21 Guns" keeps its name.
static LEADING_NUMBER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\d{1,3})\s*[-._]\s*(.+)$").unwrap());

fn clean_artist(dir: &str) -> Option<String> {
    let cleaned = TRAILING_YEARS.replace(dir, "").trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Whatever can be inferred for `path`, a file somewhere under `root`.
/// Fields that can't be inferred stay `None`.
pub fn infer(root: &Path, path: &Path) -> LocalTrackTags {
    let mut guess = LocalTrackTags::default();

    let dirs: Vec<String> = path
        .parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|rel| rel.components().filter_map(|c| c.as_os_str().to_str().map(str::to_string)).collect())
        .unwrap_or_default();

    if let Some(album_dir) = dirs.last() {
        match LEADING_YEAR.captures(album_dir) {
            Some(c) => {
                guess.year = c[1].parse().ok();
                guess.album = Some(c[2].trim().to_string());
            }
            None => guess.album = Some(album_dir.trim().to_string()),
        }
    }
    // The folder above the album is the artist - only when there is one
    // below the library root, so loose "Album/track" files stay artist-less.
    if dirs.len() >= 2 {
        guess.artist = clean_artist(&dirs[dirs.len() - 2]);
    }

    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        match LEADING_NUMBER.captures(stem) {
            Some(c) => {
                guess.track_no = c[1].parse().ok();
                guess.title = Some(c[2].trim().to_string());
            }
            None => guess.title = Some(stem.trim().to_string()),
        }
    }
    guess
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess(rel: &str) -> LocalTrackTags {
        infer(Path::new("/m"), &Path::new("/m").join(rel))
    }

    #[test]
    fn artist_album_track_layout() {
        let g = guess("Siti Nurhaliza/Cindai/03 - Cindai.mp3");
        assert_eq!(g.artist.as_deref(), Some("Siti Nurhaliza"));
        assert_eq!(g.album.as_deref(), Some("Cindai"));
        assert_eq!(g.track_no, Some(3));
        assert_eq!(g.title.as_deref(), Some("Cindai"));
    }

    #[test]
    fn year_ranges_and_leading_album_years_are_split_off() {
        let g = guess("Full(1)/Siti Nurhaliza 1996-2014/2014 - Fragmen/Aku.mp3");
        assert_eq!(g.artist.as_deref(), Some("Siti Nurhaliza"));
        assert_eq!(g.album.as_deref(), Some("Fragmen"));
        assert_eq!(g.year, Some(2014));
        assert_eq!(g.title.as_deref(), Some("Aku"));
        assert_eq!(g.track_no, None);
    }

    #[test]
    fn a_title_starting_with_a_number_but_no_separator_is_left_alone() {
        let g = guess("A/B/21 Guns.mp3");
        assert_eq!(g.title.as_deref(), Some("21 Guns"));
        assert_eq!(g.track_no, None);
    }

    #[test]
    fn a_file_directly_under_one_folder_gets_an_album_but_no_artist() {
        let g = guess("Some Album/track.flac");
        assert_eq!(g.album.as_deref(), Some("Some Album"));
        assert_eq!(g.artist, None);
    }

    #[test]
    fn a_file_at_the_library_root_gets_only_a_title() {
        let g = guess("loose.mp3");
        assert_eq!((g.artist, g.album), (None, None));
        assert_eq!(g.title.as_deref(), Some("loose"));
    }
}
