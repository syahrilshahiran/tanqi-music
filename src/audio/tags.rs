//! Best-effort tags and embedded cover art for local audio files.

use serde::Serialize;
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Best-effort embedded metadata for a local audio file, read from its own
/// path (no download involved - this is only ever used for files the user
/// already has on disk). All fields are `None` when unreadable/untagged -
/// never an error, since one bad file shouldn't sink a whole folder import.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Album-artist tag; groups compilations and "feat." tracks under one
    /// artist when the per-track artist differs.
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Front-cover image embedded in the file's tags, if any.
    #[serde(skip)]
    pub embedded_cover: Option<EmbeddedCover>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedCover {
    /// File extension to store it under ("jpg", "png", ...).
    pub ext: &'static str,
    pub data: Vec<u8>,
}

impl LocalTrackTags {
    /// Fills every field still `None` from `other`; existing values win.
    pub fn fill_missing(&mut self, other: LocalTrackTags) {
        self.title = self.title.take().or(other.title);
        self.artist = self.artist.take().or(other.artist);
        self.album = self.album.take().or(other.album);
        self.album_artist = self.album_artist.take().or(other.album_artist);
        self.track_no = self.track_no.or(other.track_no);
        self.disc_no = self.disc_no.or(other.disc_no);
        self.year = self.year.or(other.year);
        self.duration_ms = self.duration_ms.or(other.duration_ms);
        self.embedded_cover = self.embedded_cover.take().or(other.embedded_cover);
    }
}

fn cover_ext(media_type: &str, data: &[u8]) -> &'static str {
    match media_type.to_lowercase().as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/jpeg" | "image/jpg" => "jpg",
        // Missing/odd media type: sniff the magic bytes.
        _ if data.starts_with(&[0x89, b'P', b'N', b'G']) => "png",
        _ if data.starts_with(b"RIFF") => "webp",
        _ => "jpg",
    }
}

