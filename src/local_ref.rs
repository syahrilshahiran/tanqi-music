//! Local references (see GLOSSARY.md): `local:<path>` names a file on this
//! machine. It is the id of a scanned Track and the location of its cover,
//! so audio and image files share one convention and nothing else needs to
//! know the prefix. Stored as plain strings everywhere, so existing data is
//! unaffected.

use std::path::Path;

const PREFIX: &str = "local:";

/// `/music/a.flac` -> `local:/music/a.flac`.
pub fn encode(path: impl AsRef<Path>) -> String {
    format!("{PREFIX}{}", path.as_ref().to_string_lossy())
}

/// `local:/music/a.flac` -> `/music/a.flac`. `None` for anything that isn't
/// a local reference (a bare id, an `https://` cover).
pub fn decode(reference: &str) -> Option<&str> {
    reference.strip_prefix(PREFIX)
}

pub fn is_local(reference: &str) -> bool {
    decode(reference).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_round_trips() {
        let reference = encode("/music/My Cool Song.flac");
        assert_eq!(reference, "local:/music/My Cool Song.flac");
        assert_eq!(decode(&reference), Some("/music/My Cool Song.flac"));
        assert!(is_local(&reference));
    }

    #[test]
    fn a_path_object_encodes_the_same_as_its_text() {
        assert_eq!(encode(std::path::PathBuf::from("/m/a.mp3")), encode("/m/a.mp3"));
    }

    #[test]
    fn anything_else_is_not_a_local_reference() {
        for other in ["dQw4w9WgXcQ", "https://example.com/a.jpg", "/music/a.flac", "", "LOCAL:/x", "local"] {
            assert_eq!(decode(other), None, "{other:?}");
            assert!(!is_local(other));
        }
    }

    #[test]
    fn an_empty_path_still_decodes_to_an_empty_path() {
        assert_eq!(decode("local:"), Some(""));
    }
}
