<!-- PAGE_ID: hark_12_desktop_ui -->
<details>
<summary>Relevant source files</summary>

- [Theme and elevation](../../crates/hark-app/src/theme.rs), [palettes](../../crates/hark-app/src/theme/palette.rs), [categorical hues](../../crates/hark-app/src/theme/hues.rs), and [appearance persistence](../../crates/hark-app/src/theme/appearance.rs)
- [Wizard lettering](../../crates/hark-app/src/theme/lettering.rs), [fonts](../../crates/hark-app/src/theme/fonts.rs), and [font provenance](../../crates/hark-app/assets/README.md)
- [Book view frame](../../crates/hark-app/src/ui/book.rs), [Spellbook book](../../crates/hark-app/src/ui/spellbook/book.rs), and [Invocations book](../../crates/hark-app/src/ui/invocations/book.rs)
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

The meeting detection prompt ("Teams is using your mic. Take meeting notes?") follows the same rule for the same reason: one persistent, deferred viewport, registered from root `logic` so it keeps working while the main window is hidden in the tray, created hidden and only ever shown or hidden — never rebuilt per detection. It is non-modal, never steals focus from the meeting app, offers Start / Not this meeting / Settings, and dismisses itself after 30 seconds with no answer. It sizes its window to what it paints rather than to a fixed height, which clipped the buttons and bottom edge: each pass measures the content (the app's name wraps at a fixed 380 pt width) and a change resizes the window, through Win32 on Windows so it stays anchored above the taskbar. It opens with an accent header naming Hark and counting down to that dismissal, and on Windows 11 DWM gives it rounded corners and an accent outline (Windows 10 keeps square corners and the painted border). **The root shows and places it, not the prompt itself:** eframe 0.36 runs a hidden deferred viewport's UI callback only while egui considers it visible, and that is derived from minimized/occluded state rather than from whether the window is shown, so a prompt that revealed itself from its own callback appeared for one call and never for the next. `App::logic` sends `Visible(true)` to its viewport and moves and sizes the window through Win32 to the bottom-right of the **primary** monitor's work area, in that monitor's physical pixels. It used to follow the foreground window's monitor, and on a tall portrait screen that put it far below the Teams window the user was watching. While showing, its callback re-asserts `HWND_TOPMOST` once a second (z-order only, no activation) on the timeout tick it already has, so a topmost window opened after it, such as Teams' floating call window, cannot bury it. Each step (showing, placed, painted, answered, withdrawn, and "shown but never painted") is logged as a label.
<!-- END:AUTOGEN hark_12_desktop_ui_overlay -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_window -->
## Window Shell

A labeled sidebar groups Home, Insights, History, and Meetings, followed by Spellbook and Invocations, with Settings at the bottom. Below 760 points wide or 500 points of available height after footer bars, it becomes wrapping navigation rows so every destination remains reachable ([navigation.rs:9-10](../../crates/hark-app/src/ui/navigation.rs#L9-L10), [wrapped rows](../../crates/hark-app/src/ui/navigation.rs#L33-L42)). The top bar keeps a theme menu at its right edge in both layouts; it lists the six themes and changes the theme only, while Wizard lettering lives in Settings → General ([navigation.rs:29-31](../../crates/hark-app/src/ui/navigation.rs#L29-L31), [appearance.rs:116-131](../../crates/hark-app/src/theme/appearance.rs#L116-L131)). Content centers at up to 1,020 points and contracts with the window. The running-pipeline landing page is Home; missing setup or startup errors still lead to Settings.

The footer remains visible with the actual pipeline state, configured shortcut, and active transcription/cleanup models. The transcription model is the routed one (`hark_pipeline::stt_route`), so with Live transcribe on it reads `gpt-live-transcribe`, not the batch model still pinned in the draft ([footer.rs `provider_line`](../../crates/hark-app/src/ui/footer.rs)). Long model text truncates with the full value available on hover. A key-related issue opens Dictation settings.

Settings' Save changes / Discard bar stays outside its scroll area. The update banner opens the Updates section directly. Destructive confirmations explain the consequence and initially focus Cancel. Existing settings drafts and editor state survive page navigation; shortcut capture stops when leaving its settings page.

The shell's navigation test runs every theme at both letterings across four small window sizes, requiring each page label to fit unclipped, because Wizard's larger Alegreya text and Cinzel brand must fit the windows Modern does ([shell.rs:204-217](../../crates/hark-app/src/ui/shell.rs#L204-L217)).

Sources: [navigation](../../crates/hark-app/src/ui/navigation.rs), [shell](../../crates/hark-app/src/ui/shell.rs), [page routing](../../crates/hark-app/src/ui/pages.rs), [startup](../../crates/hark-app/src/app.rs).
<!-- END:AUTOGEN hark_12_desktop_ui_window -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_pages -->
## Home, Insights, and Editors

- **Home** shows the actual configured push-to-talk shortcut, pipeline state, today's recorded numeric progress, and recent retained dictations with copy/delete actions. Partial-day coverage is labeled when earlier activity may be missing. Links open full History or Insights. Empty and unavailable-storage states use real status rather than sample content. Visible Home and Insights schedule a refresh at local midnight, including daylight-saving transitions.
- **History** keeps grouped days, search, copy/delete actions, expandable raw transcripts, timing, and Spellbook selection handoff. Wide rows separate timestamps from content. Captions name the actual provider/model and show cleanup only when it ran. Clearing history preserves lifetime counters and retained numeric Insights.
- **Meetings** (Windows, Linux, and macOS 14.2+) is shaped like History: a searchable list of past meetings by title/date/duration, and a detail view with the transcript (speaker chips, timestamps), notes with checkable action items, speaker rename, a Share menu for text, Word, subtitles, audio excerpts, and Windows text sharing, and delete. While a meeting is recording, the page instead shows a live pane: the rolling Me/Them transcript, elapsed time, and a Stop button. A first-run notice reminds the user that some places require every party's consent to record a call; an optional button pastes a canned "I'm using Hark to transcribe this meeting" line into the focused chat.
- **Spellbook** has a raised vocabulary surface, editable terms, aliases, the advanced mishearing control, and undo for the most recent addition. Edits still persist immediately. A **List | Book** switch above the vocabulary opens the same terms as a book ([switch](../../crates/hark-app/src/ui/spellbook/mod.rs#L124-L140)).
- **Invocations** retains trigger scope, expansion text, validation, and explicit Save. Each invocation can also list exact alternate phrases for repeatable transcription errors. The raised test panel reports whether a typed phrase would fire using the real matcher. The same List | Book switch sits beside **New invocation** ([invocations/mod.rs:71-88](../../crates/hark-app/src/ui/invocations/mod.rs#L71-L88)).
- **Insights** has Overview, Your voice, and Performance tabs with 7-, 30-, or 90-day ranges. Overview combines words/dictations, estimated pace and time saved, median latency, daily bars, local-calendar activity/streaks, and optional app usage. Your voice shows vocabulary size, measured Spellbook replacements, invocation usage/output words, busiest local hour, and opt-in retained-history word/phrase analysis. Performance compares median/p95 completion latency and provider/voice usage. The first ten dictations have an introductory progress card; measured panels are available from the first dictation. Lifetime totals are available below the selected range.

Meeting deletion is asynchronous and acknowledged. After confirmation, the detail view shows **Deleting meeting…** and disables conflicting actions while the storage worker removes audio and then the database record. It returns to the list automatically only on a success reply. Audio-removal failures keep the meeting record and show guidance to close any player, check file access, and retry. An unavailable worker or lost reply is displayed as unconfirmed deletion rather than success ([detail view and reply tests](../../crates/hark-app/src/ui/meetings/detail.rs), [storage deletion](../../crates/hark-app/src/storage/meetings.rs)).

### Book view

List is the default for both pages; the choice is remembered on the device, and any unknown saved value reads as List ([book.rs:43-51](../../crates/hark-app/src/ui/book.rs#L43-L51)). The shared frame packs entries onto pages and shows a two-page spread when the column is at least `BOOK_SPREAD_MIN_WIDTH` (two 340-point pages plus a 24-point gutter), otherwise one page ([book.rs:166-173](../../crates/hark-app/src/ui/book.rs#L166-L173), [theme.rs:141-146](../../crates/hark-app/src/theme.rs#L141-L146)). Pages turn with **Previous** / **Next** (disabled, never hidden, at the ends) or with Page Down, Page Up, Home and End while no text field has focus; each page has a running head and a numbered folio ([page_keys](../../crates/hark-app/src/ui/book.rs#L288-L307), [turn_row](../../crates/hark-app/src/ui/book.rs#L366-L396), [page_frame](../../crates/hark-app/src/ui/book.rs#L320-L353)). Position is an anchor entry, so resizing the window never loses the reader's place ([book.rs:1-12](../../crates/hark-app/src/ui/book.rs#L1-L12)).

Each entry's first letter is a large drop cap, shown as a capital without changing the stored text, in a hue that names the page: violet for Spellbook terms and rose for Invocation phrases ([heading](../../crates/hark-app/src/ui/book.rs#L398-L438), [spellbook](../../crates/hark-app/src/ui/spellbook/book.rs#L91-L96), [invocations](../../crates/hark-app/src/ui/invocations/book.rs#L82)). A Spellbook entry then shows "Also heard as: …" with its aliases in monospace, a **New** chip on the term just added (adding from History opens the book on its page), and **Edit** / **Done** and **Delete**. Edit opens the term and aliases in place through the list's own editor, so the two views cannot disagree, and an open entry pins its spread ([spellbook/book.rs:33-56](../../crates/hark-app/src/ui/spellbook/book.rs#L33-L56), [paint](../../crates/hark-app/src/ui/spellbook/book.rs#L85-L142)). An Invocation entry shows a scope chip, "Also: …" alternates, the expansion written out in full with line breaks kept, and a warning icon with the reason when it cannot fire. Editing lays the book flat: the editor spans the whole spread, page turning is disabled, and Save, Cancel and Delete go through the list's own outcome path, so a save still happens only on an explicit Save ([invocations/book.rs:19-49](../../crates/hark-app/src/ui/invocations/book.rs#L19-L49), [paint](../../crates/hark-app/src/ui/invocations/book.rs#L78-L128), [show_flat](../../crates/hark-app/src/ui/book.rs#L121-L141)).

### Interpreting Insights

Duration-based pace and time savings use only rows with measured clip duration, including capture padding; the UI shows measurement coverage. Savings compare those same words against a 40 WPM typing baseline. Older retained transcripts can supply counts and latency, but their missing duration and correction counts stay unknown. A partial-history notice distinguishes backfilled entries from complete tracking. Missing measurements display a dash rather than a fabricated value.

Invocation dictations count the spoken transcript toward dictated words. **Words in invocation output** is a separate total of full inserted output on dictations where an invocation fired; an anywhere-scope invocation includes the surrounding speech. Dictionary corrections count actual replacements across both Spellbook passes, not provider cleanup edits or a claimed accuracy score. Latency describes recorded successful completions, not a success/failure rate.

App names and text analysis each default off in Settings → Privacy. App detection samples only app identity at dictation start, never window/document titles or continuous activity; it is best effort on Windows, macOS, and local X11 and unavailable on Wayland. Turning it off stops future collection; previous labels remain until expiry or Reset stats. Word/phrase analysis reads retained transcripts locally without storing another text copy or contacting a provider. Reset stats clears numeric details and lifetime counters while retaining transcripts, so opted-in word analysis can still use that history.

Queries and text aggregation run on the storage worker. The UI caches results by data generation, range, local date, and text-analysis choice; an obsolete reply cannot overwrite a newly selected range. See [Data Storage](../core/DATA_STORAGE.md#lifetime-stats-and-detailed-insights) for retention and coverage semantics.

### Settings

| Section | Controls |
|---|---|
| General | Launch at startup, always on top, exit when the window is closed, Appearance (Theme menu and the **Wizard lettering** checkbox, both applied immediately and remembered on this device), Close Program ([preferences.rs:7-75](../../crates/hark-app/src/ui/settings/preferences.rs#L7-L75)) |
| Dictation | Speech to text card: provider with a one-line description, key with a Get a key link until one is stored, a no-key on-device suggestion, connection test, Model and endpoint (optional); then the on-device toggle (Backup or Primary) with model download/progress/cancel/delete. Primary hides the cloud controls. Voice, cleanup, and a privacy line that follows the mode |
| Audio & shortcut | Shortcut recording/manual entry, microphone picker and input meter |
| Meetings | Take notes toggle, optional Windows start/stop shortcut, microphone, detection (off/ask/auto, auto-stop delay, app list), speaker labels (Deepgram key, independent of the dictation key), storage cap and usage, delete all meeting audio |
| Behavior | Cleanup limits, single-word punctuation |
| Privacy | History capture/retention, optional local app tracking, optional retained-history word/phrase analysis, numeric retention and provider disclosures |
| Updates | Version, checking, download/install status, release details |

The Dictation provider picker offers Deepgram, OpenAI, Gemini and OpenAI-compatible, always in that order; Groq is gone (schema 7 migrates saved Groq configs). The selected provider's option row sits under the radios ([live_rows.rs](../../crates/hark-app/src/ui/settings/live_rows.rs)). Gemini shows its Verbatim/Smart transcript mode. OpenAI shows a **Live transcribe** checkbox with the cost note "Streams while you hold the key. Billed per audio minute, about 4× gpt-transcribe." It uses the same OpenAI key, so nothing is entered twice. While it is ticked the Model field becomes a read-only `gpt-live-transcribe (Live transcribe)` label, and a batch model typed earlier is kept for when it is unticked. Switching provider turns the toggle off. Toggling it counts as a change, so an earlier Test connection pass no longer stands. A `--no-default-features` build, which lacks the `live-stt` WebSocket client, hides both Gemini and the toggle instead of offering controls that cannot work ([form.rs `provider_choices`, `provider_section`](../../crates/hark-app/src/ui/settings/form.rs)). The Groq cleanup row and Groq's 10-second-minimum note under Test connection are gone too ([cleanup.rs](../../crates/hark-app/src/ui/settings/cleanup.rs), [test.rs](../../crates/hark-app/src/ui/settings/test.rs)).

Meetings also exposes **Reduce speaker echo**, off by default. Save applies it
from the next meeting. It reduces playback picked up by the meeting microphone;
the helper text recommends leaving it off with headphones and turning it off if
the local voice sounds worse. It does not change dictation or select a new
provider ([settings](../../crates/hark-app/src/ui/settings/meetings.rs),
[capture behavior](MEETINGS.md#reduce-speaker-echo)).

Section navigation is vertical when space permits and wraps above the content in narrow windows. Each section retains its own scroll position and shares one draft. Save validates, persists TOML, and restarts the pipeline; Discard restores saved fields. Theme and lettering changes, key actions, and model downloads remain immediate. Download and test completions are polled from root logic even when their section is hidden. Leaving shortcut settings or hiding the window cancels shortcut capture.

Within Settings, General is the initial section after setup. Startup and window preferences take effect on Save. Always on top affects the main window; the recording overlay keeps its own behavior. With **Exit when the window is closed** off (the default), the X hides Hark in the tray. With it on, the X exits. Without a working tray, the X always exits so Hark cannot become inaccessible. **Close Program** and the tray's **Quit** always use the full shutdown path, stopping dictation and flushing pending history writes; unsaved settings are discarded.

### First run

Setup offers cloud or on-device transcription, with a tip that suggests on-device to anyone without a paid key, then configuration, platform-specific permission guidance, and a first dictation. The local choice downloads the real model and uses Verbatim voice to keep setup offline. Readiness follows the draft's mode, not the first choice: on-device Primary (chosen up front, or by the Speech to text toggle or its no-key suggestion during configuration) waits for the model download, while cloud and Backup require a successful test of the current provider configuration; changing a stored key invalidates that test ([onboarding.rs](../../crates/hark-app/src/ui/settings/onboarding.rs), [get_started.rs](../../crates/hark-app/src/ui/settings/get_started.rs)). Testing does not save the draft. **Save & continue** is explicit.

Permission guidance explains microphone and keyboard/insertion access without claiming permission has been granted. The final step uses the saved shortcut and retires after an actual successful insertion. Users can skip setup, return to configuration, or change their shortcut.
<!-- END:AUTOGEN hark_12_desktop_ui_pages -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_theme -->
## Theming

The visual system combines warm neutral surfaces, restrained forest/sage accents, rounded cards, and a labeled sidebar. There are six themes: System, Light, Dark, Solarized Light, Solarized Dark, and Parchment ([Appearance](../../crates/hark-app/src/theme/appearance.rs#L7-L47)). Typography is a separate choice, described under Lettering below. Phosphor icons keep a dedicated font family so Inter's private-use glyphs cannot replace them. Fonts are bundled locally with their licenses; nothing is fetched at runtime.

| Token | Light | Dark | Solarized Light | Solarized Dark | Parchment |
|---|---|---|---|---|---|
| Canvas | `#F7F7F3` | `#171C19` | `#FDF6E3` | `#002B36` | `#F4EAD5` |
| Sidebar | `#EEEFE9` | `#131814` | `#EEE8D5` | `#073642` | `#EADCBF` |
| Card | `#FFFFFF` | `#202722` | `#FDF6E3` | `#073642` | `#FBF6EA` |
| Main text | `#252E29` | `#E9EEE4` | `#526A71` | `#EEE8D5` | `#3D3024` |
| Secondary text | `#616B63` | `#A4B1A4` | `#526A71` | `#93A1A1` | `#65533D` |
| Action accent | `#376C58` | `#B5CCA1` | `#526A71` | `#93A1A1` | `#2E4C84` |
| Chart accent | `#527E69` | `#A8C99A` | `#2AA198` | `#2AA198` | `#2E4C84` |

Values are the `Palette` constants ([palette.rs:36-165](../../crates/hark-app/src/theme/palette.rs#L36-L165)). The two Solarized palettes use [Ethan Schoonover's published base fills and cyan](https://ethanschoonover.com/solarized/), with derived supporting borders. Solarized Light body/action text blends base01 8% toward base02 because the original base01/base2 pair falls below 4.5:1 on the sidebar and hero. Light secondary text is similarly adjusted for those tinted surfaces.

**Parchment** is aged paper with dark-brown ink and a lapis-blue accent, and it is light only: it stores the Light preference and fills both the light and dark palette slots, so it stays light even when the OS is dark. Its window color must differ from Solarized Light's, because `palette()` identifies a palette by its canvas color. Cards and book pages are a lighter vellum than the window. The accent is lapis rather than red so a focus ring or link never reads as an error; red belongs to the hue set and the drop caps. Its secondary text on the sidebar, 5.42:1, is the lowest contrast pair in the palette ([parchment palette](../../crates/hark-app/src/theme/palette.rs#L138-L165), [appearance pairing](../../crates/hark-app/src/theme/appearance.rs#L49-L70)).

Each palette also owns its status colors (danger, success, warning). Light has its own; Dark and Solarized Dark use the shared dark-surface constants; Solarized Light has always drawn status in the Light theme's colors; and Parchment tunes its own to its paper (`#A1301C`, `#2F6B3A`, `#82560C`) ([palette.rs:12-34](../../crates/hark-app/src/theme/palette.rs#L12-L34), [Dark](../../crates/hark-app/src/theme/palette.rs#L74-L76), [Solarized Light](../../crates/hark-app/src/theme/palette.rs#L108-L111), [theme.rs:153-164](../../crates/hark-app/src/theme.rs#L153-L164)).

System, Light, Dark, Solarized Light, Solarized Dark, and Parchment apply immediately and persist independently of the TOML settings draft; see [Configuration](../core/CONFIGURATION.md) for the stored keys. A stable egui-memory palette key preserves explicit choices; absent or unknown palette keys respect the previously saved Light/Dark/System preference. Returning to System restores both neutral palettes before following OS changes.

### Categorical hues and speaker colors

Every palette carries six categorical hues in a fixed order: Blue, Teal, Green, Gold, Rose, Violet. Each is an ink (text and icons) paired with a wash (chip and badge fill). Tests hold every palette to an ink at least 4.5:1 on every fill the palette draws, ink on its own wash at least 4.5:1, and the six inks at least 0.06 apart in OKLab, so two categories cannot silently merge ([hues.rs:1-5](../../crates/hark-app/src/theme/hues.rs#L1-L5), [theme.rs:182-221](../../crates/hark-app/src/theme.rs#L182-L221), [contrast tests](../../crates/hark-app/src/theme/tests.rs#L64-L155)). Hues name categories such as a page, a provider or a speaker, never a state: a hued element never carries a check, warning or X glyph, and it always has a text label next to it. The `chip` and `chip_button` widgets are the only way a hue reaches a pill, so ink and wash stay a tested pair ([chip](../../crates/hark-app/src/theme.rs#L293-L325), [chip_button](../../crates/hark-app/src/theme.rs#L251-L279)).

Meeting speakers map onto the hues through `speaker_hue`: slot 0, "Me", is always Blue and no other slot ever is; slots 1 to 5 take Gold, Violet, Teal, Rose, then Green (last, because it sits near the success color), and past six speakers those five repeat while the name beside every mark does not. A transcript line starts with a thin bar in the speaker's hue, and the Word export colors names with the Light palette's inks because a document is white whatever the theme ([theme.rs:223-249](../../crates/hark-app/src/theme.rs#L223-L249), [speaker tests](../../crates/hark-app/src/theme/tests.rs#L157-L202)).

### Lettering and fonts

**Wizard lettering** is a typography switch independent of the theme. Modern keeps Inter as the interface font, with embedded Lora Regular for the Home headline and the brand. Wizard sets text in Alegreya, a calligraphic book face, and sets page titles, dialog titles and the Home headline in Cinzel Decorative; book drop caps use Cinzel Decorative Bold. Key names, timings and literal aliases stay in JetBrains Mono and icons are unchanged in both ([font families](../../crates/hark-app/src/theme/fonts.rs#L8-L16), [family lists](../../crates/hark-app/src/theme/fonts.rs#L91-L129), [Lettering](../../crates/hark-app/src/theme/lettering.rs#L8-L33)). Body text is one point larger under Wizard (15 against 14, with the subheading, button and small sizes also one step up) because Alegreya reads smaller than Inter at the same size ([text_styles](../../crates/hark-app/src/theme.rs#L581-L603)).

Until the user chooses, lettering follows the theme: Wizard on Parchment, Modern on every other theme. After the user ticks or unticks the Settings → General → Appearance checkbox, that choice holds on every theme and is remembered on this device. The derived default is never written at startup, so "follows the theme" cannot turn into a fixed choice, and fonts are reinstalled only when the effective lettering actually changes, since `set_fonts` rebuilds the font atlas ([lettering.rs:35-77](../../crates/hark-app/src/theme/lettering.rs#L35-L77), [set_appearance](../../crates/hark-app/src/theme/appearance.rs#L95-L107)).

Alegreya and Cinzel Decorative are vendored under the SIL Open Font License from the google/fonts repository at a pinned commit, with their source, download date and checksums recorded beside the files. They ship unmodified: Cinzel declares the Reserved Font Name "Cinzel", so a modified font could not keep the name. Cinzel Decorative does not map the no-break space or the micro sign, so the Wizard serif families fall back to Alegreya for those two ([assets README](../../crates/hark-app/assets/README.md#L12-L21), [CINZEL_GAPS test](../../crates/hark-app/src/theme/tests.rs#L577)). Alegreya is one variable font file registered three times, at weights 400, 500 and 600, each pinned with a `wght` variation coordinate; a spike test requires the three widths to rise, and the approved fallback if egui stops honoring the pin is Spectral's static files ([fonts.rs:51-63](../../crates/hark-app/src/theme/fonts.rs#L51-L63), [README weights](../../crates/hark-app/assets/README.md#L23-L34)).

Tests cover serialized appearance restoration, lettering derivation and restart persistence, legacy preferences, text and semantic colors across canvas/cards/sidebar/hero, primary labels, and visible focus rings. The tray and overlay keep fixed, readable dark-background colors because they sit over arbitrary desktop content. Actual native window composition, microphone capture, global shortcuts, and text insertion still require target-platform smoke tests.

Sources: [tokens](../../crates/hark-app/src/theme.rs), [palettes](../../crates/hark-app/src/theme/palette.rs), [persistence](../../crates/hark-app/src/theme/appearance.rs), [fonts and licenses](../../crates/hark-app/assets/README.md), [theme tests](../../crates/hark-app/src/theme/tests.rs).
<!-- END:AUTOGEN hark_12_desktop_ui_theme -->

---

## macOS integration

Setup and Settings → General show current Microphone, Input Monitoring and Accessibility status, with explicit permission requests and links to System Settings. Retry dictation after granting access; macOS may require restarting Hark. Meeting settings explain separate system-audio permission and browser-title access. The native recording pill uses the visible frame of the screen under the pointer; the meeting prompt uses the primary screen. Both respect the Dock and menu bar, remain above ordinary windows, and can appear across Spaces. Exports use an asynchronous native save sheet, Word export, Finder reveal and the macOS share sheet ([bridge](../../crates/hark-app/src/macos.rs), [AppKit implementation](../../crates/hark-app/src/macos/native.m)).
