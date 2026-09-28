<!-- PAGE_ID: hark_12_desktop_ui -->
<details>
<summary>Relevant source files</summary>

- [Theme and elevation](../../crates/hark-app/src/theme.rs)
- [Window shell](../../crates/hark-app/src/ui/shell.rs)
- [Page routing](../../crates/hark-app/src/ui/pages.rs)
- [Grouped Settings](../../crates/hark-app/src/ui/settings/sections.rs)
- [First-run setup](../../crates/hark-app/src/ui/settings/onboarding.rs)
- [Overlay viewport](../../crates/hark-app/src/overlay.rs), [feedback state](../../crates/hark-app/src/overlay/feedback.rs), and [painting](../../crates/hark-app/src/overlay/paint.rs)
- [Native tray](../../crates/hark-app/src/tray/mod.rs)
- [Meetings page](../../crates/hark-app/src/ui/meetings/mod.rs), [detail view](../../crates/hark-app/src/ui/meetings/detail.rs), and [sharing](../../crates/hark-app/src/ui/meetings/share.rs)
- [Meetings settings](../../crates/hark-app/src/ui/settings/meetings.rs)
- [Meeting controller](../../crates/hark-app/src/meeting.rs) and [detection prompt viewport](../../crates/hark-app/src/meeting_prompt.rs)

</details>

