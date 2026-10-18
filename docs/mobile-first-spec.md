# Mobile-first Tanqi: spec (draft)

Status: draft for review. Nothing here is built yet. Terms follow [GLOSSARY.md](../GLOSSARY.md).

## 1. Goal

Make the phone the primary design target: a Track can be found, played and controlled one-handed on a ~390×844 screen, and the same Dioxus UI scales *up* to the desktop window instead of the desktop UI being squeezed down.

"Mobile-first" has two halves that ship separately:

| Half | What it means | Risk |
|---|---|---|
| **A. Layout** | CSS and component structure are written for narrow touch screens first; desktop is the enhancement | Low, pure UI work |
| **B. Platform** | The app actually runs on Android (later iOS) | High: storage access, background audio, WebView audio limits |

A is valuable on its own and unblocks B. Do A first.

## 2. Non-goals (v1)

- iOS (needs macOS + Xcode; the dev machine is Linux). Icons exist in `icons/ios/`, so the door stays open.
- Lyrics analysis / Lyrics translation on-device (see §6.4).
- Cloud sync of Favorites or Playlists.
- Rewriting the audio engine (`assets/engine.js`) unless the §6.3 spike fails.

## 3. Where the code stands today

Facts from the current tree that shape the work:

- **Desktop-only shell.** `src/main.rs` uses `dioxus::desktop` (`WindowBuilder`, min size 900×600, `tq://` custom protocol). `Cargo.toml` enables only `desktop` + `router`.
- **Desktop-first CSS.** `shell.css` fixes `.app-main` at `max-width: 64rem; padding: 3rem 1.5rem`. Only a handful of `min-width` queries exist (library grid, EQ grid); the player bar hides its meter with a `max-width: 60rem` query. No viewport/safe-area handling, `#main { min-height: 100vh }`.
- **Top nav, 4 links + 2 buttons** (`navbar.rs`) in a 3.5rem bar with `gap: 2rem`: does not fit 390px.
- **Player bar is a full device** (`player_bar.rs`, `player-bar.css`): fixed 5.25rem row holding disc, display, meter, 4 keys, waveform seek, 4 tags, volume fader. Far too wide for a phone.
- **Hover-only actions.** Library row actions sit at `opacity: 0` until `:hover`/`:focus-within` (`library.css:74-76`). Invisible on touch.
- **Modals** are centred `.modal-scrim` dialogs sized in rem/vw (`eq-controls.css`, `song-info-modal.css`, `lyrics-modal.css`, `audio-analysis-modal.css`).
- **Floating status indicators** are fixed bottom-right (`status-indicators.css`) and will collide with a bottom tab bar and mini player.
- **Mini player** (`compact.rs`) resizes and pins the OS window. No meaning on mobile.
- **Desktop-only backend pieces:** `rfd` folder/file pickers (`commands.rs`), `notify-rust`, MPRIS (Linux `cfg`), Ollama at hardcoded `http://localhost:11434` (`ollama.rs:5`), Scan via `std::fs::read_dir` (`scanner.rs`).
- **Audio** is Web Audio: whole file fetched via `tq://localhost/file?path=…` and `decodeAudioData`'d into memory, with a second buffer prefetched for gapless (`engine.js`). On a phone WebView this is the main memory and background-playback risk.

## 4. Principles

1. **Base styles are the phone styles.** Write un-queried CSS for narrow screens; add `@media (min-width: …)` to enhance. Existing `max-width` queries get inverted.
2. **One component tree, not a second app.** Prefer CSS to branching in Rust. Branch in Rust only where the structure truly differs (tab bar vs top nav; player sheet).
3. **Touch is the baseline input.** No hover-only affordances; 44×44px minimum hit targets; no hover state is required to discover an action.
4. **Playback must never be interrupted by layout.** Same rule the shell already follows: the player lives above the router.
5. **Breakpoints:** base `<600px` phone · `≥600px` tablet / small window · `≥960px` desktop (matches the existing library grid steps at 560/760/960). Pick three and reuse them everywhere.

## 5. Layout spec (Half A)

### 5.1 App shell
- Add `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">` (via the custom head in `main.rs` / the mobile launcher).
- Replace `100vh` with `100dvh`; pad with `env(safe-area-inset-top/bottom/left/right)`.
- `.app-main`: base padding `1rem`, bottom padding computed from tab bar + mini player height; `3rem 1.5rem` only at `≥960px`.
- Lower or remove `with_min_inner_size(900×600)` so the desktop build can be resized to phone width for testing.

### 5.2 Navigation
- **Phone:** fixed bottom tab bar with Library · EQ · Lyrics · Settings (icon + label, 56px tall, safe-area aware). The wordmark moves to the Library page header.
- **≥960px:** current top nav, unchanged in look.
- Theme toggle moves into Settings on phone. "Mini player" button is desktop-only (hidden below 960px).
- Stacking order bottom-up: tab bar → mini player → content. Both are fixed; `.app-main` reserves both heights.

### 5.3 Player
Two states instead of one wide bar:

