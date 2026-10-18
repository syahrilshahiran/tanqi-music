use dioxus::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// For a popover: returns a DOM id to put on its root element, and while
/// `open` is true the first press outside that element (or Escape) sets it
/// back to false.
pub fn use_dismiss(mut open: Signal<bool>) -> String {
    let dom_id = use_hook(|| format!("dismiss-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    use_effect({
        let dom_id = dom_id.clone();
        move || {
            if !open() {
                return;
            }
            let mut ev = document::eval(&format!(
                r#"
                const root = '#{dom_id}';
                const done = () => {{
                    document.removeEventListener('pointerdown', down, true);
                    document.removeEventListener('keydown', key, true);
                    dioxus.send(true);
                }};
                const down = (e) => {{ if (!e.target.closest(root)) done(); }};
                const key = (e) => {{ if (e.key === 'Escape') done(); }};
                document.addEventListener('pointerdown', down, true);
                document.addEventListener('keydown', key, true);
                "#
            ));
            spawn(async move {
                if ev.recv::<bool>().await.is_ok() {
                    open.set(false);
                }
            });
        }
    });
    dom_id
}
