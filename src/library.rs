//! Groups scanned tracks into a library: albums first, then the artists
//! that appear on them. Pure functions over `Track`, so the grouping
//! rules are unit-tested without a database or UI.
//!
//! An album is identified by its title plus where its files live, never by
//! track artist: a soundtrack or compilation has a different artist on
//! nearly every track but is still one album.

use crate::local_ref;
use crate::models::Track;
use std::collections::{HashMap, HashSet};

const UNKNOWN_ALBUM: &str = "Unknown Album";
pub const VARIOUS_ARTISTS: &str = "Various Artists";

/// An album's artist is the one on at least this share of its tracks;
/// below that it's a "Various Artists" compilation.
const DOMINANT_SHARE: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    /// Stable identity for navigation: title key plus folder.
    pub id: String,
    pub title: String,
    /// Who the album is filed under: the album-artist tag, else the artist
    /// on most of its tracks, else "Various Artists".
    pub artist: String,
    /// No single artist owns it (a compilation).
    pub various: bool,
    /// Only set on the copies listed under a guest artist: `tracks` is then
    /// just that artist's tracks, not the whole album.
    pub partial: bool,
    pub year: Option<i64>,
    pub cover: Option<String>,
    /// In play order: disc, then track number, then title.
    pub tracks: Vec<Track>,
    /// True when the disc tags look genuine, so the UI should label disc
    /// breaks. See `discs_are_real`.
    pub multi_disc: bool,
}

impl Album {
    pub fn duration_ms(&self) -> i64 {
        self.tracks.iter().filter_map(|t| t.duration_ms).sum()
    }

    /// Every artist on the album, those with the most tracks first.
    pub fn artists(&self) -> Vec<String> {
        let mut counts: Vec<(String, String, usize)> = vec![]; // (key, display, tracks)
        for name in self.tracks.iter().map(|t| t.artist.trim()).filter(|n| !n.is_empty()) {
            let key = group_key(name);
            match counts.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, _, n)) => *n += 1,
                None => counts.push((key, name.to_string(), 1)),
            }
        }
        // Stable sort: ties keep the order they first appear on the album.
        counts.sort_by(|a, b| b.2.cmp(&a.2));
        let mut names: Vec<String> = counts.into_iter().map(|(_, n, _)| n).collect();
        if !self.various && !names.iter().any(|n| group_key(n) == group_key(&self.artist)) {
            names.insert(0, self.artist.clone());
        }
        names
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Artist {
    pub name: String,
    /// Every album this artist is on, oldest first. Albums they don't own
    /// are `partial` copies holding only their own tracks.
    pub albums: Vec<Album>,
}

impl Artist {
    pub fn track_count(&self) -> usize {
        self.albums.iter().map(|a| a.tracks.len()).sum()
    }

