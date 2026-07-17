<!-- PAGE_ID: hark_12_desktop_ui -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [crates/hark-app/src/tray/mod.rs:1-236](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L1-L236)
- [crates/hark-app/src/tray/icon.rs:1-247](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L1-L247)
- [crates/hark-app/src/overlay.rs:1-120](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L1-L120)
- [crates/hark-app/src/theme.rs:1-484](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L1-L484)
- [crates/hark-app/src/ui/mod.rs:1-15](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/mod.rs#L1-L15)
- [crates/hark-app/src/ui/shell.rs:1-218](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L1-L218)
- [crates/hark-app/src/ui/pages.rs:1-131](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L1-L131)
- [crates/hark-app/src/ui/history/mod.rs:1-309](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L1-L309)
- [crates/hark-app/src/ui/stats.rs:1-238](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L1-L238)
- [crates/hark-app/src/ui/settings/mod.rs:1-178](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L1-L178)

</details>

# Desktop UI

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md), [Data Storage](../core/DATA_STORAGE.md), [Updates and Autostart](UPDATES_AND_AUTOSTART.md)

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_overview -->
## Overview

Hark's desktop surface is a native `tray-icon` + `eframe`/`egui` UI with no web view or browser runtime involved. The tray is the always-present daemon: a menu, a state icon, and a tooltip built directly from `PipelineStatus`, created lazily on the first main-thread callback so it satisfies the macOS requirement that all UI construction happens on the main thread ([tray/mod.rs:1-14](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L1-L14)).