/// Leading integer of a tag like "3" or "3/12" (track/disc numbers are
/// commonly written as "n/total").
fn leading_int(s: &str) -> Option<i64> {
    let digits: String = s.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Four-digit year at the start of a date tag ("2019", "2019-04-12").
fn leading_year(s: &str) -> Option<i64> {
    let year = leading_int(s.trim().get(..4)?)?;
    (1000..=9999).contains(&year).then_some(year)
}

/// Fills any field of `tags` still `None` from `rev`, so the probe-time and
/// container-level metadata blocks can be layered without overwriting.
fn fill_tags(tags: &mut LocalTrackTags, rev: &symphonia::core::meta::MetadataRevision) {
    use symphonia::core::meta::StandardTagKey as K;
    tags.title = tags.title.take().or_else(|| tag_value(rev, K::TrackTitle));
    tags.artist = tags.artist.take().or_else(|| tag_value(rev, K::Artist));
    tags.album = tags.album.take().or_else(|| tag_value(rev, K::Album));
    tags.album_artist = tags.album_artist.take().or_else(|| tag_value(rev, K::AlbumArtist));
    if tags.embedded_cover.is_none() {
        use symphonia::core::meta::StandardVisualKey;
        let visuals = rev.visuals();
        let best = visuals
            .iter()
            .find(|v| v.usage == Some(StandardVisualKey::FrontCover))
            .or_else(|| visuals.first())
            .filter(|v| !v.data.is_empty());
        tags.embedded_cover = best.map(|v| EmbeddedCover { ext: cover_ext(&v.media_type, &v.data), data: v.data.to_vec() });
    }
    tags.track_no = tags.track_no.or_else(|| tag_value(rev, K::TrackNumber).and_then(|v| leading_int(&v)));
    tags.disc_no = tags.disc_no.or_else(|| tag_value(rev, K::DiscNumber).and_then(|v| leading_int(&v)));
    tags.year = tags.year.or_else(|| {
        [K::Date, K::ReleaseDate, K::OriginalDate].into_iter().find_map(|k| tag_value(rev, k).and_then(|v| leading_year(&v)))
    });
}

fn tag_value(rev: &symphonia::core::meta::MetadataRevision, key: symphonia::core::meta::StandardTagKey) -> Option<String> {
    rev.tags().iter().find(|t| t.std_key == Some(key)).map(|t| t.value.to_string())
}

/// Reads title/artist/album tags and a best-effort duration directly from a
/// local file's headers - no decoding of actual audio samples needed, so
/// this stays cheap even over a folder with hundreds of tracks.
pub fn read_local_track_tags(path: &std::path::Path) -> LocalTrackTags {
    let mut result = LocalTrackTags::default();

    let Ok(file) = std::fs::File::open(path) else {
        return result;
    };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let Ok(mut probed) =
        symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
    else {
        return result;
    };

    // Different containers surface embedded tags through one of two paths -
    // metadata read during probing (e.g. an ID3v2 block prepended ahead of
    // raw MP3 data) or metadata inside the container itself, only available
    // once a FormatReader exists (e.g. FLAC's Vorbis comments) - so probed
    // metadata is checked first, then anything still missing is filled in
    // from the container's own metadata.
    let mut probed_metadata = probed.metadata.get();
    if let Some(rev) = probed_metadata.as_mut().and_then(|m| m.skip_to_latest()) {
        fill_tags(&mut result, rev);
    }
    if let Some(rev) = probed.format.metadata().skip_to_latest() {
        fill_tags(&mut result, rev);
    }

    if let Some(track) = probed.format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL) {
        if let (Some(n_frames), Some(sample_rate)) = (track.codec_params.n_frames, track.codec_params.sample_rate) {
            if sample_rate > 0 {
                result.duration_ms = Some((n_frames as i64 * 1000) / sample_rate as i64);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal canonical WAV (RIFF/WAVE, 16-bit PCM, mono) of silence:
    /// decodable, but carrying no metadata at all.
    fn silent_wav() -> Vec<u8> {
        let sample_rate = 44_100u32;
        let data_len = sample_rate * 2; // one second of 16-bit mono
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend(std::iter::repeat(0u8).take(data_len as usize));
        wav
    }

    #[test]
    fn reads_tags_missing_entirely_from_an_untagged_file() {
        // A synthetic WAV built by `sine_wav` carries no metadata at all -
        // should come back as all-`None`, not an error.
        let wav = silent_wav();
        let dir = std::env::temp_dir();
        let path = dir.join(format!("tanqi-test-untagged-{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(&path, &wav).expect("should write temp file");

        let tags = read_local_track_tags(&path);
        let _ = std::fs::remove_file(&path);

        assert!(tags.title.is_none());
        assert!(tags.artist.is_none());
        assert!(tags.album.is_none());
    }

    #[test]
    fn nonexistent_file_reads_as_all_none_rather_than_panicking() {
        let tags = read_local_track_tags(std::path::Path::new("/nonexistent/path/does-not-exist.mp3"));
        assert!(tags.title.is_none());
        assert!(tags.artist.is_none());
        assert!(tags.album.is_none());
        assert!(tags.duration_ms.is_none());
    }

    /// Hits real ffmpeg to build tagged fixture files. Not run by default -
    /// requires ffmpeg on PATH; `cargo test -- --ignored`.
    ///
    /// Regression coverage for both tag-storage conventions this function
    /// has to handle: ID3v2 (prepended ahead of the raw audio, surfaced via
    /// `probed.metadata` during probing) for MP3, and Vorbis comments
    /// (inside the container itself, only reachable via
    /// `format.metadata()` once a `FormatReader` exists) for FLAC -
    /// confirmed live that a real MP3's tags come from the first path and a
    /// real FLAC's come from the second, which is exactly why
    /// `read_local_track_tags` checks both rather than just one.
    #[test]
    #[ignore]
    fn reads_id3_and_vorbis_comment_tags_from_real_files() {
        let dir = std::env::temp_dir();
        let mp3_path = dir.join(format!("tanqi-test-{}.mp3", uuid::Uuid::new_v4()));
        let flac_path = dir.join(format!("tanqi-test-{}.flac", uuid::Uuid::new_v4()));

        for (path, ext) in [(&mp3_path, "mp3"), (&flac_path, "flac")] {
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
                    "-metadata",
                    &format!("title=Test {ext} Title"),
                    "-metadata",
                    &format!("artist=Test {ext} Artist"),
                ])
                .arg(path)
                .status()
                .expect("ffmpeg should run");
            assert!(status.success(), "ffmpeg should produce a tagged {ext} fixture");
        }

        let mp3_tags = read_local_track_tags(&mp3_path);
        let flac_tags = read_local_track_tags(&flac_path);
        let _ = std::fs::remove_file(&mp3_path);
        let _ = std::fs::remove_file(&flac_path);

        assert_eq!(mp3_tags.title.as_deref(), Some("Test mp3 Title"));
        assert_eq!(mp3_tags.artist.as_deref(), Some("Test mp3 Artist"));
        assert_eq!(flac_tags.title.as_deref(), Some("Test flac Title"));
        assert_eq!(flac_tags.artist.as_deref(), Some("Test flac Artist"));
    }
}

#[cfg(test)]
mod tag_parsing_tests {
    use super::{cover_ext, leading_int, leading_year};

    #[test]
    fn cover_extension_comes_from_the_media_type_then_the_magic_bytes() {
        assert_eq!(cover_ext("image/png", &[]), "png");
        assert_eq!(cover_ext("IMAGE/JPEG", &[]), "jpg");
        assert_eq!(cover_ext("", &[0x89, b'P', b'N', b'G', 1]), "png");
        assert_eq!(cover_ext("", &[1, 2, 3]), "jpg");
    }

    #[test]
    fn track_and_disc_numbers_accept_n_of_total() {
        assert_eq!(leading_int("3/12"), Some(3));
        assert_eq!(leading_int(" 07 "), Some(7));
        assert_eq!(leading_int("x"), None);
    }

    #[test]
    fn year_is_the_leading_four_digits_of_a_date() {
        assert_eq!(leading_year("2019-04-12"), Some(2019));
        assert_eq!(leading_year("1999"), Some(1999));
        assert_eq!(leading_year("99"), None);
        assert_eq!(leading_year("April 2019"), None);
    }
}