    pub fn cover(&self) -> Option<&str> {
        self.albums.iter().find_map(|a| a.cover.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Library {
    /// Sorted by name, ignoring a leading "The".
    pub artists: Vec<Artist>,
    /// Each album once, grouped by artist then year.
    pub albums: Vec<Album>,
}

impl Library {
    pub fn track_count(&self) -> usize {
        self.albums.iter().map(|a| a.tracks.len()).sum()
    }

    /// Every track in the library, album by album.
    pub fn all_tracks(&self) -> Vec<Track> {
        self.albums.iter().flat_map(|a| a.tracks.iter().cloned()).collect()
    }
}

/// Lowercased, with a leading "the " dropped, so "The Beatles" sorts under B.
fn sort_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_prefix("the ").map(str::to_string).unwrap_or(lower)
}

/// Grouping key that ignores case, punctuation and spacing, so "That's" and
/// "That’s" (tagged by different tools) or "AC/DC" and "AC-DC" land in the
/// same artist or album. Falls back to the plain lowercase text for a name
/// with no letters or digits at all.
pub fn group_key(name: &str) -> String {
    let key: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if key.is_empty() {
        name.trim().to_lowercase()
    } else {
        key
    }
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// "CD 2" / "Disc 1" style folders hold part of an album, so the album's
/// folder is their parent.
fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    ["cd", "disc", "disk"].iter().any(|p| {
        n.strip_prefix(p)
            .map(|rest| rest.trim_start_matches([' ', '-', '_', '.']).chars().next().is_some_and(|c| c.is_ascii_digit()))
            .unwrap_or(false)
    })
}

fn album_folder(id: &str) -> String {
    let Some(path) = local_ref::decode(id) else {
        return String::new();
    };
    let path = std::path::Path::new(path);
    let mut dir = path.parent();
    if let Some(d) = dir {
        if d.file_name().and_then(|n| n.to_str()).is_some_and(is_disc_folder) {
            dir = d.parent();
        }
    }
    dir.map(|d| d.to_string_lossy().to_string()).unwrap_or_default()
}

/// Names a group of tracks by the spelling most of them use (first seen on
/// a tie), so one stray variant doesn't title the album.
fn most_common<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut counts: Vec<(&str, usize)> = vec![];
    for n in names {
        match counts.iter_mut().find(|(c, _)| *c == n) {
            Some((_, k)) => *k += 1,
            None => counts.push((n, 1)),
        }
    }
    counts.iter().max_by_key(|(_, k)| *k).map(|(n, _)| (*n).to_string()).unwrap_or_default()
}

/// Disc numbers are trusted only if some track number repeats across discs
/// (each disc restarting at 1). Files of one album often disagree about disc
/// ("1/1" in one, "2/2" in another, or a stray value); when track numbers
/// are all distinct, splitting by disc would only scramble the order.
fn discs_are_real(tracks: &[Track]) -> bool {
    let mut seen: HashMap<i64, i64> = HashMap::new();
    tracks.iter().any(|t| match t.track_no {
        Some(n) => {
            let disc = t.disc_no.unwrap_or(1);
            *seen.entry(n).or_insert(disc) != disc
        }
        None => false,
    })
}

/// Artist keys on a track: its own artist and the album-artist tag.
fn track_artist_keys(t: &Track) -> Vec<String> {
    let mut keys = vec![group_key(&t.artist)];
    if let Some(aa) = non_empty(&t.album_artist) {
        keys.push(group_key(aa));
    }
    keys
}

/// Splits the tracks sharing one album title into albums. Everything in one
/// folder is one album (a compilation). Different folders are the same album
/// only if they share an artist (the same record ripped twice); otherwise
/// they're unrelated albums that happen to share a title ("Greatest Hits").
fn split_title_group<'a>(tracks: Vec<&'a Track>) -> Vec<Vec<&'a Track>> {
    let mut folders: Vec<(String, Vec<&Track>)> = vec![];
    for t in tracks {
        let folder = album_folder(&t.id);
        match folders.iter_mut().find(|(f, _)| *f == folder) {
            Some((_, v)) => v.push(t),
            None => folders.push((folder, vec![t])),
        }
    }
    // Union folders that share an artist.
    let artist_sets: Vec<HashSet<String>> =
        folders.iter().map(|(_, v)| v.iter().flat_map(|t| track_artist_keys(t)).collect()).collect();
    let mut parent: Vec<usize> = (0..folders.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let root = find(parent, parent[i]);
            parent[i] = root;
        }
        parent[i]
    }
    for i in 0..folders.len() {
        for j in (i + 1)..folders.len() {
            if !artist_sets[i].is_disjoint(&artist_sets[j]) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[b.max(a)] = a.min(b);
            }
        }
    }
    let mut groups: Vec<(usize, Vec<&Track>)> = vec![];
    for (i, (_, v)) in folders.into_iter().enumerate() {
        let root = find(&mut parent, i);
        match groups.iter_mut().find(|(r, _)| *r == root) {
            Some((_, g)) => g.extend(v),
            None => groups.push((root, v)),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

fn build_album(group: Vec<&Track>) -> Album {
    let title = match most_common(group.iter().filter_map(|t| non_empty(&t.album))) {
        t if t.is_empty() => UNKNOWN_ALBUM.to_string(),
        t => t,
    };
    let folder = group.iter().map(|t| album_folder(&t.id)).min().unwrap_or_default();

    // Album artist: the tag if any track has one; else the dominant track artist.
    let tagged = most_common(group.iter().filter_map(|t| non_empty(&t.album_artist)));
    let (artist, various) = if !tagged.is_empty() {
        let v = group_key(&tagged) == group_key(VARIOUS_ARTISTS);
        (tagged, v)
    } else {
        let top = most_common(group.iter().map(|t| t.artist.trim()));
        let share = group.iter().filter(|t| group_key(&t.artist) == group_key(&top)).count() as f64 / group.len() as f64;
        if share >= DOMINANT_SHARE {
            (top, false)
        } else {
            (VARIOUS_ARTISTS.to_string(), true)
        }
    };

    let mut tracks: Vec<Track> = group.into_iter().cloned().collect();
    let multi_disc = discs_are_real(&tracks);
    tracks.sort_by_key(|t| {
        let disc = if multi_disc { t.disc_no.unwrap_or(1) } else { 1 };
        (disc, t.track_no.unwrap_or(i64::MAX), t.title.to_lowercase())
    });
    Album {
        id: format!("{}|{}", group_key(&title), folder),
        year: tracks.iter().filter_map(|t| t.year).min(),
        cover: tracks.iter().find_map(|t| t.thumbnail_url.clone()),
        title,
        artist,
        various,
        partial: false,
        tracks,
        multi_disc,
    }
}

pub(crate) fn build_library(tracks: &[Track]) -> Library {
    let mut by_title: Vec<(String, Vec<&Track>)> = vec![];
    for t in tracks.iter().filter(|t| local_ref::is_local(&t.id)) {
        let key = group_key(non_empty(&t.album).unwrap_or(UNKNOWN_ALBUM));
        match by_title.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(t),
            None => by_title.push((key, vec![t])),
        }
    }

    let mut albums: Vec<Album> =
        by_title.into_iter().flat_map(|(_, g)| split_title_group(g)).map(build_album).collect();
    albums.sort_by_key(|a| (sort_key(&a.artist), a.year.unwrap_or(i64::MAX), a.title.to_lowercase()));

    // Artists: the album's own artist, plus every guest with a track on it.
    let mut artists: HashMap<String, (String, Vec<Album>)> = HashMap::new();
    for album in &albums {
        let owner = group_key(&album.artist);
        artists.entry(owner.clone()).or_insert_with(|| (album.artist.clone(), vec![])).1.push(album.clone());

        let mut guests: Vec<(String, String)> = vec![]; // (key, display)
        for t in &album.tracks {
            for (key, name) in
                [(group_key(&t.artist), t.artist.trim()), (group_key(non_empty(&t.album_artist).unwrap_or("")), non_empty(&t.album_artist).unwrap_or(""))]
            {
                if !name.is_empty() && key != owner && !guests.iter().any(|(k, _)| *k == key) {
                    guests.push((key, name.to_string()));
                }
            }
        }
        for (key, name) in guests {
            let theirs: Vec<Track> = album
                .tracks
                .iter()
                .filter(|t| group_key(&t.artist) == key || non_empty(&t.album_artist).is_some_and(|a| group_key(a) == key))
                .cloned()
                .collect();
            let mut copy = album.clone();
            copy.partial = true;
            copy.tracks = theirs;
            artists.entry(key).or_insert_with(|| (name, vec![])).1.push(copy);
        }
    }

    let mut artists: Vec<Artist> = artists
        .into_values()
        .map(|(name, mut albums)| {
            albums.sort_by_key(|a| (a.year.unwrap_or(i64::MAX), a.title.to_lowercase()));
            Artist { name, albums }
        })
        .collect();
    artists.sort_by_key(|a| sort_key(&a.name));
    Library { artists, albums }
}

/// Albums in which no track has a cover, as `(artist, album, track
/// ids)`. Compilations, unknown albums and unknown artists are
/// skipped: there's nothing meaningful to look up.
pub(crate) fn albums_missing_cover(tracks: &[Track]) -> Vec<(String, String, Vec<String>)> {
    build_library(tracks)
        .albums
        .into_iter()
        .filter(|a| !a.various && a.title != UNKNOWN_ALBUM && a.cover.is_none())
        .filter(|a| group_key(&a.artist) != group_key("Unknown Artist"))
        .map(|a| (a.artist, a.title, a.tracks.into_iter().map(|t| t.id).collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, title: &str, artist: &str, album: Option<&str>) -> Track {
        Track {
            id: local_ref::encode(path),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.map(str::to_string),
            duration_ms: Some(1000),
            thumbnail_url: None,
            added_at: String::new(),
            album_artist: None,
            track_no: None,
            disc_no: None,
            year: None,
        }
    }

    fn artist<'a>(lib: &'a Library, name: &str) -> &'a Artist {
        lib.artists.iter().find(|a| a.name == name).unwrap_or_else(|| panic!("no artist {name}"))
    }

    #[test]
    fn groups_tracks_into_albums_and_artists() {
        let lib = build_library(&[
            track("/m/Band/First/1.mp3", "One", "Band", Some("First")),
            track("/m/Band/First/2.mp3", "Two", "Band", Some("First")),
            track("/m/Band/Second/1.mp3", "Solo", "Band", Some("Second")),
            track("/m/Else/Else/1.mp3", "Other", "Someone", Some("Else")),
        ]);
        assert_eq!(lib.albums.len(), 3);
        assert_eq!(lib.artists.len(), 2);
        assert_eq!(artist(&lib, "Band").albums.len(), 2);
        assert_eq!(artist(&lib, "Band").track_count(), 3);
    }

    #[test]
    fn a_compilation_with_a_different_artist_per_track_is_one_album() {
        let lib = build_library(&[
            track("/m/OST/1.flac", "A", "Hirano Yoshihisa", Some("Original Soundtrack")),
            track("/m/OST/2.flac", "B", "Hirano Yoshihisa", Some("Original Soundtrack")),
            track("/m/OST/3.flac", "C", "Hirano Yoshihisa", Some("Original Soundtrack")),
            track("/m/OST/4.flac", "D", "Amane Misa", Some("Original Soundtrack")),
            track("/m/OST/5.flac", "E", "Kenji Kawai", Some("Original Soundtrack")),
        ]);
        assert_eq!(lib.albums.len(), 1, "one folder, one title: one album");
        let album = &lib.albums[0];
        assert_eq!(album.tracks.len(), 5);
        assert_eq!(album.artist, "Hirano Yoshihisa", "the artist on most tracks owns it");
        assert!(!album.various);

        // Guests still find the album under their own name, with only their track.
        let guest = artist(&lib, "Amane Misa");
        assert_eq!(guest.albums.len(), 1);
        assert!(guest.albums[0].partial);
        assert_eq!(guest.albums[0].tracks.len(), 1);
        assert_eq!(guest.albums[0].id, album.id, "same album, opened from either artist");
        assert!(!artist(&lib, "Hirano Yoshihisa").albums[0].partial);
    }

    #[test]
    fn no_dominant_artist_means_various_artists() {
        let lib = build_library(&[
            track("/m/Comp/1.mp3", "A", "One", Some("Hits")),
            track("/m/Comp/2.mp3", "B", "Two", Some("Hits")),
            track("/m/Comp/3.mp3", "C", "Three", Some("Hits")),
        ]);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].artist, VARIOUS_ARTISTS);
        assert!(lib.albums[0].various);
        assert_eq!(lib.artists.len(), 4, "the three artists plus Various Artists");
    }

    #[test]
    fn same_title_by_different_artists_in_different_folders_stays_two_albums() {
        let lib = build_library(&[
            track("/m/A/Greatest Hits/1.mp3", "x", "A", Some("Greatest Hits")),
            track("/m/B/Greatest Hits/1.mp3", "y", "B", Some("Greatest Hits")),
        ]);
        assert_eq!(lib.albums.len(), 2);
    }

    #[test]
    fn the_same_album_ripped_into_two_folders_by_one_artist_is_merged() {
        let lib = build_library(&[
            track("/m/Siti/Sahmura/1.mp3", "A", "Siti", Some("Sahmura")),
            track("/m/Box/2000 - Sahmura/2.mp3", "B", "Siti", Some("Sahmura")),
        ]);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].tracks.len(), 2);
    }

    #[test]
    fn disc_subfolders_belong_to_the_album_folder() {
        let lib = build_library(&[
            track("/m/Band/Album/CD1/1.mp3", "A", "Band", Some("Album")),
            track("/m/Band/Album/CD 2/1.mp3", "B", "Band", Some("Album")),
            track("/m/Other/Album/1.mp3", "C", "Other", Some("Album")),
        ]);
        assert_eq!(lib.albums.len(), 2);
        assert_eq!(artist(&lib, "Band").albums[0].tracks.len(), 2);
    }

    #[test]
    fn artist_and_album_names_match_case_insensitively() {
        let lib = build_library(&[
            track("/m/x/1.mp3", "A", "Band", Some("Album")),
            track("/m/x/2.mp3", "B", "BAND", Some("album")),
        ]);
        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].tracks.len(), 2);
    }

    #[test]
    fn album_artist_tag_overrides_track_artist_so_featured_tracks_stay_together() {
        let mut feat = track("/m/x/2.mp3", "B", "Band feat. Guest", Some("Album"));
        feat.album_artist = Some("Band".to_string());
        let mut plain = track("/m/x/1.mp3", "A", "Band", Some("Album"));
        plain.album_artist = Some("Band".to_string());
        let lib = build_library(&[plain, feat]);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].artist, "Band");
        assert_eq!(artist(&lib, "Band").albums[0].tracks.len(), 2);
    }

    #[test]
    fn tracks_sort_by_disc_then_number_with_untagged_after_numbered_on_the_same_disc() {
        let mut t1 = track("/m/x/1", "Zed", "Band", Some("Album"));
        t1.track_no = Some(2);
        t1.disc_no = Some(2);
        let mut t2 = track("/m/x/2", "Beta", "Band", Some("Album"));
        t2.track_no = Some(5);
        t2.disc_no = Some(1);
        let mut t3 = track("/m/x/3", "Alpha", "Band", Some("Album"));
        t3.track_no = Some(2);
        t3.disc_no = Some(1);
        let untagged = track("/m/x/4", "Aardvark", "Band", Some("Album"));
        let lib = build_library(&[t1, t2, untagged, t3]);
        let order: Vec<_> = lib.albums[0].tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, ["Alpha", "Beta", "Aardvark", "Zed"]);
        assert!(lib.albums[0].multi_disc);
    }

    #[test]
    fn albums_sort_by_year_oldest_first_and_undated_last() {
        let mut new = track("/m/a/1", "A", "Band", Some("New"));
        new.year = Some(2020);
        let mut old = track("/m/b/1", "B", "Band", Some("Old"));
        old.year = Some(1999);
        let undated = track("/m/c/1", "C", "Band", Some("Aaa"));
        let lib = build_library(&[undated, new, old]);
        let titles: Vec<_> = artist(&lib, "Band").albums.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Old", "New", "Aaa"]);
    }

    #[test]
    fn artists_sort_ignoring_a_leading_the() {
        let lib = build_library(&[
            track("/m/1/1", "A", "The Beatles", None),
            track("/m/2/1", "B", "Abba", None),
            track("/m/3/1", "C", "Cher", None),
        ]);
        let names: Vec<_> = lib.artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Abba", "The Beatles", "Cher"]);
    }

    #[test]
    fn missing_album_tag_files_under_unknown_album_and_non_local_tracks_are_ignored() {
        let mut remote = track("/m/x/r", "R", "Band", Some("Album"));
        remote.id = "abc123".to_string();
        let lib = build_library(&[track("/m/x/1", "A", "Band", None), remote]);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].title, "Unknown Album");
        assert_eq!(lib.track_count(), 1);
    }

    #[test]
    fn punctuation_and_quote_style_do_not_split_an_album_or_artist() {
        let lib = build_library(&[
            track("/m/x/1", "A", "AC/DC", Some("Whatever That’s Not")),
            track("/m/y/2", "B", "AC-DC", Some("Whatever That's Not")),
        ]);
        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].tracks.len(), 2);
    }

    #[test]
    fn bogus_disc_tags_are_ignored_when_track_numbers_are_all_distinct() {
        let mut a = track("/m/x/1", "Fake Tales", "Band", Some("Album"));
        a.track_no = Some(3);
        a.disc_no = Some(1);
        let mut b = track("/m/x/2", "The View", "Band", Some("Album"));
        b.track_no = Some(1);
        b.disc_no = Some(2);
        let mut c = track("/m/x/3", "Dancing", "Band", Some("Album"));
        c.track_no = Some(4);
        let lib = build_library(&[a, b, c]);
        let order: Vec<_> = lib.albums[0].tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, ["The View", "Fake Tales", "Dancing"]);
        assert!(!lib.albums[0].multi_disc);
    }

    #[test]
    fn restarted_track_numbers_mark_a_genuine_multi_disc_album() {
        let mut a = track("/m/x/1", "D1T1", "Band", Some("Album"));
        a.track_no = Some(1);
        a.disc_no = Some(1);
        let mut b = track("/m/x/2", "D2T1", "Band", Some("Album"));
        b.track_no = Some(1);
        b.disc_no = Some(2);
        let lib = build_library(&[b, a]);
        assert!(lib.albums[0].multi_disc);
        assert_eq!(lib.albums[0].tracks[0].title, "D1T1");
    }

    #[test]
    fn albums_missing_cover_skips_covered_unknown_artist_unknown_album_and_compilations() {
        let mut covered = track("/m/a/1", "A", "Band", Some("Has Cover"));
        covered.thumbnail_url = Some("local:/c.jpg".to_string());
        let bare1 = track("/m/b/1", "B", "Band", Some("Bare"));
        let bare2 = track("/m/b/2", "C", "Band", Some("Bare"));
        let unknown_artist = track("/m/c/1", "D", "Unknown Artist", Some("Whatever"));
        let no_album = track("/m/d/1", "E", "Band", None);
        let comp = [
            track("/m/e/1", "F", "X", Some("Comp")),
            track("/m/e/2", "G", "Y", Some("Comp")),
            track("/m/e/3", "H", "Z", Some("Comp")),
        ];
        let mut all = vec![covered, bare1, bare2, unknown_artist, no_album];
        all.extend(comp);
        let missing = albums_missing_cover(&all);
        assert_eq!(missing.len(), 1);
        assert_eq!((missing[0].0.as_str(), missing[0].1.as_str()), ("Band", "Bare"));
        assert_eq!(missing[0].2.len(), 2);
    }

    #[test]
    fn an_albums_artists_list_the_most_represented_first() {
        let lib = build_library(&[
            track("/m/x/1", "A", "Guest", Some("Album")),
            track("/m/x/2", "B", "Main", Some("Album")),
            track("/m/x/3", "C", "Main", Some("Album")),
            track("/m/x/4", "D", "MAIN", Some("Album")),
        ]);
        assert_eq!(lib.albums[0].artists(), ["Main", "Guest"]);
    }

    #[test]
    fn a_tagged_album_artist_with_no_tracks_of_their_own_is_still_listed() {
        let mut t = track("/m/x/1", "A", "Band feat. Guest", Some("Album"));
        t.album_artist = Some("Band".to_string());
        let lib = build_library(&[t]);
        assert_eq!(lib.albums[0].artists(), ["Band", "Band feat. Guest"]);
    }
}