- **Mini player (collapsed, phone default):** one row above the tab bar. Cover thumb, title/artist, play-pause, heart; a 2px progress line on its top edge. Tap the row to expand.
- **Now Playing (expanded):** full-screen sheet. Large cover (the disc motif can stay as the cover frame), title/artist, waveform seek with times, previous / play-pause / next, then a row for heart, Play mode, Play queue, EQ, Info. Volume fader is hidden on phone (hardware volume), shown ≥960px. Swipe down or a chevron collapses it.
- **≥960px:** today's `PlayerBar` device row, unchanged.
- The spectrum canvas (`#tq-meter`) renders only in the expanded sheet or the desktop bar, and `engine.js` should stop drawing it when it is not visible (battery).
- Queue and EQ panels become bottom sheets on phone (they are absolutely positioned popovers now).

### 5.4 Library and lists
- Track rows: 56px minimum height, cover + title/artist, a visible `⋯` overflow button (play next, add to Playlist, Favorite, Info) replacing hover-reveal actions. Tap row = play.
- Library grid: 2 columns at base (today `library__grid` starts narrower and steps up at 560/760/960; make 2 the explicit base).
- Search field: full width, `font-size: 16px` (prevents iOS zoom-on-focus), sticky under the page header.
- Artist/Album drill-down keeps `tqLibNav` scroll restore; add hardware/gesture back support (see §6.5).

### 5.5 Modals and panels
- Phone: modals render as bottom sheets (full width, rounded top, `max-height: 90dvh`, scroll inside, drag handle / close button). Centred dialog from `≥600px`.
- Applies to: song info, lyrics modal, audio analysis modal, artwork modal, add-to-playlist, EQ panel, queue panel, onboarding gate.
- Keep the existing `player-open` contract so scrims stop short of the mini player; update the offset from "bar height" to "mini player + tab bar".

### 5.6 EQ page
`eq-controls` is a fixed `35rem` panel. On phone: bands as a horizontally scrollable fader strip with larger thumbs, or a vertical list of sliders; preset and preamp controls stacked above. Sliders must be touch-draggable (verify `pointer` events, not mouse-only handlers).

### 5.7 Status indicators and job queue
Anchor above the mini player (`bottom: calc(var(--chrome-bottom) + .5rem)`) with a single shared `--chrome-bottom` custom property so the tab bar / mini player heights are defined once.

### 5.8 Typography and density
Base font 16px; body copy ≥ 14px; metadata ≥ 12px. The mono/device styling stays, but no 10px text (`.625rem` in `player-bar.css` waveform times) on phone.

## 6. Platform spec (Half B, Android first)

### 6.1 Build target
- Add Dioxus `mobile` feature behind a cargo feature or `cfg` split; `desktop` and `mobile` entry points share `ui::App`.
- Move desktop-only deps behind `cfg(not(target_os = "android"))`: `rfd`, `notify-rust`, `mpris-server`, window APIs in `compact.rs`.
- `dx serve --platform android` works on this machine's installed targets (`aarch64-linux-android`, `x86_64-linux-android`); verify NDK/SDK first.

### 6.2 Custom protocol
`engine.js` already switches `tq://localhost` ↔ `http://tq.localhost` for Windows. Android WebView also needs the `http://tq.localhost` form; extend the UA check. Confirm Range requests are not required (current handler reads the whole file).

### 6.3 Audio engine spike (do early, it can invalidate the plan)
Questions to answer on a real mid-range phone:
1. Does `decodeAudioData` of a 40–60 MB FLAC succeed, and how long does it take? Peak memory with the gapless prefetch buffer?
2. Does playback continue with the screen off / app backgrounded? WebView audio is usually throttled or paused without a foreground service.
3. Can lock-screen / notification controls drive the existing Rust `Player` commands?

If (1) or (2) fail: switch Android playback to a native decoder (Symphonia is already a dependency) feeding a native audio out (Oboe/AAudio via `cpal`), keeping EQ in Rust. That is a large change; decide after the spike.

### 6.4 Storage and Scan
`rfd::pick_folder` and `std::fs::read_dir` over arbitrary paths do not work under Android scoped storage. Options:
- **(a) Permission + known dirs:** request `READ_MEDIA_AUDIO`, add `/storage/emulated/0/Music` etc. as Library folders, Scan with `std::fs`. Simplest; may not reach SD cards or other folders.
- **(b) Storage Access Framework tree picker** + content-URI reads via JNI. Correct and general, but `Local reference` (`local:`-prefixed *path*, the Track id and cover location) assumes real paths. That would need a design decision and an ADR.

Recommend **(a) for v1**, keep `Local reference` unchanged.

### 6.5 Media session and lifecycle
- Android `MediaSession` + foreground service, replacing the Linux-only MPRIS layer (`src/mpris/`) behind the same `NowPlaying` / `MprisCommand` shape so `use_mpris` stays.
- Handle system Back: close sheet → collapse Now Playing → pop Library drill-down → exit.
- Audio focus (pause on call, duck on notifications), headphone-unplug pause.

