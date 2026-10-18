// The app is dark-only: the `dark` class on <html> is always set, before first
// paint, so the `:root.dark` rules apply.
(function () {
  document.documentElement.classList.add("dark");
  // `silver` is the light skin, layered on top of `dark` (which carries the
  // device layout and type); the choice is remembered across launches.
  const root = document.documentElement;
  const KEY = "tanqi-theme";
  try {
    root.classList.toggle("silver", localStorage.getItem(KEY) === "silver");
  } catch (_) {}
  window.tqTheme = () => (root.classList.contains("silver") ? "silver" : "dark");
  window.tqSetTheme = (name) => {
    root.classList.toggle("silver", name === "silver");
    try {
      localStorage.setItem(KEY, name);
    } catch (_) {}
  };
  // Called by Shell (src/ui/mod.rs) with whether a track is loaded. While the
  // player bar is up, `.player-open` makes modal scrims stop short of it (see
  // assets/css/00-base.css).
  // Library drill-down (artist / album pages): opening one starts at its top,
  // and going back returns to where the list was. The scroll position is
  // taken on press, just before the click changes the page.
  let pressedY = 0;
  document.addEventListener("pointerdown", () => { pressedY = window.scrollY; }, true);
  let savedY = {};
  let depth = 0;
  window.tqLibForget = () => { savedY = {}; depth = 0; };
  window.tqLibNav = (next) => {
    if (next > depth) {
      savedY[depth] = pressedY;
      window.scrollTo(0, 0);
    } else if (next < depth) {
      const y = savedY[next] || 0;
      requestAnimationFrame(() => window.scrollTo(0, y));
    }
    depth = next;
  };
  // The phone's Back button (android/MainActivity.kt asks this first; `false` means close the app).
  // One step per press: an open sheet or panel, then the full-screen player, then a Library page,
  // then back to the Library tab. On the Library's top level it returns false.
  window.tqBack = () => {
    const click = (el) => { if (el) { el.click(); return true; } return false; };
    const scrims = document.querySelectorAll(".modal-scrim");
    if (scrims.length) return click(scrims[scrims.length - 1]);
    if (click(document.querySelector(".library__drawer-scrim"))) return true;
    if (click(document.querySelector(".queue-panel__head button:last-child"))) return true;
    if (click(document.querySelector(".lyrics-panel__head button:last-child"))) return true;
    const eq = document.querySelector(".player-bar__eq-panel .eq__head button[aria-label=\"Close\"]");
    if (click(eq)) return true;
    if (document.querySelector(".player-bar--open")) return click(document.querySelector(".player-bar__expand"));
    if (click(document.querySelector(".library__back"))) return true;
    const tabs = [...document.querySelectorAll(".tabbar__tab")];
    if (tabs.length && !tabs[0].classList.contains("tabbar__tab--active")) return click(tabs[0]);
    return false;
  };
  window.tqPlayerOpen = (open) => document.documentElement.classList.toggle("player-open", !!open);
})();
