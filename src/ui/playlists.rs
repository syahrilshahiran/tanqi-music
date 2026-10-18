//! App-wide playlists: anything that reads `use_playlists()` re-renders when
//! one changes. Every edit goes to the database, then the list is reloaded,
//! so the signal always mirrors what is stored.

use super::api;
use app_lib::models::Playlist;
use dioxus::prelude::*;

#[derive(Clone, Copy)]
pub struct Playlists(Signal<Vec<Playlist>>);

/// Provide once, near the root.
pub fn use_playlists_provider() -> Playlists {
    let list = use_context_provider(|| Signal::new(Vec::<Playlist>::new()));
    let playlists = Playlists(list);
    use_future(move || async move { playlists.reload().await });
    playlists
}

pub fn use_playlists() -> Playlists {
    Playlists(use_context::<Signal<Vec<Playlist>>>())
}

impl Playlists {
    pub fn all(&self) -> Vec<Playlist> {
        self.0.read().clone()
    }

    pub fn get(&self, id: i64) -> Option<Playlist> {
        self.0.read().iter().find(|p| p.id == id).cloned()
    }

    pub async fn reload(&self) {
        let mut list = self.0;
        if let Ok(all) = api::list_playlists().await {
            list.set(all);
        }
    }

    /// Creates a playlist holding `track_ids`; `on_done` gets its id.
    pub fn create(&self, name: String, track_ids: Vec<String>, on_done: impl FnOnce(i64) + 'static) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            if let Ok(id) = api::create_playlist(name, track_ids).await {
                this.reload().await;
                on_done(id);
            }
        });
    }

    pub fn rename(&self, id: i64, name: String) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            let _ = api::rename_playlist(id, name).await;
            this.reload().await;
        });
    }

    pub fn delete(&self, id: i64) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            let _ = api::delete_playlist(id).await;
            this.reload().await;
        });
    }

    pub fn add(&self, id: i64, track_ids: Vec<String>) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            let _ = api::add_to_playlist(id, track_ids).await;
            this.reload().await;
        });
    }

    pub fn remove(&self, id: i64, position: usize) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            let _ = api::remove_from_playlist(id, position as i64).await;
            this.reload().await;
        });
    }

    pub fn move_track(&self, id: i64, position: usize, delta: i64) {
        let this = *self;
        // On the root scope: the sheet or menu that asked may close before this finishes.
        dioxus::core::spawn_forever(async move {
            let _ = api::move_in_playlist(id, position as i64, delta).await;
            this.reload().await;
        });
    }
}