# Desktop UI

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md), [Data Storage](../core/DATA_STORAGE.md), [Updates and Autostart](UPDATES_AND_AUTOSTART.md), [Meetings](MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_overview -->
## Overview

Hark uses native `eframe`/`egui` for its window and floating dictation feedback, and `tray-icon` for its system menu. There is no webview. The main window can remain hidden while push-to-talk runs from the tray.

The main thread owns egui and the Windows/macOS tray. On Linux, libappindicator owns GTK widgets on a dedicated thread with its own loop. Audio, hotkeys, transcription, cleanup, and insertion run on worker threads. The UI consumes state; it never delays insertion to render feedback.

On Windows, the UI also exposes meeting transcription: a Meetings page, a tray entry to start or stop taking notes, and a non-modal prompt when a meeting app starts using the microphone. Meetings is hidden everywhere its capture is unsupported (currently macOS and Linux); nothing about it changes how push-to-talk dictation looks or behaves.
<!-- END:AUTOGEN hark_12_desktop_ui_overview -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_tray -->
## Tray Daemon

The native menu groups voice choices under **Voice**, followed (where meetings exist) by a **Start meeting notes** / **Stop meeting notes (since HH:MM)** entry and a separator, then **Open Hark**, **Settings…**, and **Quit Hark** on every platform. Voice choices still apply immediately, update the Settings draft, and persist. Double-clicking the icon also restores the current page where the platform supports it.

| State | Icon | Meaning |
|---|---|---|
| Idle | Accent ring | Ready for the configured shortcut |
| Recording | Red disc | Capturing audio |
| Processing | Accent disc | Processing a dictation or loading the local model |
| Needs key | Amber disc with an exclamation mark | Missing or rejected key |
| Error | Red disc with an exclamation mark | Last dictation failed |
| Stopped | Gray disc with an exclamation mark | Pipeline is not running |

Tooltips name the state and shortcut. Quiet-audio hints preserve the ready icon and explain the issue in text. Updates reach the OS only when something changes, avoiding repeated icon writes and channel traffic.

A recording meeting is shown on an otherwise-idle tray as the same red recording disc, with a tooltip naming the time notes started ("Hark: taking meeting notes since 14:30"); once a detected meeting's app hangs up, the tooltip switches to when the notes will stop ("Hark: the call ended; meeting notes stop at 14:52:07"), matching the Meetings page's notice and its **Stop now** button. A start or stop *time* is shown rather than a countdown so a hidden, idle window never has to wake each second to update it — the recording state has to stay visible for as long as a meeting runs, but a dictation's own recording, processing, or error state still takes priority while it lasts. Where meeting capture does not exist on the platform (macOS, Linux for now), the menu builds without the entry at all rather than shipping one that can never work; where it exists but is off or failed to start, the entry stays in the menu, disabled.
<!-- END:AUTOGEN hark_12_desktop_ui_tray -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_overlay -->
## Recording Overlay

The floating capsule is a persistent, nonactivating 192 × 46 point viewport near the bottom of the active monitor on Windows. Other platforms use the monitor size supplied by egui. It never takes focus from the insertion target or appears in the taskbar.

| Feedback | Source | Lifetime |
|---|---|---|
| Listening… with a live waveform | Worker capture flag and microphone meter | While recording |
| Processing… | Pipeline processing event | Until the next event |
| Loading model… | Local-engine loading event | Until the next event |
| Inserted | Successful insertion event | One second |
| Check microphone / Check provider / Couldn't insert | Corresponding failure stage | Three seconds |
| No speech heard | Quiet-audio gate | Three seconds |

The feedback snapshot contains only a state and timestamp, never transcript text. The child reads it directly and expires terminal feedback on its own pass, even if the root window is asleep. It sleeps while hidden; while visible it repaints at about 30 FPS. A stopped pipeline disables its snapshot so late events cannot resurrect an old pill.

The viewport is created hidden once per pipeline run and reused for dictations. Windows clips and strips its native frame before revealing it. Mouse passthrough is deliberately disabled because layered-window composition breaks the transparent margins on that platform. Do not reintroduce a window per dictation or make hiding depend on the parent painting.

The meeting detection prompt ("Teams is using your mic. Take meeting notes?") follows the same rule for the same reason: one persistent, deferred viewport, registered from root `logic` so it keeps working while the main window is hidden in the tray, created hidden and only ever shown or hidden — never rebuilt per detection. It is non-modal, never steals focus from the meeting app, offers Start / Not this meeting / Settings, and dismisses itself after 30 seconds with no answer. **The root shows and places it, not the prompt itself:** eframe 0.36 runs a hidden deferred viewport's UI callback only while egui considers it visible, and that is derived from minimized/occluded state rather than from whether the window is shown, so a prompt that revealed itself from its own callback appeared for one call and never for the next. `App::logic` sends `Visible(true)` to its viewport and moves and sizes the window through Win32 in the target monitor's physical pixels; each step (showing, placed, painted, answered, withdrawn, and "shown but never painted") is logged as a label.
<!-- END:AUTOGEN hark_12_desktop_ui_overlay -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_window -->
## Settings Window Shell

A top bar contains Hark, History, Meetings (where meeting capture exists on the platform), Spellbook, Invocations, Stats, and Settings. Selected tabs have a raised surface; keyboard focus has a separate visible ring. Content is centered at up to 860 points and contracts with the window.

The footer remains visible with the actual pipeline state, configured shortcut, and active transcription/cleanup models. Long model text truncates with the full value available on hover. A key-related issue opens Dictation settings.

Settings' Save changes / Discard bar stays outside its scroll area. The update banner uses a tinted surface and opens the Updates section directly. Destructive confirmations have stronger elevation, explain the consequence, and initially focus Cancel.
<!-- END:AUTOGEN hark_12_desktop_ui_window -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_pages -->
## History, Spellbook, Invocations, Stats and Settings

- **History** keeps grouped days, search, copy/delete actions, expandable raw transcripts, timing, and Spellbook selection handoff. Wide rows separate timestamps from content. Captions name the actual provider/model and show cleanup only when it ran. Clearing history preserves lifetime statistics.
- **Meetings** (Windows only for now) is shaped like History: a searchable list of past meetings by title/date/duration, and a detail view with the transcript (speaker chips, timestamps), notes with checkable action items, speaker rename, a Share menu for text, Word, subtitles, audio excerpts, and Windows text sharing, and delete. While a meeting is recording, the page instead shows a live pane: the rolling Me/Them transcript, elapsed time, and a Stop button. A first-run notice reminds the user that some places require every party's consent to record a call; an optional button pastes a canned "I'm using Hark to transcribe this meeting" line into the focused chat.
- **Spellbook** has a raised vocabulary surface, editable terms, aliases, the advanced mishearing control, and undo for the most recent addition. Edits still persist immediately.
- **Invocations** retains trigger scope, expansion text, validation, and explicit Save. Each invocation can also list exact alternate phrases for repeatable transcription errors. The raised test panel reports whether a typed phrase would fire using the real matcher.
- **Stats** uses responsive elevated cards for dictations, words, speaking time, and average release-to-insert latency. It scrolls at short window heights. The ten-dictation gate, missing-data `n/a`, estimated typing time saved, and independent reset remain intact.

### Settings

| Section | Controls |
|---|---|
| General | Launch at startup, always on top, exit when the window is closed, appearance, Close Program |
| Dictation | Speech provider, key, connection test, model/endpoint, voice, cleanup |
| Audio & shortcut | Shortcut recording/manual entry, microphone picker and input meter |
| On-device | Off/Backup/Primary modes, model download/progress/cancel/delete |
| Meetings | Take notes toggle, optional Windows start/stop shortcut, microphone, detection (off/ask/auto, auto-stop delay, app list), speaker labels (Deepgram key, independent of the dictation key), storage cap and usage, delete all meeting audio |
| Behavior | Cleanup limits, single-word punctuation |
| Privacy | History capture, retention, audio/text/provider disclosures |
| Updates | Version, checking, download/install status, release details |

Section navigation is vertical when space permits and wraps above the content in narrow windows. Each section retains its own scroll position and shares one draft. Save validates, persists TOML, and restarts the pipeline; Discard restores saved fields. Theme changes, key actions, and model downloads remain immediate. Download and test completions are polled from root logic even when their section is hidden. Leaving shortcut settings or hiding the window cancels shortcut capture.

General opens by default after setup. Startup and window preferences take effect on Save. Always on top affects the main window; the recording overlay keeps its own behavior. With **Exit when the window is closed** off (the default), the X hides Hark in the tray. With it on, the X exits. Without a working tray, the X always exits so Hark cannot become inaccessible. **Close Program** and the tray's **Quit** always use the full shutdown path, stopping dictation and flushing pending history writes; unsaved settings are discarded.

### First run

Setup offers cloud or on-device transcription, then configuration, platform-specific permission guidance, and a first dictation. The local choice downloads the real model and uses Verbatim voice to keep setup offline. The cloud choice requires a successful test of the current provider configuration; changing a stored key invalidates that test. Testing does not save the draft. **Save & continue** is explicit.

Permission guidance explains microphone and keyboard/insertion access without claiming permission has been granted. The final step uses the saved shortcut and retires after an actual successful insertion. Users can skip setup, return to configuration, or change their shortcut.
<!-- END:AUTOGEN hark_12_desktop_ui_pages -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_theme -->
## Theming

The refined Nocturne design uses three levels: recessed inputs and chrome, the canvas, and raised cards. Cards combine a restrained shadow, fine border, and upper-edge highlight. Menus, dialogs, and the floating pill use stronger separation. Inter handles prose and headings; JetBrains Mono remains available for technical values. Embedded Phosphor icons use a dedicated font family to prevent Inter's private-use glyphs from replacing them.

| Token | Dark | Light |
|---|---|---|
| Canvas | `#1A1C20` | `#F3F3F6` |
| Chrome / recessed input | `#15171B` | `#FAFAFB` |
| Raised surface | `#23262C` | `#FFFFFF` |
| Main text | `#EDEEF3` | `#23242D` |
| Secondary text | `#A8ADBB` | `#626574` |
| Accent | `#B7A3F7` | `#6847C4` |

System, Light, and Dark appearance preferences are preserved across launches. Success, warning, and danger use separate light/dark colors with text labels or icons. Contrast tests cover secondary and semantic text on the canvas, chrome, and raised surfaces, plus primary action labels. The tray and overlay use fixed colors because they sit over arbitrary desktop content.

Headless egui layout checks cover the minimum-width navigation and card padding. Actual native window composition, microphone capture, global shortcuts, and text insertion require a desktop smoke test on each supported OS.
<!-- END:AUTOGEN hark_12_desktop_ui_theme -->

---