### 6.6 Lyrics analysis and Ollama
Ollama is hardcoded to `localhost:11434`; a phone has no local model. v1: hide Lyrics analysis / translation actions on mobile unless a **remote Ollama URL** setting is configured; add that setting (also useful on desktop). Already-saved analyses remain viewable.

### 6.7 Audio analysis (EQ suggestion)
FFT/BPM analysis is CPU-heavy. On mobile: user-triggered only, one Track at a time; bulk library analysis off by default or limited to when charging. Reuse the existing job queue.

### 6.8 Network and data
Artwork/metadata lookups (iTunes, MusicBrainz, lrclib) already go over rustls. Add a "Wi-Fi only" setting for artwork and lookups; cache behaviour unchanged. Android needs `INTERNET` and media permissions in the manifest.

## 7. Phases

| Phase | Scope | Done when |
|---|---|---|
| **0. Test harness** | Lower min window size; viewport meta; `--chrome-bottom` custom property | Desktop window can be resized to 390px wide without horizontal scroll |
| **1. Phone layout** | §5.1–5.4: shell, tab bar, mini player + Now Playing, track rows, library grid | All four pages usable at 390×844 and 360×640; desktop at ≥960px looks as it does today |
| **2. Sheets and EQ** | §5.5–5.8 | No centred dialogs on phone; EQ usable by touch |
| **3. Android spike** | §6.1–6.3 | A debug APK plays a Track on a real device; go/no-go on the WebView audio engine |
| **4. Android v1** | §6.4–6.8 | Scan, play in background, lock-screen controls, Favorites/Playlists work |
| **5. iOS (optional)** | needs macOS | Out of scope until requested |

Phases 0–2 are shippable to desktop users and need no Android toolchain.

## 8. Acceptance criteria

- No horizontal page scroll at 360px, 390px, 430px, 768px, 1200px widths.
- Every interactive element ≥ 44×44px on phone; none reachable only by hover.
- Playback survives navigation, collapsing/expanding Now Playing, and rotation.
- Safe areas respected (notch, gesture bar): nothing is covered by system UI.
- Desktop (≥960px) is visually unchanged apart from intentional fixes. Take before/after screenshots using the existing `TANQI_START_ROUTE` / `TANQI_THEME` dev hooks.
- Both themes (dark, silver) verified at phone width.

## 9. Open questions

1. **Scope of "mobile":** responsive layout on desktop only (Half A), or a real Android app (Half B), or both? This spec assumes A first, then B.
2. **Android only, or iOS too?** iOS changes the toolchain and storage story.
3. **Library source on phone:** music already on the device (v1 assumption), or streaming from the desktop machine over the network? The latter is a different product (server mode).
4. **Is the "device" look sacred?** The disc, tape strip and fader are distinctive. On phone I keep the disc as the cover frame and drop the rest; confirm that is acceptable.
5. **Lyrics analysis on phone:** drop, or support a remote Ollama URL?
6. **Minimum Android version:** assumed Android 10+ (API 29), which fixes the storage and permission model.

## 10. Android spike results (2026-10-16)

Ran on the x86_64 emulator (Pixel-class, API 36, 2 GB RAM) with a debug build; no physical device yet. Sections 6.1-6.3 of this spec.

### Build (6.1): works
- `dx` must be 0.7.x to match `dioxus = "0.7"` (the installed 0.6.3 did not). Installed 0.7.10.
- Features are now `desktop` (default) and `mobile`; `rfd` and `notify-rust` are `cfg(not(android))`; `dioxus::desktop` is reached through `crate::host`. Desktop is unaffected.
- Build and run:
  ```bash
  export ANDROID_HOME=$HOME/Android/Sdk ANDROID_NDK_HOME=$ANDROID_HOME/ndk/26.3.11579264 JAVA_HOME=$HOME/Android/jdk17
  dx build --platform android --no-default-features --features mobile --target x86_64-linux-android   # or aarch64-linux-android for a phone
  ```
  APK: `target/dx/tanqi-music/debug/android/app/app/build/outputs/apk/debug/app-debug.apk`. About 124 MB debug and about 700 MB installed (a release build will be far smaller). The emulator's data disk filled up once.
- `[android.permissions]` in `Dioxus.toml` adds READ_MEDIA_AUDIO and READ_EXTERNAL_STORAGE.
- The Android data dir has no `dirs` answer; `app::app_data_dir` falls back to `/data/data/<package>/files`. Database creation works.

### Protocol (6.2): needed a fix
Dioxus serves the Android page from `https://dioxus.index.html`, so the `tq` protocol must be `https://tq.localhost`. `http://` is blocked as mixed content ("Failed to fetch"). Fixed in `engine.js` and `ui/util.rs`. Whole-file reads work; no Range support was needed.

### Storage and Scan (6.4): works with a granted permission
With READ_MEDIA_AUDIO granted through `adb`, Scan of `/storage/emulated/0/Music` found both test tracks with correct tags. Not done yet: the runtime permission prompt (JNI) in the app. Until then the permission has to be granted by hand.

