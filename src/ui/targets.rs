//! Where a notification leads. A finished job's notification carries a target
//! string; clicking it opens the app and lands here:
//!
//! - `eq` / `lyrics` / `library`: that page.
//! - `eq:<track id>` / `lyrics:<track id>`: that page, with the track's analysis open.
//! - `artist:<name>`: the Library, on that artist's page.
//!
//! The Shell goes to the page; the page opens the track (it has the signal for its own window).

use super::Route;
use dioxus::prelude::*;

#[derive(Clone, Copy)]
pub struct OpenTarget(pub Signal<Option<String>>);

pub fn use_open_target_provider() -> OpenTarget {
    use_context_provider(|| OpenTarget(Signal::new(None)))
}

pub fn use_open_target() -> OpenTarget {
    use_context::<OpenTarget>()
}

impl OpenTarget {
    pub fn open(&self, target: String) {
        let mut s = self.0;
        s.set(Some(target));
    }

    pub fn clear(&self) {
        let mut s = self.0;
        s.set(None);
    }

    /// The track id in a `<kind>:<id>` target of this `kind`, if that is what is waiting.
    pub fn track_for(&self, kind: &str) -> Option<String> {
        let t = self.0.read();
        let (k, id) = t.as_deref()?.split_once(':')?;
        (k == kind && !id.is_empty()).then(|| id.to_string())
    }
}

/// The page a target belongs on.
pub fn route_for(target: &str) -> Route {
    match target.split(':').next().unwrap_or("") {
        "eq" => Route::Eq {},
        "lyrics" => Route::Lyrics {},
        _ => Route::Library {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_pick_their_page() {
        assert_eq!(route_for("eq"), Route::Eq {});
        assert_eq!(route_for("eq:local:/m/a.flac"), Route::Eq {});
        assert_eq!(route_for("lyrics:local:/m/a.flac"), Route::Lyrics {});
        assert_eq!(route_for("library"), Route::Library {});
        assert_eq!(route_for("anything else"), Route::Library {});
    }
}
