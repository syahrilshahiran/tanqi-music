//! The Play queue: what comes next or before (following shuffle and repeat), the prefetch of the next
//! Track, and the edits (add, remove, move, clear, shuffle, repeat). The order rules themselves are pure and
//! live in `play_queue`.

use super::*;

impl Player {
    pub fn has_next(&self) -> bool {
        let Some(idx) = (self.current_index)() else { return false };
        let repeat = (self.repeat)();
        repeat == RepeatMode::One
            || resolve_next_index(idx, (self.queue)().len(), self.shuffle_order_if_on().as_ref(), repeat).is_some()
    }

    pub fn has_previous(&self) -> bool {
        let Some(idx) = (self.current_index)() else { return false };
        let len = (self.queue)().len();
        match self.shuffle_order_if_on() {
            Some(order) if order.len() == len => order.iter().position(|&i| i == idx).map(|p| p > 0).unwrap_or(false),
            _ => idx > 0,
        }
    }

    pub(super) fn shuffle_order_if_on(&self) -> Option<Vec<usize>> {
        if (self.shuffle)() {
            (self.shuffle_order)()
        } else {
            None
        }
    }

    /// Live position without subscribing the caller to updates.

    pub(super) fn schedule_prefetch(&self, index: usize) {
        let mut this = *self;
        let queue = this.queue.peek().clone();
        let next_idx = resolve_next_index(index, queue.len(), this.shuffle_order_if_on_peek().as_ref(), *this.repeat.peek());
        let token = *this.next_token.peek() + 1;
        this.next_token.set(token);
        let Some(ni) = next_idx else {
            this.next_info.set(None);
            cmd("setNext", json!(null));
            return;
        };
        let Some(track) = queue.get(ni).cloned() else { return };
        let existing = this.next_info.peek().clone();
        if let Some(existing) = existing {
            if existing.id == track.id {
                let mut info = existing;
                info.index = ni;
                let key = info.id.clone();
                let norm = norm_db(&info.analysis);
                this.next_info.set(Some(info));
                // Same track already requested: the engine re-arms.
                // (URL isn't needed again - the engine keeps its buffer.)
                cmd("setNext", json!({"key": key, "url": "", "normDb": norm}));
                return;
            }
        }
        this.next_info.set(None);
        let with_analysis = self.audio_analysis_on();
        spawn(async move {
            let Ok((url, analysis)) = Self::resolve_source(&track, with_analysis).await else { return };
            if token != *this.next_token.peek() {
                return;
            }
            let norm = norm_db(&analysis);
            this.next_info.set(Some(NextInfo { index: ni, id: track.id.clone(), analysis }));
            cmd("setNext", json!({"key": track.id, "url": url, "normDb": norm}));
        });
    }

    pub(super) fn shuffle_order_if_on_peek(&self) -> Option<Vec<usize>> {
        if *self.shuffle.peek() {
            self.shuffle_order.peek().clone()
        } else {
            None
        }
    }

    pub(super) fn reset_prefetch(&self) {
        let mut this = *self;
        let t = *this.next_token.peek() + 1;
        this.next_token.set(t);
        cmd("setNext", json!(null));
    }

    // ---- queue editing ----

    /// Swaps in an edited queue and re-derives what depends on queue
    /// positions: the shuffle order (rebuilt, with `after_current` placed
    /// right behind the playing track) and the gapless lookahead.
    pub(super) fn queue_edited(&self, queue: Vec<TrackSummary>, index: Option<usize>, after_current: Option<usize>) {
        let mut this = *self;
        if this.shuffle_order.peek().is_some() {
            let mut order = build_shuffle_order(queue.len(), index.unwrap_or(0));
            if let (Some(ac), Some(cur)) = (after_current, index) {
                order.retain(|&i| i != ac);
                let at = order.iter().position(|&i| i == cur).map_or(0, |p| p + 1);
                order.insert(at, ac);
            }
            this.shuffle_order.set(Some(order));
        }
        this.queue.set(queue);
        this.current_index.set(index);
        self.reset_prefetch();
        this.next_info.set(None);
        if let Some(i) = index {
            self.schedule_prefetch(i);
        }
    }

    /// Plays `track` right after the current one. With nothing queued it
    /// just starts playing.
    pub fn play_after_current(&self, track: TrackSummary) {
        let Some(idx) = *self.current_index.peek() else {
            self.play_track(track, None);
            return;
        };
        let mut queue = self.queue.peek().clone();
        queue.insert(idx + 1, track);
        self.queue_edited(queue, Some(idx), Some(idx + 1));
    }

    /// Adds `track` to the end of the queue.
    pub fn enqueue(&self, track: TrackSummary) {
        let Some(idx) = *self.current_index.peek() else {
            self.play_track(track, None);
            return;
        };
        let mut queue = self.queue.peek().clone();
        queue.push(track);
        self.queue_edited(queue, Some(idx), None);
    }

    /// Drops the entry at `index` (never the one playing).
    pub fn remove_from_queue(&self, index: usize) {
        let Some(cur) = *self.current_index.peek() else { return };
        let mut queue = self.queue.peek().clone();
        if index == cur || index >= queue.len() {
            return;
        }
        queue.remove(index);
        self.queue_edited(queue, Some(if index < cur { cur - 1 } else { cur }), None);
    }

    /// Moves the entry at `index` one place up (`-1`) or down (`1`).
    pub fn move_in_queue(&self, index: usize, delta: i32) {
        let Some(cur) = *self.current_index.peek() else { return };
        let mut queue = self.queue.peek().clone();
        let to = index as i32 + delta;
        if to < 0 || to as usize >= queue.len() || index >= queue.len() {
            return;
        }
        let to = to as usize;
        queue.swap(index, to);
        let cur = if cur == index { to } else if cur == to { index } else { cur };
        self.queue_edited(queue, Some(cur), None);
    }

    /// Keeps only the playing track.
    pub fn clear_queue(&self) {
        let Some(track) = self.current_track() else { return };
        self.queue_edited(vec![track], Some(0), None);
    }

    /// Jumps to the entry at `index`.
    pub fn play_index(&self, index: usize) {
        let mut this = *self;
        let Some(track) = this.queue.peek().get(index).cloned() else { return };
        self.reset_prefetch();
        this.current_index.set(Some(index));
        self.load_and_play(track, index);
    }

    pub fn toggle_shuffle(&self) {
        let mut this = *self;
        let next = !*this.shuffle.peek();
        this.shuffle.set(next);
        this.shuffle_order.set(if next {
            Some(build_shuffle_order(this.queue.peek().len(), this.current_index.peek().unwrap_or(0)))
        } else {
            None
        });
        // Whatever was prefetched was chosen under the old order.
        self.reset_prefetch();
        let cur = *this.current_index.peek();
        if let Some(i) = cur {
            self.schedule_prefetch(i);
        }
    }

    pub fn cycle_repeat(&self) {
        let mut this = *self;
        let next = match *this.repeat.peek() {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        this.repeat.set(next);
        self.reset_prefetch();
        if next != RepeatMode::One {
            let cur = *this.current_index.peek();
            if let Some(i) = cur {
                self.schedule_prefetch(i);
            }
        }
    }
}