### Audio engine (6.3): promising, with caveats
| Question | Result |
|---|---|
| Decode a 6 min, 37 MB FLAC | Works. About 0.9 s on the emulator host, 138 MB decoded (48 kHz stereo float). Real phones will be slower; measure on a device. |
| Memory while playing | App process about 230 MB, WebView renderer about 310-390 MB RSS. The gapless prefetch adds a second buffer, so two long tracks could reach about 300 MB of audio alone. |
| Background and screen off | Playback continued and the position advanced in real time for at least 30 s. But Android logs `AudioHardening background playback would be muted ... level: partial`, so a foreground service and a MediaSession are still required for real use. |
| Lock-screen controls | Not built. `MediaSession` is still the §6.5 work. |

Decision: keep the WebView audio engine for now. Revisit if a real device shows decode stalls or memory kills on long tracks (options in 6.3).

### Open issues found
- After the first folder is added the Library stayed on "No music yet" until the app restarted (the Scan had completed). Check whether desktop does the same.
- Android's WebView updater can kill the app while it installs; harmless but looks like a crash in the log.

## 11. Permission prompt, foreground service and MediaSession (spec 6.4-6.5)

Built and checked on the same emulator (API 36, x86_64). Still no physical device.

### What was added
- **`android/MainActivity.kt`** (dx copies it over the generated file via `[application] android_main_activity`): asks for READ_MEDIA_AUDIO (READ_EXTERNAL_STORAGE below API 33) and POST_NOTIFICATIONS at launch; `PlaybackService` (foreground service, type `mediaPlayback`) with a `MediaSession`, a media-style notification (previous / play-pause / next) and cover art; a `TqAndroid` JS bridge.
- **`android/AndroidManifest.xml`** (via `android_manifest`): the permissions and the `<service>`. dx's `[android.raw] application` is not rendered by its template, so a full manifest is used instead.
- **Rust side** (`ui/player.rs`): `push_now_playing` sends the same `NowPlaying` state MPRIS gets to `TqAndroid.nowPlaying(...)`; `window.tqNativeCmd` feeds lock-screen and notification buttons into the existing `apply_mpris`, so the actions are the MPRIS ones. `NowPlaying` is now `Serialize` and `MprisCommand` `Deserialize`.

### Results
| Check | Result |
|---|---|
| Permission dialogs on first launch | Notifications, then audio; both granted through the UI |
| Library after the first folder is added | Refreshes straight away (it needed a restart before) |
| Foreground service | Running with type `mediaPlayback`; notification is `NO_CLEAR` while playing |
| System media controls (shade) | Title, artist, play/pause, seek bar and previous shown |
| `media_session dispatch pause / play / next` | All reach the page and the player: position froze, resumed, and the second track played with its metadata |
| Screen off after playing | Playback continued; no "background playback would be muted" warning this time |

### A bug this uncovered, now fixed
A Scan of a folder the app may not list used to read as empty, so the store would have dropped every Track. `FsScanner::walk` now returns an error when the folder can't be read, so a missing permission leaves the library alone. After a grant, Rust re-runs the Scan and the Library reloads.

### Not covered
- Real-device behaviour (battery optimisation, OEM background limits, Bluetooth buttons, audio focus on calls).
- Audio focus and "becoming noisy" (headphones unplugged) pause: not implemented yet.
- Cover art in the notification only works for `file://` covers; remote artwork URLs are skipped.
- Denied permission: the app does not yet explain why the Library is empty or link to settings.
- ~~The revoke-and-relaunch check~~ **Wrong, corrected in section 12:** the `read_dir` guard above did not protect the library. Without the permission Android still lets the app list the folder but hides the audio files, so the Scan saw an empty folder and removed every Track.

## 12. Audio focus, headphones, permission refusal (2026-10-16)

Same emulator (API 36, x86_64), still no physical device.

### A data-loss bug in section 11, now really fixed
With READ_MEDIA_AUDIO refused, `read_dir` on `/storage/emulated/0/Music` succeeds but returns no audio files. The startup Scan therefore reconciled against an empty folder and deleted all Tracks (reproduced: 0 rows after a refused launch). Section 11's "can't read" check was not enough. Now:
- `scanner::set_storage_readable(bool)`: while `false`, `FsScanner::walk` refuses every Scan, so nothing is dropped. It is always `true` on desktop.
- On Android the backend no longer Scans at startup; the UI does it once it has checked the permission (`use_mpris` in `ui/player.rs`), and again after a grant at the prompt or in Settings.
- Verified: grant, add folder (2 Tracks) -> revoke -> relaunch and refuse both prompts -> still 2 Tracks.

