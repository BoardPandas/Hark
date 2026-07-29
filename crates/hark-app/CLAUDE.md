# hark-app — Crate Rules

The desktop binary: eframe window (tray from CP5) on the **main thread**, the
dictation pipeline on worker threads via `PipelineController`. Root
`CLAUDE.md` threading rule applies with no exceptions.

## eframe 0.35 conventions (verified against registry source 2026-07-16)

- `App::ui(&mut self, ui: &mut egui::Ui, frame)` is the only required
  method; the root `Ui` has no margin or background. `App::logic(ctx, frame)`
  runs before every `ui` call AND while the window is hidden whenever
  `request_repaint` fires; event draining lives there, painting never does.
- Panels are the unified `egui::Panel` (`Panel::left("id")`, `::bottom`,
  `.exact_size`); `SidePanel`/`TopBottomPanel` and `show_inside` are gone or
  deprecated in 0.35. `CentralPanel::default().show(ui, ..)` last.
- Cross-thread UI wake-up is **`app::wake_ui(&ctx)`**, never
  `Context::request_repaint()`. The latter is
  `request_repaint_of(viewport_id())`, and `viewport_id()` is whichever
  viewport is mid-pass — from a worker thread that is a race, and during a
  dictation the recording pill (a second viewport, ~60 fps) wins it, so
  eframe discards the request as outdated and the main window never wakes.
  Only the root viewport runs `App::logic`. Never poll or sleep on the UI
  thread; idle CPU stays near zero.
- **`persist_window` is off and must stay off.** eframe's auto-save runs at
  the end of *whichever* viewport just painted and writes that viewport's
  geometry under the root window's key, so the recording pill kept being
  saved as "the Hark window" (a 160x40 main window on the next launch).
  Window geometry lives in `window_state.rs`, captured in `App::logic` —
  which runs for the root viewport only — and applied on the first pass,
  before the window is shown.

## Design system discipline

- **The UI / latency / accessibility SLA is
  [`.claude/references/design-guardrails.md`](../../.claude/references/design-guardrails.md)** —
  read it before changing panels, tokens, or anything on the release-to-inject
  path. It is cited here, in the crate file that loads whenever hark-app is
  touched, because a pointer sitting only in the root `CLAUDE.md` has no
  trigger and never fires.
- **Every color, size, spacing, and font token lives in `theme.rs`**; no
  panel sets ad-hoc values inline. New tokens go into `theme.rs` with a
  contrast test when they carry text.
- Icons are the vendored Phosphor glyphs in `theme::icons` (constants match
  the embedded `Phosphor.ttf`; see `assets/README.md` before touching
  either). If egui-phosphor ships an egui-0.35 release, swapping back is a
  drop-in.
- Status is never conveyed by color alone: icon + label, always.
- UI modules stay under ~300 lines; split panels into widgets.

## Content hygiene

- `DictationRecord` (transcript content) flows worker -> UI channel ->
  history panel/DB only. It has no `Debug` impl on purpose; never format it
  into a log line. Logs carry lengths, counts, millis, and config labels.
- Key material never reaches this crate: hark-keychain resolves keys and
  hands them straight to `hark_pipeline::run`; the UI sees `KeyStatus`
  labels only. Cache `key_status` lookups (OS keychain I/O), never call
  them per frame.

## Build

- `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`:
  debug keeps the console for logs; release is windowless, so any console
  child process must set `CREATE_NO_WINDOW` (LL-G HIGH, standing).
- This machine builds and tests only; the window itself is validated on
  real Windows/macOS hardware (CP6).
