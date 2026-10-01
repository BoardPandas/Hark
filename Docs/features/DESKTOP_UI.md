<!-- PAGE_ID: hark_12_desktop_ui -->
<details>
<summary>Relevant source files</summary>

- [Theme and elevation](../../crates/hark-app/src/theme.rs), [palettes](../../crates/hark-app/src/theme/palette.rs), and [appearance persistence](../../crates/hark-app/src/theme/appearance.rs)
- [Navigation](../../crates/hark-app/src/ui/navigation.rs), [Home](../../crates/hark-app/src/ui/home.rs), and [Insights](../../crates/hark-app/src/ui/stats.rs)
- [Insights worker cache](../../crates/hark-app/src/ui/insights_cache.rs) and [local aggregates](../../crates/hark-store/src/insights/mod.rs)
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

Meeting settings include an explicit Gemini after-call choice, its key field, and
an independent model setting. Completed refined meetings use the provider-neutral
“refined transcript” label; the saved-recording re-run confirmation still names
Deepgram ([meeting settings](../../crates/hark-app/src/ui/settings/meetings.rs),
[detail](../../crates/hark-app/src/ui/meetings/detail.rs)).

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md), [Data Storage](../core/DATA_STORAGE.md), [Updates and Autostart](UPDATES_AND_AUTOSTART.md), [Meetings](MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_overview -->
## Overview

Hark uses native `eframe`/`egui` for its window and floating dictation feedback, and `tray-icon` for its system menu. There is no webview. The main window can remain hidden while push-to-talk runs from the tray.

The main thread owns egui and the Windows/macOS tray. On Linux, libappindicator owns GTK widgets on a dedicated thread with its own loop. Audio, hotkeys, transcription, cleanup, and insertion run on worker threads. The UI consumes state; it never delays insertion to render feedback.

On Windows, Linux, and macOS 14.2+, the UI also exposes meeting transcription: a Meetings page, a tray entry to start or stop taking notes, and a non-modal prompt when a meeting app starts using the microphone. Meetings is hidden wherever its capture is unsupported; nothing about it changes how push-to-talk dictation looks or behaves.
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

A recording meeting is shown on an otherwise-idle tray as the same red recording disc, with a tooltip naming the time notes started ("Hark: taking meeting notes since 14:30"); once a detected meeting's app hangs up, the tooltip switches to when the notes will stop ("Hark: the call ended; meeting notes stop at 14:52:07"), matching the Meetings page's notice and its **Stop now** button. A start or stop *time* is shown rather than a countdown so a hidden, idle window never has to wake each second to update it — the recording state has to stay visible for as long as a meeting runs, but a dictation's own recording, processing, or error state still takes priority while it lasts. Where meeting capture does not exist on the platform, the menu builds without the entry at all rather than shipping one that can never work; where it exists but is off or failed to start, the entry stays in the menu, disabled.
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

On Windows, the indicator also reasserts `HWND_TOPMOST` when it appears and once a second while any feedback is visible. This restores its position above other windows without moving, resizing, or activating it. Repeating egui's window-level command is insufficient because winit caches the unchanged always-on-top flag. The existing paint ticks drive the check, and hiding clears its timer so the next dictation raises immediately ([painting and z-order](../../crates/hark-app/src/overlay/paint.rs)). Native stacking and insertion focus still require a Windows smoke test.

The meeting detection prompt ("Teams is using your mic. Take meeting notes?") follows the same rule for the same reason: one persistent, deferred viewport, registered from root `logic` so it keeps working while the main window is hidden in the tray, created hidden and only ever shown or hidden — never rebuilt per detection. It is non-modal, never steals focus from the meeting app, offers Start / Not this meeting / Settings, and dismisses itself after 30 seconds with no answer. **The root shows and places it, not the prompt itself:** eframe 0.36 runs a hidden deferred viewport's UI callback only while egui considers it visible, and that is derived from minimized/occluded state rather than from whether the window is shown, so a prompt that revealed itself from its own callback appeared for one call and never for the next. `App::logic` sends `Visible(true)` to its viewport and moves and sizes the window through Win32 to the bottom-right of the **primary** monitor's work area, in that monitor's physical pixels. It used to follow the foreground window's monitor, and on a tall portrait screen that put it far below the Teams window the user was watching. While showing, its callback re-asserts `HWND_TOPMOST` once a second (z-order only, no activation) on the timeout tick it already has, so a topmost window opened after it, such as Teams' floating call window, cannot bury it. Each step (showing, placed, painted, answered, withdrawn, and "shown but never painted") is logged as a label.
<!-- END:AUTOGEN hark_12_desktop_ui_overlay -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_window -->
## Window Shell

A labeled sidebar groups Home, Insights, History, and Meetings, followed by Spellbook and Invocations, with Settings at the bottom. Below 760 points wide or 500 points of available height after footer bars, it becomes wrapping navigation rows so every destination remains reachable. The appearance picker remains available at the top. Content centers at up to 1,020 points and contracts with the window. The running-pipeline landing page is Home; missing setup or startup errors still lead to Settings.

The footer remains visible with the actual pipeline state, configured shortcut, and active transcription/cleanup models. Long model text truncates with the full value available on hover. A key-related issue opens Dictation settings.

Settings' Save changes / Discard bar stays outside its scroll area. The update banner opens the Updates section directly. Destructive confirmations explain the consequence and initially focus Cancel. Existing settings drafts and editor state survive page navigation; shortcut capture stops when leaving its settings page.

Sources: [navigation](../../crates/hark-app/src/ui/navigation.rs), [shell](../../crates/hark-app/src/ui/shell.rs), [page routing](../../crates/hark-app/src/ui/pages.rs), [startup](../../crates/hark-app/src/app.rs).
<!-- END:AUTOGEN hark_12_desktop_ui_window -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_pages -->
## Home, Insights, and Editors

- **Home** shows the actual configured push-to-talk shortcut, pipeline state, today's recorded numeric progress, and recent retained dictations with copy/delete actions. Partial-day coverage is labeled when earlier activity may be missing. Links open full History or Insights. Empty and unavailable-storage states use real status rather than sample content. Visible Home and Insights schedule a refresh at local midnight, including daylight-saving transitions.
- **History** keeps grouped days, search, copy/delete actions, expandable raw transcripts, timing, and Spellbook selection handoff. Wide rows separate timestamps from content. Captions name the actual provider/model and show cleanup only when it ran. Clearing history preserves lifetime counters and retained numeric Insights.
- **Meetings** (Windows, Linux, and macOS 14.2+) is shaped like History: a searchable list of past meetings by title/date/duration, and a detail view with the transcript (speaker chips, timestamps), notes with checkable action items, speaker rename, a Share menu for text, Word, subtitles, audio excerpts, and Windows text sharing, and delete. While a meeting is recording, the page instead shows a live pane: the rolling Me/Them transcript, elapsed time, and a Stop button. A first-run notice reminds the user that some places require every party's consent to record a call; an optional button pastes a canned "I'm using Hark to transcribe this meeting" line into the focused chat.
- **Spellbook** has a raised vocabulary surface, editable terms, aliases, the advanced mishearing control, and undo for the most recent addition. Edits still persist immediately.
- **Invocations** retains trigger scope, expansion text, validation, and explicit Save. Each invocation can also list exact alternate phrases for repeatable transcription errors. The raised test panel reports whether a typed phrase would fire using the real matcher.
- **Insights** has Overview, Your voice, and Performance tabs with 7-, 30-, or 90-day ranges. Overview combines words/dictations, estimated pace and time saved, median latency, daily bars, local-calendar activity/streaks, and optional app usage. Your voice shows vocabulary size, measured Spellbook replacements, invocation usage/output words, busiest local hour, and opt-in retained-history word/phrase analysis. Performance compares median/p95 completion latency and provider/voice usage. The first ten dictations have an introductory progress card; measured panels are available from the first dictation. Lifetime totals are available below the selected range.

Meeting deletion is asynchronous and acknowledged. After confirmation, the detail view shows **Deleting meeting…** and disables conflicting actions while the storage worker removes audio and then the database record. It returns to the list automatically only on a success reply. Audio-removal failures keep the meeting record and show guidance to close any player, check file access, and retry. An unavailable worker or lost reply is displayed as unconfirmed deletion rather than success ([detail view and reply tests](../../crates/hark-app/src/ui/meetings/detail.rs), [storage deletion](../../crates/hark-app/src/storage/meetings.rs)).

### Interpreting Insights

Duration-based pace and time savings use only rows with measured clip duration, including capture padding; the UI shows measurement coverage. Savings compare those same words against a 40 WPM typing baseline. Older retained transcripts can supply counts and latency, but their missing duration and correction counts stay unknown. A partial-history notice distinguishes backfilled entries from complete tracking. Missing measurements display a dash rather than a fabricated value.

Invocation dictations count the spoken transcript toward dictated words. **Words in invocation output** is a separate total of full inserted output on dictations where an invocation fired; an anywhere-scope invocation includes the surrounding speech. Dictionary corrections count actual replacements across both Spellbook passes, not provider cleanup edits or a claimed accuracy score. Latency describes recorded successful completions, not a success/failure rate.

App names and text analysis each default off in Settings → Privacy. App detection samples only app identity at dictation start, never window/document titles or continuous activity; it is best effort on Windows, macOS, and local X11 and unavailable on Wayland. Turning it off stops future collection; previous labels remain until expiry or Reset stats. Word/phrase analysis reads retained transcripts locally without storing another text copy or contacting a provider. Reset stats clears numeric details and lifetime counters while retaining transcripts, so opted-in word analysis can still use that history.

Queries and text aggregation run on the storage worker. The UI caches results by data generation, range, local date, and text-analysis choice; an obsolete reply cannot overwrite a newly selected range. See [Data Storage](../core/DATA_STORAGE.md#lifetime-stats-and-detailed-insights) for retention and coverage semantics.

### Settings

| Section | Controls |
|---|---|
| General | Launch at startup, always on top, exit when the window is closed, appearance, Close Program |
| Dictation | Speech provider, key, connection test, model/endpoint, voice, cleanup |
| Audio & shortcut | Shortcut recording/manual entry, microphone picker and input meter |
| On-device | Off/Backup/Primary modes, model download/progress/cancel/delete |
| Meetings | Take notes toggle, optional Windows start/stop shortcut, microphone, detection (off/ask/auto, auto-stop delay, app list), speaker labels (Deepgram key, independent of the dictation key), storage cap and usage, delete all meeting audio |
| Behavior | Cleanup limits, single-word punctuation |
| Privacy | History capture/retention, optional local app tracking, optional retained-history word/phrase analysis, numeric retention and provider disclosures |
| Updates | Version, checking, download/install status, release details |

Meetings also exposes **Reduce speaker echo**, off by default. Save applies it
from the next meeting. It reduces playback picked up by the meeting microphone;
the helper text recommends leaving it off with headphones and turning it off if
the local voice sounds worse. It does not change dictation or select a new
provider ([settings](../../crates/hark-app/src/ui/settings/meetings.rs),
[capture behavior](MEETINGS.md#reduce-speaker-echo)).

Section navigation is vertical when space permits and wraps above the content in narrow windows. Each section retains its own scroll position and shares one draft. Save validates, persists TOML, and restarts the pipeline; Discard restores saved fields. Theme changes, key actions, and model downloads remain immediate. Download and test completions are polled from root logic even when their section is hidden. Leaving shortcut settings or hiding the window cancels shortcut capture.

Within Settings, General is the initial section after setup. Startup and window preferences take effect on Save. Always on top affects the main window; the recording overlay keeps its own behavior. With **Exit when the window is closed** off (the default), the X hides Hark in the tray. With it on, the X exits. Without a working tray, the X always exits so Hark cannot become inaccessible. **Close Program** and the tray's **Quit** always use the full shutdown path, stopping dictation and flushing pending history writes; unsaved settings are discarded.

### First run

Setup offers cloud or on-device transcription, then configuration, platform-specific permission guidance, and a first dictation. The local choice downloads the real model and uses Verbatim voice to keep setup offline. The cloud choice requires a successful test of the current provider configuration; changing a stored key invalidates that test. Testing does not save the draft. **Save & continue** is explicit.

Permission guidance explains microphone and keyboard/insertion access without claiming permission has been granted. The final step uses the saved shortcut and retires after an actual successful insertion. Users can skip setup, return to configuration, or change their shortcut.
<!-- END:AUTOGEN hark_12_desktop_ui_pages -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_theme -->
## Theming

The visual system combines warm neutral surfaces, restrained forest/sage accents, rounded cards, and a labeled sidebar. Inter remains the interface font; embedded Lora Regular provides editorial Home headings and the brand. JetBrains Mono handles technical values. Phosphor icons keep a dedicated font family so Inter's private-use glyphs cannot replace them. Fonts are bundled locally with their licenses; nothing is fetched at runtime.

| Token | Light | Dark | Solarized Light | Solarized Dark |
|---|---|---|---|---|
| Canvas | `#F7F7F3` | `#171C19` | `#FDF6E3` | `#002B36` |
| Sidebar | `#EEEFE9` | `#131814` | `#EEE8D5` | `#073642` |
| Card | `#FFFFFF` | `#202722` | `#FDF6E3` | `#073642` |
| Main text | `#252E29` | `#E9EEE4` | `#526A71` | `#EEE8D5` |
| Secondary text | `#616B63` | `#A4B1A4` | `#526A71` | `#93A1A1` |
| Action accent | `#376C58` | `#B5CCA1` | `#526A71` | `#93A1A1` |
| Chart accent | `#527E69` | `#A8C99A` | `#2AA198` | `#2AA198` |

The two Solarized palettes use [Ethan Schoonover's published base fills and cyan](https://ethanschoonover.com/solarized/), with derived supporting borders. Solarized Light body/action text blends base01 8% toward base02 because the original base01/base2 pair falls below 4.5:1 on the sidebar and hero. Light secondary text is similarly adjusted for those tinted surfaces.

System, Light, Dark, Solarized Light, and Solarized Dark apply immediately and persist independently of the TOML settings draft. A stable egui-memory palette key preserves explicit choices; absent or unknown palette keys respect the previously saved Light/Dark/System preference. Returning to System restores both neutral palettes before following OS changes.

Tests cover serialized appearance restoration, legacy preferences, text and semantic colors across canvas/cards/sidebar/hero, primary labels, and visible focus rings. The tray and overlay keep fixed, readable dark-background colors because they sit over arbitrary desktop content. Actual native window composition, microphone capture, global shortcuts, and text insertion still require target-platform smoke tests.

Sources: [tokens](../../crates/hark-app/src/theme.rs), [palettes](../../crates/hark-app/src/theme/palette.rs), [persistence](../../crates/hark-app/src/theme/appearance.rs), [fonts and licenses](../../crates/hark-app/assets/README.md), [theme tests](../../crates/hark-app/src/theme/tests.rs).
<!-- END:AUTOGEN hark_12_desktop_ui_theme -->

---

## macOS integration

Setup and Settings → General show current Microphone, Input Monitoring and Accessibility status, with explicit permission requests and links to System Settings. Retry dictation after granting access; macOS may require restarting Hark. Meeting settings explain separate system-audio permission and browser-title access. The native recording pill uses the visible frame of the screen under the pointer; the meeting prompt uses the primary screen. Both respect the Dock and menu bar, remain above ordinary windows, and can appear across Spaces. Exports use an asynchronous native save sheet, Word export, Finder reveal and the macOS share sheet ([bridge](../../crates/hark-app/src/macos.rs), [AppKit implementation](../../crates/hark-app/src/macos/native.m)).