The settings/history/stats window is a second, optional surface: a sidebar shell that routes between four egui pages (History, Dictionary, Stats, Settings) plus a status footer and an update banner, and it can stay closed entirely while push-to-talk dictation keeps working from the tray ([ui/mod.rs:1-14](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/mod.rs#L1-L14), [ui/shell.rs:1-2](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L1-L2)). A third, transient surface is the recording overlay: a borderless always-on-top pill shown only while the push-to-talk chord is held ([overlay.rs:1-18](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L1-L18)). All three surfaces are painted from the same main-thread egui context; the dictation pipeline (hotkey, audio, HTTP, injection) never touches egui directly and communicates only through channels and status snapshots.

Sources: [tray/mod.rs:1-14](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L1-L14), [ui/mod.rs:1-14](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/mod.rs#L1-L14), [overlay.rs:1-18](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L1-L18)
<!-- END:AUTOGEN hark_12_desktop_ui_overview -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_tray -->
## Tray Daemon

The tray owns the "hot path" surface: it is built once per process and reconciled against the pipeline every frame, without ever requiring the window to be open ([tray/mod.rs:39-47](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L39-L47)). Its icon, tooltip, and menu are a direct, testable projection of `PipelineStatus`.

### Icon states

`icon::state` collapses `PipelineStatus` into six tray-drawable states, and `icon::rgba` paints each one as a bold, programmatically-drawn disc or ring so it stays legible at taskbar size ([icon.rs:21-36](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L21-L36), [icon.rs:38-52](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L38-L52)):

| Tray state | Meaning | Visual | Mapped from |
|---|---|---|---|
| `Idle` | Listening for the chord | Accent ring (hollow center) | `PipelineStatus::Idle` ([icon.rs:38-41](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L38-L41)) |
| `Recording` | Capturing audio | Red (danger) filled disc | `PipelineStatus::Recording` |
| `Processing` | Request in flight | Accent filled disc | `PipelineStatus::Processing` |
| `NeedsKey` | Key missing or rejected | Amber disc + "!" | `Errored` / `Stopped` with `key_related: true` ([icon.rs:43-48](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L43-L48)) |
| `Error` | Last dictation failed | Red disc + "!" | `Errored` with `key_related: false` |
| `Stopped` | Pipeline not running, non-key reason | Gray disc + "!" | `Stopped` with `key_related: false` |

Color never carries a state alone: every failure state also draws a white exclamation mark, and the tooltip always spells the state out in words ([icon.rs:1-6](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L1-L6), [icon.rs:55-69](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L55-L69)). Tooltips are capped at 127 characters (Windows truncates longer strings mid-word) and get an ellipsis instead ([icon.rs:14-16](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L14-L16), [icon.rs:71-78](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L71-L78)).

### Menu and actions

`Tray::create` builds the native menu once: a `CheckMenuItem` radio group for every configured voice, a separator, "Open Settings", another separator, and "Quit Hark" ([tray/mod.rs:60-85](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L60-L85)). Interactions surface as a small enum the rest of the app can match on:

```rust
pub enum TrayAction {
    SelectVoice(VoiceName),
    OpenSettings,
    /// Double-click on the icon: bring the window back, current page.
    ShowWindow,
    Quit,
}
```

Sources: [tray/mod.rs:30-37](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L30-L37)

Two pump threads translate the global `muda`/`tray-icon` event channels (`MenuEvent::receiver()`, `TrayIconEvent::receiver()`) into `TrayAction`s on an `mpsc` channel and wake the egui event loop via `request_repaint`, because a hidden window paints no frames to drain those channels on its own ([tray/mod.rs:8-14](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L8-L14), [tray/mod.rs:157-190](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L157-L190)). A double-click on the icon is the only `TrayIconEvent` handled, and it maps straight to `TrayAction::ShowWindow` to restore the window ([tray/mod.rs:175-188](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L175-L188)). `App::logic` drains the queue every frame with `take_actions` ([tray/mod.rs:107-113](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L107-L113)).

`Tray::apply` reconciles icon, tooltip, and the voice checkmarks against current state each frame, but only calls into the OS when something actually changed, and logs (rather than panics on) any OS-level set failure since the in-window footer remains the authoritative status surface ([tray/mod.rs:115-137](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L115-L137)). `set_voice` is unconditional over every check item because a native `CheckMenuItem` toggles itself on click, so even clicking the already-selected voice needs its checkmark restored ([tray/mod.rs:139-147](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L139-L147)).

Sources: [tray/mod.rs:1-236](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/mod.rs#L1-L236), [tray/icon.rs:1-247](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/tray/icon.rs#L1-L247)
<!-- END:AUTOGEN hark_12_desktop_ui_tray -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_overlay -->
## Recording Overlay

The recording overlay is the "floating recording pill": a small always-on-top capsule with a purple circle that pulses with mic input, shown near the bottom of the screen for as long as the push-to-talk chord is held ([overlay.rs:1-4](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L1-L4)). It is implemented as an egui **deferred viewport**, a second borderless OS window driven by the same main-thread event loop, which keeps it inside the one hard threading rule: no UI off the main thread ([overlay.rs:6-9](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L6-L9)). `show` is registered from the app's `logic` callback (which runs even while the main window is hidden in the tray), so the overlay works during ordinary daemon operation and tears down as soon as the caller stops registering it on chord release ([overlay.rs:9-13](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L9-L13)).

The window is deliberately powerless to interfere with whatever app is focused: it never activates, is fully mouse-transparent, and is hidden from the taskbar, because Hark injects text into the previously focused window and the overlay must never steal that focus ([overlay.rs:14-18](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L14-L18)):

```rust
let mut builder = egui::ViewportBuilder::default()
    .with_title("Hark recording")
    .with_inner_size(WINDOW)
    .with_decorations(false)
    .with_transparent(true)
    .with_resizable(false)
    .with_always_on_top()
    .with_taskbar(false)
    // Never take focus: injection targets the previously focused app.
    .with_active(false)
    // Clicks fall through to whatever is underneath.
    .with_mouse_passthrough(true);
```

Sources: [overlay.rs:45-56](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L45-L56)

When a primary-monitor size is available the pill is placed bottom-centered, a fixed fraction of the screen height above the bottom edge; otherwise the OS chooses the position ([overlay.rs:33-34](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L33-L34), [overlay.rs:58-62](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L58-L62)). Each frame, `paint` requests another repaint in ~16 ms (about 60 fps) so the pulse keeps animating while the parent window sleeps ([overlay.rs:72-74](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L72-L74)). The displayed amplitude is a square-root curve over the raw peak level (lifting normal speech into a visible range without pinning loud peaks), eased across frames with `animate_value_with_time`, and blended with a faint idle "breathing" sine so the dot stays visibly alive during silence ([overlay.rs:78-87](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L78-L87)). The pill itself is a rounded, translucent-filled capsule with a hairline stroke, and the pulse renders as a solid core dot plus two translucent glow rings that bloom with the pulse amount ([overlay.rs:92-118](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L92-L118)).

Sources: [overlay.rs:1-120](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/overlay.rs#L1-L120)
<!-- END:AUTOGEN hark_12_desktop_ui_overlay -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_window -->
## Settings Window Shell

`ui::shell::show` lays out the window in a fixed order every frame: the status footer claims the full window width first as the always-visible truth about the pipeline, then an optional update banner, then a fixed-width left sidebar, then the routed page content in the remaining central panel ([shell.rs:1-2](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L1-L2), [shell.rs:16-71](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L16-L71)).

| Component | Built by | Responsibility |
|---|---|---|
| Status footer | `footer::show` | Always-visible pipeline status; clicking it switches to the Settings page ([shell.rs:26-31](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L26-L31)) |
| Update banner | `banner` | Accent-filled top strip shown only while `updater.banner_visible()`; shows Downloading / Ready-to-install / Available phases with a primary action and a dismiss ([shell.rs:33-37](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L33-L37), [shell.rs:76-133](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L76-L133)) |
| Sidebar | `sidebar` | Fixed `184.0` px left panel: a Hark wordmark, nav rows for History/Dictionary/Stats, then Settings pinned above the version caption at the bottom ([shell.rs:13](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L13), [shell.rs:167-198](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L167-L198)) |
| Content | `pages::show` inside `CentralPanel` | Routes to the currently selected page ([shell.rs:53-70](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L53-L70)) |

The update banner's primary action changes with the updater's phase: nothing while installing, "Restart now" once the update is ready, and otherwise a "Details" jump to Settings plus either "Install" (self-update path) or "View release" (opens the GitHub release page) ([shell.rs:136-165](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L136-L165)). Each nav row in the sidebar is a plain button that fills with the theme's accent color and swaps to `ON_ACCENT` text when it is the selected page, so the selected state never depends on color alone ([shell.rs:201-217](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L201-L217)).

Sources: [ui/shell.rs:1-218](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/shell.rs#L1-L218), [ui/pages.rs:1-4](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L1-L4), [ui/mod.rs:1-15](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/mod.rs#L1-L15)
<!-- END:AUTOGEN hark_12_desktop_ui_window -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_pages -->
## History, Stats and Settings

`ui::pages::Page` is the routing enum the shell's `CentralPanel` matches on; each page carries its own icon, nav label, and one-line description, and the content column narrows for the Settings form and widens for the list-style pages ([pages.rs:17-52](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L17-L52), [pages.rs:63-69](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L63-L69)):

| Page | Icon | Description | Column width |
|---|---|---|---|
| History | Clock-counter-clockwise | "Your dictations, newest first. Everything stays on this device." | 720 px |
| Dictionary | Book-open | "Names and terms your STT provider keeps missing." | 720 px |
| Stats | Chart-bar | "Lifetime dictation figures. They survive a history clear." | 720 px |
| Settings | Gear | "Provider, key, hotkey, and voice." | 560 px (narrow form) |

Every page still ships honest empty, gated, and error states; a blank region is treated as a bug, not an acceptable idle state ([pages.rs:1-4](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L1-L4)).

### History

`HistoryPage` shows a search-as-you-type toolbar, day-group headers, expandable rows with copy/delete, and a "Clear all" action gated behind a confirm dialog ([history/mod.rs:1-4](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L1-L4)). It renders from a cached window of reader-connection queries keyed on `(write generation, search text, pages loaded)`, and only re-queries the database when that key moves, so idle frames never touch SQLite ([history/mod.rs:4-11](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L4-L11), [history/mod.rs:137-172](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L137-L172)). It distinguishes three empty states explicitly: storage unavailable, zero dictations ever, and zero matches for the current search ([history/mod.rs:73-111](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L73-L111)). `Ctrl+F` focuses the search box from anywhere on the page, `Esc` collapses the expanded row unless a confirm dialog owns `Esc`, and "Show more" grows the loaded window by `PAGE_SIZE` (100) rows rather than loading everything at once ([history/mod.rs:22](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L22), [history/mod.rs:84-87](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L84-L87), [history/mod.rs:115-121](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L115-L121), [history/mod.rs:239-246](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L239-L246)).

### Stats

`StatsPage` gates on a minimum of 10 dictations before showing any numbers, presenting a progress bar toward that threshold instead of a zeroed dashboard ([stats.rs:1-3](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L1-L3), [stats.rs:13](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L13), [stats.rs:65-68](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L65-L68), [stats.rs:131-147](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L131-L147)). Once unlocked it shows 2x2 lifetime cards (dictations, words, speaking time, average release-to-inject), a time-saved estimate versus typing at 40 WPM, and the since-date, with "Reset stats" behind its own confirm that explicitly promises history entries stay untouched ([stats.rs:70-97](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L70-L97), [stats.rs:151-181](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L151-L181)). The average release-to-inject figure is `None` (rendered "n/a") rather than a false zero when a pre-migration database has no total-time sum yet ([stats.rs:201-204](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L201-L204)).

### Settings

`SettingsPage` edits a draft `Settings` in place; the key section writes straight to the OS keychain, "Test connection" runs on a background thread, and "Save" validates, persists to TOML, and restarts the pipeline ([settings/mod.rs:1-6](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L1-L6), [settings/mod.rs:146-167](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L146-L167)). It is composed from sub-modules for the onboarding "Get Started" card, provider/model/mic/hotkey/voice/cleanup/behavior/privacy form sections, key storage, connection testing, and the update settings section ([settings/mod.rs:7-13](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L7-L13), [settings/mod.rs:56-104](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L56-L104)). Some changes take effect immediately rather than waiting for Save: storing or removing a key for the currently-running provider restarts the pipeline right away, and starting a hotkey capture stops the pipeline's own keyboard hook so only one low-level hook runs at a time ([settings/mod.rs:80-96](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L80-L96)). A failed save leaves the app running with the pipeline stopped and a visible cause under the Save button, never a silent dead state ([settings/mod.rs:112-121](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L112-L121), [settings/mod.rs:165-166](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L165-L166)).

Sources: [ui/pages.rs:1-131](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/pages.rs#L1-L131), [ui/history/mod.rs:1-309](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/history/mod.rs#L1-L309), [ui/stats.rs:1-238](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/stats.rs#L1-L238), [ui/settings/mod.rs:1-178](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/ui/settings/mod.rs#L1-L178)
<!-- END:AUTOGEN hark_12_desktop_ui_pages -->

---

<!-- BEGIN:AUTOGEN hark_12_desktop_ui_theme -->
## Theming

`theme.rs` is the single home for every color, size, spacing, and font token; `theme::apply` runs once at startup and no panel is meant to set an ad-hoc color, size, or spacing value inline ([theme.rs:1-4](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L1-L4)). It installs fonts and the type scale, applies both light and dark `Visuals`, and re-applies whatever theme preference egui already restored from memory rather than forcing `System` ([theme.rs:127-143](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L127-L143)):

```rust
pub fn apply(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    ctx.all_styles_mut(|style| {
        style.text_styles = text_styles();
        spacing(&mut style.spacing);
    });
    ctx.set_visuals_of(Theme::Dark, dark_visuals());
    ctx.set_visuals_of(Theme::Light, light_visuals());
    let preference = ctx.options(|o| o.theme_preference);
    ctx.set_theme(preference);
}
```

Sources: [theme.rs:130-143](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L130-L143)

Fonts are Inter (Regular/Medium/SemiBold, each its own `FontFamily` since egui cannot interpolate variable-font weights) plus JetBrains Mono for transcripts and latency figures and vendored Phosphor glyphs for icons, all embedded via `include_bytes!` ([theme.rs:145-199](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L145-L199)). `theme::icons` exposes the curated Phosphor codepoint constants used across the tray, sidebar, and pages ([theme.rs:19-43](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L19-L43)).

Both palettes share a small set of theme-independent semantic colors, always paired with an icon or label rather than used as the sole carrier of a state:

| Token | Value | Used for |
|---|---|---|
| `DANGER` | `#E5484D` | Destructive intent (Clear all, Reset stats), the tray "Recording"/"Error" fill |
| `DANGER_FILL` | `#C62A30` | Fill behind `ON_ACCENT` text on destructive buttons |
| `SUCCESS` | `#30A46C` | Passing test / saved-successfully notices |
| `WARNING` | `#F5A524` | Needs-key tray state, warning empty states |
| `TRAY_ACCENT` / `TRAY_STOPPED` | dark accent / gray | Tray icon fills, which are drawn into RGBA bitmaps and cannot follow the light/dark theme |

Sources: [theme.rs:69-96](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L69-L96)

The dark and light `Visuals` are both built from one shared `Palette` struct and `build_visuals` helper, so widget fills, hairline strokes, selection/focus color (which doubles as the 2 px visible focus ring), and shadows stay structurally identical between themes and only the color values differ ([theme.rs:230-302](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L230-L302), [theme.rs:304-342](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L304-L342)). The recording overlay's palette is the one exception: it is always dark since it must read over arbitrary desktop content, so its tokens are fixed rather than theme-paired ([theme.rs:85-95](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L85-L95)). A dedicated test suite pins body text, weak text, and accent-on-fill contrast to WCAG AA (>= 4.5:1 for text, >= 3:1 for non-text indicators) for both palettes ([theme.rs:344-483](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L344-L483)).

Sources: [theme.rs:1-484](https://github.com/BoardPandas/Hark/blob/1c1738716fa4cd758b0c26ec94d0873d1bc35ac1/crates/hark-app/src/theme.rs#L1-L484)
<!-- END:AUTOGEN hark_12_desktop_ui_theme -->

---