### Permission refusal
- Kotlin reports `permissionDenied`; the page also re-checks `TqAndroid.hasAudioPermission()` on launch and every time the app resumes (`appResumed`), because Android stops showing the prompt after a refusal.
- The Library shows a banner ("Tanqi can't read your music...") with **Open settings** (the app's info page). After granting there and returning, the banner is gone and the Library Scans again.

### Audio focus (`PlaybackService`)
The service requests focus (`USAGE_MEDIA`, music) while playing and answers changes by telling the page what to do. Tested with a small program that takes focus the way another app would (`app_process`, source kept out of the repo):
| Other app asks for | Result |
|---|---|
| Transient focus (call, alarm) | Tanqi pauses, and resumes by itself when the other app lets go |
| Transient, may duck (notification sound) | Keeps playing; `tqEngine.duck(0.25)`, then `duck(1)` afterwards (new in `engine.js`) |
| Permanent focus (another music app) | Tanqi pauses and stays paused |

### Headphones unplugged
A receiver for `AUDIO_BECOMING_NOISY` pauses playback; it is registered while the service runs (checked with `dumpsys`). The emulator cannot send that protected broadcast, so the pause itself is **untested**. Needs a real device with headphones or Bluetooth.

### Build notes
- A stale `jniLibs/arm64-v8a` from the first build doubled the APK; delete it when switching targets.
- `[profile.android-dev]` now has `debug = false`, `strip = "debuginfo"`; the unstripped library was about 490 MB.

### Still open
Real-device testing (OEM battery limits, Bluetooth buttons, actual unplug), notification cover art for remote URLs, and iOS.

## 13. Library tools drawer and Folders popup (2026-10-16)

- **Phone:** the Library's tool buttons moved behind a gear beside the title that opens a bottom drawer (below 960 px; desktop keeps the buttons in the header).
- **Folders popup** (`components/folders_modal.rs`, all sizes): each Library folder with its path and track count (`LibraryStore::folder_stats`), **Remove** (asks twice), **Rescan** and **Add folder**. The standalone Add folder button is gone; the empty Library's button opens this flow.
- **Add folder on Android:** there is no native folder dialog, so the popup browses the device itself (`commands::list_subfolders`, starting at `/storage/emulated/0`, up to `/storage`): sub-folders, an "N audio files here" count, and **Use this folder**. On desktop it still opens the native dialog.
- Checked on the emulator: popup list, Add folder -> browser, navigating into Music. Not yet checked: adding a second folder, Remove, and the empty-Library path.

## 14. Track actions sheet (2026-10-16)

On a phone, a Library row's `⋯` opens a bottom sheet (`components/track_actions_sheet.rs`) instead of expanding the row: the Track's cover, title and artist with a heart, and tiles for Play next, Add to queue, Add to playlist (which switches to the playlist list with a "New playlist" field) and Track info. The row keeps only the heart and `⋯`. Checked on the emulator in the Silver theme; picking an existing playlist and adding to the queue were not exercised.

## 15. Tab bar and confirmations (2026-10-16)

- **Tab bar:** restyled as a row of keys (icon above a mono uppercase label) in the player bar's metal, with the active tab as a bright key (dark with an orange underline in Silver). Hardcoded colours on purpose, so `build.rs` derives the Silver skin from them.
- **Confirm popup** (`components/confirm_modal.rs`): Analyze audio and Analyze lyrics now ask first, naming the track count and the cost, and Cancel queues nothing. A centred dialog on desktop, a bottom sheet on a phone. On Android the lyrics one also warns that the local AI model (Ollama) usually can't run on a phone. Checked on the emulator; the desktop dialog was not looked at on screen.

## 16. Drawer stays under the confirmation; Danger zone (2026-10-16)

- Tapping Analyze audio / Analyze lyrics in the Library tools drawer no longer closes the drawer first: the confirmation sits above it (`.confirm` z-index 90 over the drawer's 80), Cancel returns to the drawer, and confirming starts the job and closes both.
- **Settings -> Danger zone:** "Delete library data" moved out of the Library data card into a red-bordered Danger zone at the bottom of Settings. It opens the confirmation popup with a red confirm button instead of the old arm-for-4-seconds button, which was easy to mis-tap on a touch screen. The text now says exactly what goes (folders, Track index, saved analyses, translations, artist pictures) and what stays (music files, Favorites, Playlists). Checked on the emulator in Dark and Silver; the delete itself was not run.

Update: the whole former "Library data" card now lives in the Danger zone, as three rows: the size on disk, Remembered lookups (with **Clear lookups**), and **Delete library data** (with the confirmation popup). Checked on the emulator in Silver.

## 17. Single-track audio analysis is a queued job (2026-10-16)

Analyzing one Track's audio (Track info -> Analyze audio, and Re-analyze) no longer runs inside its window. It enqueues a `JobKind::AudioTrack` job (`jobs::audio_analysis_job`), like the lyrics translation and deep dive: it waits its turn behind other work, shows in the jobs list, can be cancelled, and survives the window being closed; the window waits for the job and then reads the saved analysis. A separate kind keeps the Library's bulk "Analyze audio" button from greying out for it. Checked on the emulator. Not changed: "Upload local file" on the EQ page still runs on its own (its result is never saved) with its bottom-right indicator.

## 18. Now Playing: no spectrum, icon tags (2026-10-16)

- The spectrum meter no longer shows in the phone's Now Playing screen (it stays in the desktop player bar), freeing the space for the keys.
- Shuffle, repeat and mute are icons everywhere (player bar on desktop and phone, and the mini-player window): shuffle, repeat (with a small 1 for repeat-one) and a speaker / crossed-out speaker. The "VOL 100" label is gone; the speaker is a mute toggle and its tooltip gives the volume. The title card is full width on the phone.

## 19. Volume knob (2026-10-16)

The volume fader (`VolumeFader`) is replaced by a rotary `VolumeKnob` (`components/device.rs`) in the desktop player bar and the mini-player window: a metal cap that turns from 7 o'clock (silent) to 5 o'clock (full), a pointer notch, 21 ticks round it that light up to the level (orange on Silver) with the volume in its tooltip (no number printed on the cap). Turn it by dragging round it (a see-through layer keeps it turning if the pointer leaves the knob), with the wheel (3% a notch), or with the arrow, Page and Home/End keys; it is a proper `slider` for screen readers. Muting shows it at zero, and turning it unmutes, like the fader did. Hidden on the phone, where the hardware keys set the volume. Checked on a desktop test window in Dark and Silver with scripted mouse events; not tried with a real drag, the wheel or the keys, and the mini-player window version was not looked at.

## 20. The last session is remembered (2026-10-16)

The Play queue, which Track is current and the position in it are now kept in the webview's storage (`tanqi-last-queue`, `tanqi-last-position`; the position every 5 s while playing and on pause) and restored on the next launch: the player bar comes back showing the same Track, paused at the same time. Nothing is loaded until play is pressed, then it starts from the saved position (`Player::resume_at`). Seeking before pressing play moves that start point. Tracks that have left the Library since are dropped; if the current one is gone the next one in its place comes back from the start; in the last 3 seconds of a Track it restarts from 0. Saving waits until the restore has looked, so a fresh start can't erase it. Checked on a desktop test window (quit, reopen, press play). Not yet tried on Android, and shuffle and repeat are not remembered. `GLOSSARY.md` no longer says the Play queue is lost when the app closes.

Found and fixed while testing: pressing play on a restored session first panicked (a `peek()` borrow held across the call that clears it).

## 21. Re-analyze stays in the same window (2026-10-16)

In the EQ analysis window, Re-analyze no longer swaps the whole body for a loading line (which shrank the window into what looked like a different, smaller one). The current result stays on show, dimmed and not clickable, under a "Re-analyzing ..." line with the loading dots, and is replaced when the new analysis arrives. The first analysis of a Track (nothing to show yet) still shows just the loading line. Checked on the emulator.

## 22. Drawers on every page, playlists, and the phone mini player (2026-10-16)

- **The ⋯ drawer works on artist and album pages.** The rise animation on those blocks used `animation-fill-mode: both`, which kept a transform alive after it ended, and a transformed element is the containing block for `position: fixed` children: the drawer opened *inside* the album block (and Track info with it was cramped) instead of over the screen. It is now `backwards`. The Tracks tab was unaffected, which is why this was missed.
- **Playlist pages on a phone:** the rules that hide a row's extra buttons were global and hid the Playlist page's move up / move down / remove / play buttons. They now apply only to Library rows (`library__row--sheet`); a Playlist row keeps its own buttons and plays on tap.
- **Adding to a Playlist from the drawer silently did nothing:** the add ran as a task owned by the drawer, which closed (and cancelled it) at once. The Playlist operations now run on the root scope (`spawn_forever`).
- **Phone mini player:** the expand arrow is gone (tap the card to open Now Playing); in its place are previous and next next to play/pause. A title or artist wider than the card scrolls to the end and back while playing (`marquee--long`, set by a small measurement after each track change); paused, it is clipped as before.
- Known gap: the Android Back button leaves the app from anywhere (it does not close a drawer or step back out of an album).

Update: the scrolling title now slides an inner `span` with a `transform` (compositor-driven, smooth) instead of animating `text-indent` (which jumped); a title that is too long fades out at its right edge instead of showing an ellipsis. While any drawer or sheet is open (`.modal-scrim`, the Library tools drawer, Now Playing) the page behind is locked (`html:has(...) { overflow: hidden }`, phone only) so it can't scroll or be dragged. Checked on the emulator by reading the page's computed overflow with a drawer and with Track info open.

Update: the scrolling title is now a ticker, not a back-and-forth. A second copy of the text follows it (`::after` using a `data-text` attribute, 48px apart), the pair slides left at about 30 px/s with a short hold at the start, and the loop restarts on an identical frame, so the text leaves on the left and comes back in from the right. Checked on the emulator over six frames.

## 23. Artwork and lyrics on a phone (2026-10-17)

- **Artwork window:** Download and Close are now icon buttons on the picture's top-right corner (they were a row of text buttons below the picture and the size buttons, which many resolutions pushed under the player bar). The sizes are one horizontally scrolling row (centred when they fit), the picture is capped to the screen height, and the window sits above the player bar (z-index 90) with even padding. Checked on the emulator with 13 sizes and a track playing, using the real stylesheet on injected markup (the live window needs an online cover).
- **Lyrics window:** on a phone it is a full-screen reader above the player bar and tab bar. When the analysis is shown, Lyrics and Analysis are two tabs of one column instead of two cramped columns (`lyrics-modal__tabs`, driven by a `tab_analysis` signal; desktop keeps the side-by-side split and hides the tabs). Type is 16px with a roomier line height, the header is one compact row, and the footer clears the gesture bar. Checked on the emulator with injected markup for both tabs; the live window needs real lyrics and the AI analysis, which the emulator can't produce.

## 24. Queue look; artwork size picker (2026-10-17)

- **Queue panel:** now a metal frame round a sunken well; the playing row has a bright bar and a lit number chip (orange on Silver); each row's move up / move down / remove are small raised keys, larger on a phone. Checked on a desktop test window in Dark and Silver (not on the phone).
- **Artwork window:** the row of size buttons (which needed a scrollbar) is replaced by a size dropdown on the picture's top-left corner, opposite Download and Close, so nothing scrolls. Built and compiled; not seen live (needs an online cover).

## 25. Why audio analysis was slow on the phone (2026-10-17)

The Android APK is built with the `android-dev` profile, which inherits `dev`: no optimisation (`opt-level 0`). The analysis (decoding the whole file, FFTs for the band levels, tempo and key) is a tight numeric loop, which runs an order of magnitude slower unoptimised. Measured on the emulator, same 6-minute FLAC, forced re-analysis: **24.9 s unoptimised, 1.7 s with `opt-level = 3`** (about 15x), now set on `[profile.android-dev]`. A phone's CPU is slower than the desktop one hosting the emulator, so expect its numbers to be a few times higher, but in the same proportion. Not measured on a real phone. A proper release build (still to do) will be smaller and faster again.

## 26. Analysis out-of-memory crash; artwork window width (2026-10-17)

- **Crash on a phone (Xiaomi, Android 16):** `rust_oom` abort inside `stratum_dsp::features::chroma::harmonic_spectrogram_time_mask`, from `tempo::detect`. stratum-dsp allocates spectrogram tables proportional to the audio it is given, and `detect` handed it the whole track. Now tempo and key are read from a **90 s excerpt from the middle** of the track (`tempo::excerpt`, with tests); band levels still use the whole track. Measured on desktop with a 30-minute FLAC: peak memory **9.0 GB -> 750 MB**, time **51 s -> 2.6 s**; a 6-minute song of pure noise is unchanged (no rhythm to analyse). The result for a long track can differ from before (tempo from the middle, not the whole). Still heavy for very long files (a 30-minute track holds its decoded samples, ~320 MB): decoding in chunks would fix that if it matters. `examples/analyze_probe.rs` runs the analysis on one file and prints the time.
- **Artwork window:** on a phone the window shrank to the picture's size, so a 100x100 picture made a 100px window and pushed the buttons out of reach. It now has a fixed width (`min(100%, 32rem)`); the picture fills it at its own aspect ratio. Checked on the emulator with a 100x100 image.

## 27. Notifications (2026-10-17)

- **Android had none:** `app::notify` was a stub on Android. Job-finished notifications now go from `ui/api.rs::send_notification` (after checking the Settings switch, `commands::notifications_enabled`) to `TqAndroid.notify(title, body)` in `android/MainActivity.kt`, which posts a normal notification that opens the app when tapped. Its channel is `jobs_popup` at **high** importance: the first try used default importance, which plays the sound but never shows the heads-up banner (a channel's importance cannot be raised after creation, hence the new id). Needs the notification permission asked for at launch. Checked on the emulator (system lists it at importance 4); the banner itself was not caught on screen.
- **Linux: sound but no pop-up (Cinnamon):** the app's own D-Bus message is identical to one that pops up when sent by a small test program, yet it never pops up, focused or not. So the desktop treats notifications from the Tanqi process (which has a window) differently. `app::notify` now runs `notify-send --app-name=Tanqi` first (its pop-ups work) and falls back to the in-process notification if `notify-send` is not installed. Verified end to end: finishing a real single-track analysis in the app showed the pop-up. `examples/notify_probe.rs` sends test notifications in several ways.

## 28. Analysis reaches the playing song; Android Back button (2026-10-17)

- **A song playing while its analysis finishes now gets the result.** The player loaded the song without an analysis and only looked one up when a song *starts*, so the suggested EQ appeared only after playing it again. `use_player_provider` now watches the job list: when an audio analysis job (bulk or single) finishes and the current song has none, it fetches it and applies the suggested EQ and loudness (unless you already adjusted the EQ). Checked on the emulator: before, the EQ panel said not analyzed; after the analysis finished, the same song (still playing) showed its suggested bands.
- **Back button (Android):** `window.tqBack()` in `boot.js`, called by `MainActivity`'s back callback, takes one step per press: an open sheet or window, the queue / lyrics / EQ panel, the full-screen player, a Library page (artist, album, playlist), then the Library tab. When already at the Library's top level it returns false and the activity finishes (music keeps playing through the foreground service). Checked on the emulator for each step.
- **Known limit found while testing:** playing a very large file (a 205 MB, 30-minute FLAC) crashes the app with a Java `OutOfMemoryError` while serving the file to the page (the whole file is copied into one Java array; the heap limit is about 192 MB). Normal songs are far below that. Fixing it needs range-based streaming for the `tq` protocol.

## 29. Ollama address setting (2026-10-17)

The Ollama address was hardcoded to `http://localhost:11434`, which can never work on a phone. It is now a setting (`AppSettings::ollama_url`, stored as `ollama_url`, default unchanged) with a field and Save button above "Ollama model" in Settings. `models::clean_ollama_url` tidies what is typed (adds `http://`, adds `:11434` when no port, drops a trailing `/`; tested), and the address is passed to every Ollama call (`check_connection`, `analyze_lyrics`, `translate_lyrics`); the "can't reach" message names the address used. A test checks requests go to the saved address. Checked end to end on the emulator: default `localhost` fails there, typing `10.0.2.2` (the emulator's name for the host PC) connects and finds `qwen3:8b`. Not yet tried from a real phone over Wi-Fi. Ollama on the PC listens on `127.0.0.1` only by default, so it must be started with `OLLAMA_HOST=0.0.0.0` (and the firewall must allow port 11434) for another device to reach it; Ollama has no login, so only do this on a network you trust.

## 30. Card details as an icon (2026-10-17)

On the EQ page's analysed-song cards, the "Details" text button is now an info icon (the same one as Track info) in the card's top-right corner, 22px with a 36px target (44px on a phone); the title leaves room for it so it never runs under the icon. Checked on desktop and on the phone emulator at the earlier, smaller size; the larger size was only built, not looked at.

## 31. Upload local file removed; more icons (2026-10-17)

- **"Upload local file" (EQ page) is gone**, with everything only it used: the `local_audio` state, the bottom-right local-analysis indicator and its CSS, `pick_local_audio_file` and `analyze_local_audio_file` (api and commands), and the file-picker helper. The EQ page intro lost its sentence about local files. The analysis window's `local_file_path` parameter is now always `None` (left in place).
- **Icons instead of words:** Close in Track info, the analysis windows, the lyrics window, the EQ panel and the folders popup; Clear in the queue (bin); Remove in the folders popup (bin); the Library tools (folder, rescan, picture, person, pulse, speech bubble; in the phone drawer the icon sits above the label); icon plus label on Rescan / Add folder / Use this folder, the Settings Save buttons, Clear lookups and Delete library data (a destructive action keeps its words); the Ollama address Save is a tick. New icons: bin, folder, rescan, picture, person, pulse, tick. The Back-button helper now finds the EQ panel's Close by its aria-label. Checked on the phone emulator (drawer, folders popup, Settings, Track info); the desktop window was not re-checked.

Update (2026-10-17): the page lock behind an open window / drawer now applies on desktop too (it was inside the phone-only media query): `html:has(.modal-scrim)` etc. `overflow: hidden` plus `scrollbar-gutter: stable` so the page does not shift sideways. Verified on a desktop test window with real mouse-wheel events (X11 XTest): without a window the wheel moved the page 0 -> 730; with Track info open, 16 notches over the dimmed page and over the window left it unchanged.

## 32. Clicking a notification goes somewhere (2026-10-17)

Every job's notification now carries a target (`JobSpec::target()`; single-track jobs set it with `.opens(...)`): the page for its kind (`eq`, `lyrics`, `library`), or, for one song's audio analysis, deep dive and translation, `eq:<track id>` / `lyrics:<track id>`. `ui/targets.rs` holds the `OpenTarget` signal; the Shell navigates to the page, and the EQ / Lyrics page opens that song's analysis window. Failed jobs lead to the same place.
- **Desktop (Linux):** `notify-send --action=default=Open` waits and prints `default` when the pop-up is clicked; that sends the target on `app::notification_clicks()`, and the window is raised. Verified with a real mouse click on the pop-up (X11 XTest): the app came to the front, went to EQ and opened the song's analysis. Without `notify-send` the fallback notification has no click action.
- **Android:** the notification's `PendingIntent` carries the target (`tq_target`, one request code per notification); `MainActivity` keeps it (`onCreate` / `onNewIntent`) and the page takes it with `TqAndroid.takeTarget()` when it comes to the front. Verified on the emulator with the app in the background: tapping the notification opened EQ with the analysis open. The `lyrics:` / `library` targets were not clicked through live (the route choice is unit-tested).

Update: the rows in the jobs list (the "All done" / "N in queue" chip) are links too. `Job` now carries its `target` (the same one its notification uses); clicking a row opens it and closes the list, while the × / Cancel buttons only dismiss or cancel (they stop the click). Verified on a desktop test window: ×  only dismissed (stayed on Library); a click on the row went to EQ with that song's analysis open. Not checked on the phone emulator (the code is shared).
