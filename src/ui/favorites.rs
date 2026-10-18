//! App-wide set of favorite track ids: anything that reads `use_favorites()`
//! re-renders when a heart is toggled.

use super::api;
use dioxus::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub struct Favorites(Signal<HashSet<String>>);

/// Provide once, near the root.
pub fn use_favorites_provider() -> Favorites {
    let mut set = use_context_provider(|| Signal::new(HashSet::new()));
    use_future(move || async move {
        if let Ok(ids) = api::list_favorites().await {
            set.set(ids.into_iter().collect());
        }
    });
    Favorites(set)
}

pub fn use_favorites() -> Favorites {
    Favorites(use_context::<Signal<HashSet<String>>>())
}

impl Favorites {
    pub fn contains(&self, id: &str) -> bool {
        self.0.read().contains(id)
    }

    /// Forgets every heart on screen (after the stored ones were deleted).
    pub fn clear_local(&self) {
        let mut set = self.0;
        set.set(HashSet::new());
    }

    /// Flips the heart now, then persists; a failed save puts it back.
    pub fn toggle(&self, id: String) {
        let mut set = self.0;
        let on = !set.peek().contains(&id);
        if on {
            set.write().insert(id.clone());
        } else {
            set.write().remove(&id);
        }
        spawn(async move {
            if api::set_favorite(id.clone(), on).await.is_err() {
                if on {
                    set.write().remove(&id);
                } else {
                    set.write().insert(id);
                }
            }
        });
    }
}
