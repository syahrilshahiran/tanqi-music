//! Compact-widget mode: same window, smaller and always-on-top, showing the
//! compact music widget instead of the full app shell. The widget has no
//! window frame; it is moved by dragging its body (see `CompactMusicWidget`).

use crate::host::tao::dpi::PhysicalSize;
use crate::host::{window, LogicalSize};
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompactWidget {
    Music,
}

const WIDGET_SIZE: (f64, f64) = (344.0, 660.0);
const WIDGET_MIN: (f64, f64) = (300.0, 580.0);
const DEFAULT_MIN: (f64, f64) = (900.0, 600.0);
const DEFAULT_SIZE: (f64, f64) = (1200.0, 800.0);

#[derive(Clone, Copy)]
pub struct Compact {
    pub widget: Signal<Option<CompactWidget>>,
    restore: Signal<Option<PhysicalSize<u32>>>,
}

pub fn use_compact_provider() -> Compact {
    use_context_provider(|| Compact { widget: Signal::new(None), restore: Signal::new(None) })
}

pub fn use_compact() -> Compact {
    use_context::<Compact>()
}

impl Compact {
    pub fn current(&self) -> Option<CompactWidget> {
        (self.widget)()
    }

    pub fn enter(&self, kind: CompactWidget) {
        let already = self.widget.peek().is_some();
        let mut w = self.widget;
        w.set(Some(kind));
        if already {
            return;
        }
        let win = window();
        let mut restore = self.restore;
        restore.set(Some(win.inner_size()));
        win.set_min_inner_size(Some(LogicalSize::new(WIDGET_MIN.0, WIDGET_MIN.1)));
        win.set_inner_size(LogicalSize::new(WIDGET_SIZE.0, WIDGET_SIZE.1));
        win.set_decorations(false);
        win.set_always_on_top(true);
    }

    pub fn exit(&self) {
        let mut w = self.widget;
        w.set(None);
        let win = window();
        win.set_decorations(true);
        win.set_min_inner_size(Some(LogicalSize::new(DEFAULT_MIN.0, DEFAULT_MIN.1)));
        match *self.restore.peek() {
            Some(size) => {
                win.set_inner_size(size);
            }
            None => {
                win.set_inner_size(LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
            }
        }
        // Dropping always-on-top lets the window manager slide it behind
        // whatever is under it; the user just clicked it, so bring it back.
        win.set_minimized(false);
        win.set_focus();
        // Some windows (the Claude desktop app, for one) won't yield on a
        // plain focus request. Stay on top just long enough to rise above
        // them, then let go so the full app behaves like any other window.
        let win = window();
        // Not `spawn`: that task would die with the mini player that is
        // unmounting right now, and the window would never let go.
        dioxus::core::spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            win.set_always_on_top(false);
        });
    }
}
