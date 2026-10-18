//! Remembering the last session: the Play queue and where in the current Track it stopped, kept in the
//! webview's local storage and brought back (paused) on the next start.

use super::*;

pub(super) const LAST_QUEUE_KEY: &str = "tanqi-last-queue";
pub(super) const LAST_POSITION_KEY: &str = "tanqi-last-position";
const EQ_ENABLED_KEY: &str = "tanqi-eq-enabled";

impl Player {
    /// Remembers whether the suggested EQ button was on or off, for the next start.
    pub(super) fn persist_eq_enabled(&self) {
        let on = *self.eq_enabled.peek();
        document::eval(&format!("try {{ localStorage.setItem('{EQ_ENABLED_KEY}', '{on}') }} catch (e) {{}}"));
    }

    /// Brings back the EQ button as it was left (on when it was never touched).
    pub(super) async fn restore_eq_enabled(&self) {
        let mut this = *self;
        let mut ev = document::eval(&format!(
            "try {{ dioxus.send(localStorage.getItem('{EQ_ENABLED_KEY}')) }} catch (e) {{ dioxus.send(null) }}"
        ));
        if let Ok(Some(saved)) = ev.recv::<Option<String>>().await {
            this.eq_enabled.set(saved != "false");
        }
    }

    /// Writes the Play queue and which Track is current (or forgets them when the queue is empty).
    pub(super) fn persist_queue(&self) {
        let queue = self.queue.peek().clone();
        let js = match *self.current_index.peek() {
            Some(index) if !queue.is_empty() => {
                let ids: Vec<&str> = queue.iter().map(|t| t.id.as_str()).collect();
                format!("localStorage.setItem('{LAST_QUEUE_KEY}', JSON.stringify({}))", json!({"ids": ids, "index": index}))
            }
            _ => format!("localStorage.removeItem('{LAST_QUEUE_KEY}'); localStorage.removeItem('{LAST_POSITION_KEY}')"),
        };
        document::eval(&format!("try {{ {js} }} catch (e) {{}}"));
        self.persist_position();
    }

    pub(super) fn persist_position(&self) {
        if self.current_index.peek().is_some() {
            let position = *self.position.peek();
            document::eval(&format!("try {{ localStorage.setItem('{LAST_POSITION_KEY}', '{position}') }} catch (e) {{}}"));
        }
    }

    /// Brings back the Play queue and the current Track from the last session, paused
    /// where it stopped. The audio is only loaded when the user presses play. Tracks that
    /// have left the Library since are dropped.
    pub(super) async fn restore_last(&self) {
        let mut this = *self;
        let mut ev = document::eval(&format!(
            "try {{ dioxus.send([localStorage.getItem('{LAST_QUEUE_KEY}'), localStorage.getItem('{LAST_POSITION_KEY}')]) }} catch (e) {{ dioxus.send([null, null]) }}"
        ));
        let Ok((Some(raw_queue), raw_position)) = ev.recv::<(Option<String>, Option<String>)>().await else { return };
        let Ok(saved) = serde_json::from_str::<Value>(&raw_queue) else { return };
        let ids: Vec<String> = saved["ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let saved_index = saved["index"].as_u64().unwrap_or(0) as usize;
        let Some(saved_id) = ids.get(saved_index).cloned() else { return };
        let Ok(tracks) = app_lib::with_store(|s| s.list_tracks()).await else { return };
        let by_id: std::collections::HashMap<&str, &app_lib::models::Track> = tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        let queue: Vec<TrackSummary> = ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()))
            .map(|t| TrackSummary {
                id: t.id.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                duration_ms: t.duration_ms,
                thumbnail_url: t.thumbnail_url.clone(),
            })
            .collect();
        // The user may have started something while this was reading.
        if queue.is_empty() || !this.queue.peek().is_empty() {
            return;
        }
        // The Track that was current, or (if it is gone) the one now in its place, from the start.
        let (index, mut offset) = match queue.iter().position(|t| t.id == saved_id) {
            Some(i) => (i, raw_position.and_then(|p| p.parse::<f64>().ok()).unwrap_or(0.0)),
            None => (saved_index.min(queue.len() - 1), 0.0),
        };
        let duration = queue[index].duration_ms.map(|ms| ms as f64 / 1000.0).unwrap_or(0.0);
        // Not worth resuming in the last seconds.
        if duration > 0.0 && offset >= duration - 3.0 {
            offset = 0.0;
        }
        this.queue.set(queue);
        this.current_index.set(Some(index));
        this.duration.set(duration);
        this.position.set(offset);
        this.resume_at.set(Some(offset));
    }
}
