# Phase 4: Settings/history UI + storage

**Date:** 2026-07-16. **Status:** PLANNED (execute in a later session per project rule).
**Prereq:** Phase 3 CP0-CP4 complete (`main` @ `febcb5c`, v0.9.1, 236 tests green). Phase 3's CP0 live spike and CP5 interactive gate are absorbed into this phase (CP6 below).
**Master plan:** `tasks/plan-repo.md` §8 Phase 4, §9 config/secrets table. **Handoff:** `tasks/2026-07-16-handoff-phase4-planning.md`. **UI SLA:** `.claude/references/design-guardrails.md`.

## 1. Goal

Hark becomes a real desktop app: a `hark-app` binary whose main thread owns the eframe window and tray icon, with the dictation pipeline unchanged on worker threads. The window carries settings (provider config, BYOK key paste straight to the OS keychain, test-connection), the dictionary editor, history, and stats; rusqlite storage lands with retention pruning; first-run onboarding gets a user from empty machine to first dictation without touching a terminal. The deferred Phase 3 items (cleanup model spike, interactive voice gate) run once the key field exists.

## 2. Decisions made with the user (2026-07-16)

1. **Window-first sequencing.** The settings window (with the key field) lands before the tray icon; it unblocks everything else.
2. **Guided settings onboarding, no wizard.** When no STT key resolves at startup, the settings window auto-opens on a "Get started" panel (pick provider, paste key, test, dictate hint). No separate wizard screens.
3. **History stores raw + final.** Each entry keeps the raw STT transcript and the final injected text (plus voice/model labels); the list shows final, raw expands per row.
4. **Keychain-reading spike.** `cleanup_spike` learns to resolve keys via hark-keychain (per-provider env vars still win); it runs once mid-phase to pin the chat model defaults, then the interactive gate runs.
5. **Add `hark-app`, retire `hark-cli`.** hark-app replaces hark-cli entirely; the CLI crate is deleted at the end of this phase (CP7), after hark-app dictates end to end. The `--voice` flag is superseded by the voice picker; the per-crate `examples/` harnesses (transcribe_spike, cleanup_spike) remain the headless dev tools.
6. **Retention defaults confirmed: last 1,000 entries or 90 days**, whichever prunes first; both configurable; lifetime stats survive pruning and clears.
7. **A designed, modern UI is in scope now, not Phase 5 polish (user directive, 2026-07-16).** hark-app ships with a real visual identity from CP2: embedded fonts, a hand-rolled light/dark theme, a sidebar layout, and the UX flows of §3.10 and §3.11. Only asset-level polish (tray icon art, a floating recording pill) remains Phase 5.

## 3. Design

### 3.1 Crates and process shape

- **`hark-app`** (new, binary): main thread owns the eframe event loop, the tray icon, and all UI modules (`src/ui/*.rs`, each under ~300 lines per the guardrails). It resolves keys via hark-keychain, calls `hark_pipeline::run`, owns the `PipelineHandle`, and restarts the pipeline when settings change. `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` from day one: console logs in debug, no console window in release.
- **`hark-store`** (new, lib): rusqlite only (schema, migrations, queries, pruning). TOML settings stay in hark-config; this is a deliberate divergence from master plan §5's "hark-store also does TOML" (hark-config already exists and works).
- **No separate `hark-ui` lib crate** (second divergence from §5): hark-app is its only consumer; module tree beats a premature crate. Revisit only if something else ever needs the widgets.
- **`hark-pipeline` API change:** `run(settings, api_key)` becomes `run(settings, api_key, events: Sender<PipelineEvent>)`. Only hark-cli calls it today and hark-cli is retiring, so no compatibility shim.
- **`hark-cli`** is deleted at CP7.
- Create `crates/hark-app/CLAUDE.md` when the crate lands (main-thread rules, egui conventions), per the root CLAUDE.md hierarchy note.

### 3.2 Threads, events, and the hot path

Three thread groups; channels between them; the hot path is untouched:

- **Main thread:** eframe UI + tray. Never blocks on pipeline or DB work.
- **Pipeline worker threads** (existing): hotkey, audio, STT, cleanup, inject. New: the worker emits `PipelineEvent`s over the channel passed to `run`. Emitting is a non-blocking send; a full/disconnected channel is ignored (events are advisory, dictation never waits on the UI).
- **Storage thread** (new, owned by hark-app): receives `Injected` events, writes history + stats, runs retention pruning. DB writes therefore happen after injection and off both the hot path and the UI thread.

```rust
pub enum PipelineEvent {
    Recording,                       // PTT down
    Processing,                      // PTT up, request in flight
    Injected(DictationRecord),       // full record for storage + UI
    Failed { stage: FailStage, detail: String },  // labels only, never keys
}
```

`DictationRecord` carries raw text, final text, voice/provider/model labels, and timing millis. It is the sanctioned in-process carrier of transcript content (destination: the local DB and the history panel); logging discipline is unchanged (counts, millis, labels only).

UI updates: the main thread drains the event receiver each frame; the storage thread calls `egui::Context::request_repaint()` (cloned ctx) after a write so the history panel refreshes while idle.

### 3.3 Storage schema (hark-store)

- rusqlite 0.40.1, `bundled` feature (ships SQLite 3.53.2, no system dependency). DB at the OS data dir (`hark.db`), resolved the same way hark-config finds the config dir (extend that helper for the data dir; on Windows they may coincide under `%APPDATA%\Hark\`).
- **WAL mode**, two connections: the storage thread owns the writer; the UI thread owns a reader for paged history queries (LIMIT/OFFSET pages, sub-ms on an indexed table; fine in immediate mode).
- **Migrations: numbered, immutable, embedded** (`migrations/001_init.sql` via `include_str!`, applied by `PRAGMA user_version`). Never renumber an applied migration (BP FOUNDATIONAL `never-renumber-applied-migrations`).

```sql
CREATE TABLE entries (
  id           INTEGER PRIMARY KEY,
  ts_ms        INTEGER NOT NULL,
  raw_text     TEXT NOT NULL,
  final_text   TEXT NOT NULL,        -- equals raw_text when no cleanup ran
  voice        TEXT NOT NULL,
  stt_provider TEXT NOT NULL,
  stt_model    TEXT NOT NULL,
  cleanup_model TEXT,                 -- NULL when cleanup did not run
  stt_ms       INTEGER NOT NULL,
  cleanup_ms   INTEGER,
  total_ms     INTEGER NOT NULL
);
CREATE INDEX idx_entries_ts ON entries(ts_ms);

CREATE TABLE stats (                  -- exactly one row, fixed id
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  dictations    INTEGER NOT NULL DEFAULT 0,
  words         INTEGER NOT NULL DEFAULT 0,
  audio_ms      INTEGER NOT NULL DEFAULT 0,
  stt_ms        INTEGER NOT NULL DEFAULT 0,
  cleanup_ms    INTEGER NOT NULL DEFAULT 0,
  since_ts_ms   INTEGER NOT NULL
);
```

- Stats upserts key on the fixed `id = 1`, never on a display name (LL-G MEDIUM `sqlite/upsert-by-name-collision`). Per-provider breakdowns are future columns/tables keyed on provider labels only if labels are guaranteed stable; not this phase.
- **Pruning** runs on the storage thread after each insert: delete rows older than `max_age_days` and rows beyond the newest `max_entries`.
- **Independence rule:** "Clear history" deletes `entries` only. "Reset stats" zeroes the `stats` row only. Two controls, two confirms (guardrails §4/§5).
- **Capture toggle semantics:** `history.capture = false` means no `entries` rows are written (no transcript content persisted); the numeric `stats` counters still tick (they carry no content). Stated in the UI next to the toggle.

### 3.4 Config additions (hark-config)

Additive only, same `#[serde(default)]` pattern; still no migration machinery, but the **version stamp lands now** so the first breaking change later ships backup-then-migrate without retrofitting (BP `versioned-config-migration-backup`; its flow when triggered: load, check `version < current`, back up as `config.toml.v{version}.bak`, explicit field mappings, bump + persist immediately).

```toml
version = 1                # NEW: schema stamp; fresh installs write the current value

[history]                  # NEW
capture = true             # false: no dictation content stored; counters still tick
max_entries = 1000         # >= 1
max_age_days = 90          # >= 1
```

Settings saves from the UI serialize the full model back to TOML (hark-config gains a `save` path alongside `load`; preserve unknown keys is NOT attempted, the struct is the source of truth, and that is fine while the schema is additive).

### 3.5 Keychain writes (hark-keychain)

- New API alongside the resolvers: `store_key(account, key)`, `delete_key(account)`, `key_status(account) -> Stored | Missing | Backend(detail)`. keyring 4.1.5 (already pinned `=4.1.5`) exposes `Entry::set_password` and `Entry::delete_credential` (there is no `delete_password` in v4). `delete_key` treats `NoEntry` as success.
- `store_key` rejects empty/whitespace keys before touching the backend. `key_status` reads and immediately drops the value; the UI never holds key material beyond the paste buffer.
- Accounts unchanged: provider label, shared between STT and cleanup roles; `voice.provider.key_account` covers the two-openai-compatible-endpoints edge. Same hygiene rules: no key material in any error variant, no Debug on anything key-bearing, and **no test ever touches the real keyring** (Phase 3 lesson; env-override short-circuits keep tests deterministic).

### 3.6 Settings window + onboarding (window-first)

Panels (each its own module/widget): **Settings**, **Dictionary**, **History**, **Stats**, presented in the sidebar shell with status footer per §3.11 and styled by the §3.10 design system. Settings uses progressive disclosure: essentials visible, advanced knobs collapsed (§3.11).

Settings form:
- **STT provider:** kind picker (deepgram / openai / groq / openai-compatible), model field with per-kind defaults, base-URL override (required for openai-compatible), key section.
- **Key section (per provider):** masked paste field, Store button (writes keychain, clears the field), status line ("Key stored" / "No key") from `key_status`, Remove button. A stored key is never displayed back, in full or in part.
- **Test connection:** background thread (never blocks a frame). STT test = transcribe a bundled ~1 s fixture WAV through the configured provider+model and show text + latency (validates auth, model name, and the full path; Groq's 10 s minimum billing gets one line of honest copy). Cleanup test (when a cleanup call would run, inherited or explicit) = one tiny chat call showing model + latency. Results render inline under the button and persist until the next test; pass/fail styling per §3.11 (never a vanishing toast).
- **Cleanup/voice section:** voice picker (verbatim/clean/professional/casual/custom + custom prompt box), `skip_below_words`, and the provider: "Inherited from STT (openai)" as the default display, expandable to explicit `[voice.provider]` config (kind/model/base_url/key_account + its own key section when the account differs).
- **Hotkey:** text field bound to `PttChord::parse` with inline validation (capture-a-keypress UX is Phase 5 polish).
- **History/privacy:** capture toggle, retention caps, and the honest disclosure block (audio goes to the STT provider on every dictation; text to the cleanup provider on non-Verbatim voices; history/stats/dictionary stay local).
- **Save** = validate (reuse hark-config validation), persist TOML, then **pipeline restart**: drop the old `PipelineHandle` (its Drop already sequences listener, worker, capture), re-resolve keys, `run` again, surface the result in the status area. Failures leave the app running with the pipeline stopped and a visible error; never a silent dead state.

Onboarding: at startup, if no STT key resolves, the window opens focused on a Get Started strip at the top of Settings (provider picker, key paste, test button, then "hold <key> and speak" once the test passes). If a key resolves, the pipeline starts and the window stays hidden (until the tray CP lands, CP2/CP3 show the window on launch and close = quit).

Accessibility (acceptance criteria at every UI CP): full keyboard navigation with visible focus, OS light/dark followed (egui 0.29+ theme handling), status never conveyed by color alone (icon or label paired).

### 3.7 History + stats panels

- History list virtualizes via `ScrollArea::show_rows` over paged reader queries; newest first, under day group headers (Today / Yesterday / date). A toolbar carries search-as-you-type over raw + final text (a LIKE query on the reader connection is fine at the 1,000-row cap; revisit FTS5 only if retention rises), the entry count, and Clear all. Each row: final text (two-line truncate), a caption with relative time + voice + model labels (guardrails: disappointing output must have an obvious cause), and always-visible **copy** and **delete** buttons; copy affirms inline ("Copied", fades). Expanding a row reveals the raw transcript (mono), the timing breakdown (stt / cleanup / total ms), and the full timestamp. Clear-all behind an explicit confirm. Full layout + empty states: §3.11.
- Stats panel: gated until 10 dictations (progress placeholder, never a zeroed dashboard); then stat cards for dictations, words, speaking time, and average release-to-inject (derived from sums), plus a derived "time saved vs typing" line and the since-date; reset behind its own confirm, clearly labeled as separate from history.

### 3.8 Tray (after the window works)

- tray-icon 0.24.1 + muda 0.19.3. The icon must be created **on the main thread after the event loop is running**; inside eframe that means lazily on the first `App` callback, not before `run_native`. Menu and tray events arrive via `MenuEvent::receiver()` / `TrayIconEvent::receiver()` drained each frame (plus `request_repaint` so a hidden window still processes them promptly).
- Menu stays trivial: voice radio group, Open Settings, Quit. Selecting a voice updates config + restarts the pipeline (cheap) or, if feasible, swaps the worker's voice without restart; decide at execution, restart is the acceptable baseline.
- Tray states: idle / recording / processing / error variants (no network, key rejected, provider error) with distinct icons + tooltips; simple generated icons are fine this phase, polished assets are Phase 5.
- Once the tray exists: window close = hide, Quit lives in the tray menu; the app becomes the daemon it is meant to be.

### 3.9 Crate versions (verified 2026-07-16; re-verify at execution)

| Crate | Version | Notes |
|---|---|---|
| egui / eframe | 0.35.0 (2026-06-25) | 0.34 split `App::update` into `logic` + `ui` callbacks and made wgpu the default renderer; 0.35 removed all deprecated items, so pre-0.34 examples will not compile. **Trait shape now confirmed from docs.rs 0.35 source (2026-07-16): `ui(&mut self, ui, frame)` is the sole required method; `logic()`, `save()`, `on_exit()` are defaulted.** `glow` renderer feature is the fallback if wgpu is heavy. Enable `persistence` for window geometry. |
| winit | 0.30.13 | Pinned by eframe; not a direct dependency unless needed. |
| tray-icon | 0.24.1 (2026-06-10) | Create after the loop runs, on the main thread. |
| muda | 0.19.3 | tray-icon's menu dep. |
| rusqlite | 0.40.1 | `bundled` = SQLite 3.53.2. |
| keyring | 4.1.5 | Already pinned `=4.1.5`; `set_password` / `delete_credential`. |
| egui_extras | 0.35.0 | Same release train as egui (no skew risk); `TableBuilder`/`StripBuilder` available if plain layouts fall short. |
| egui-phosphor | 0.12.0 | Icon font. **Pins egui ^0.34 as of 2026-07-16 (one minor behind).** Decide at CP2: adopt if bumped, else dep-patch it or use the curated-glyph fallback (§3.10). |
| Inter + JetBrains Mono | latest static TTFs | Embedded fonts, both SIL OFL 1.1. Static weights only: egui has no variable-font weight axis (emilk/egui#1862). |

Built-ins over dependencies: confirms use `egui::Modal` (in core since 0.30), micro-motion uses `Context::animate_value_with_time`, and async-to-UI messaging stays on the existing channel + `request_repaint` pattern. egui-modal, egui-notify, and catppuccin-egui all lag 0.35 (pinned to 0.30 / 0.34 / 0.33 respectively as of 2026-07-16) and are not used.

### 3.10 Visual design system

The window must read as a designed product, not an egui demo (decision §2.7). All tokens live in one `theme.rs` module in hark-app; `theme::apply(ctx)` runs once at startup (and on theme-preference change). No panel sets ad-hoc colors, sizes, or spacing inline. API names verified against egui 0.35 docs on 2026-07-16.

- **Type.** Embedded fonts (`include_bytes!` assets): Inter Regular / Medium / SemiBold, each registered as its own font family (egui cannot interpolate variable-font weights), plus JetBrains Mono for transcripts, latency figures, and hotkey chips. Scale via `style.text_styles`: Heading 22 (SemiBold), `Name("Subheading")` 16 (Medium), Body 14, Button 14 (Medium), Small 11.5, Monospace 13. Secondary text uses `weak_text_color`, never an ad-hoc smaller size.
- **Color.** Two hand-rolled `Visuals` (light + dark), following the OS by default (`ThemePreference::System`; a Light/Dark/System radio in Settings). Neutrals: dark window fill #111317, panel fill #16181D, hairlines #26282F; light window #FAFAFC, panels #FFFFFF, hairlines #E4E4EA. One accent (indigo, #7C7FF2 dark / #5B5BD6 light) for the selected nav pill, primary buttons, focus rings, selection, and links. Semantic colors, always paired with an icon or label (guardrails §3): recording/error red #E5484D, success green #30A46C, warning amber #F5A524. All hexes are starting values, contrast-tuned on real screens at CP2/CP3.
- **Shape + depth.** `CornerRadius` (the 0.31 rename of `Rounding`): 6 on widgets, 8 on cards and menus, 10 on windows. Soft low-alpha `window_shadow`/`popup_shadow`; 1 px hairline strokes instead of egui's default bevels. At most one primary (accent-filled) button per view; every other button is quiet (`weak_bg_fill` + hairline).
- **Spacing.** 4 px base grid: `item_spacing` (8, 10), `button_padding` (14, 7), `window_margin` and `menu_margin` 16, `indent` 18, `interact_size.y` 30 (comfortable targets, uniform row heights).
- **Iconography.** egui-phosphor if it has bumped to 0.35 by CP2; else dep-patch it (a thin font-constant crate) or embed a curated ~12-glyph set (mic, gear, book, clock, chart, key, check, x, copy, trash, warning, play).
- **Motion.** `Context::animate_value_with_time` only: row expand (~140 ms), status footer transitions, "Copied" fade (~800 ms). Motion is decorative; nothing waits on it, and every state reads correctly with animation ignored (reduced-motion guardrail).
- **Copy voice.** Short, honest, sentence case ("Key stored", "No key yet", "Listening for Ctrl+Alt+Space"). Errors name the cause and the next step ("Deepgram rejected the key. Check it in Settings."). Latency always in ms, mono font.

### 3.11 Layout, flows, and states

UX patterns adopted from a 2026-07-16 survey of Wispr Flow, Superwhisper, MacWhisper, VoiceInk, and Windows Voice Access: inline test-and-save key validation, progressive disclosure, gated stats, searchable day-grouped history. Anti-patterns explicitly rejected: settings-as-server-config (Superwhisper's most-cited complaint), multi-screen wizards, modals for transient recording state, zeroed stats dashboards, and vanishing-toast-only feedback for important outcomes.

- **Shell.** Fixed left sidebar (`SidePanel::left`, ~184 px): wordmark at top; nav items History, Dictionary, Stats (icon + label; the selected item gets an accent pill); Settings pinned at the bottom above a version caption. Content in `CentralPanel` as one centered column (max ~560 px for forms, ~720 px for lists), each page headed by a Heading plus a one-line weak-text description. Default panel is History once a key resolves; Settings (Get Started) when none does.
- **Status footer.** Persistent full-width strip (~28 px) below the content: left = pipeline state icon + label (Idle: "Listening for <chord>" / Recording / Processing / Error: cause plus an "Open Settings" jump when key-related); right = active provider · model (and cleanup model when a non-Verbatim voice is active). `egui::Sides` fits this layout. The footer is the always-visible truth about the pipeline; there is no silent dead state.
- **Settings, progressive disclosure.** Visible by default: provider kind radio row, key section, hotkey field, voice picker. Collapsed `CollapsingHeader`s: "Model & endpoint" (model override, base URL; auto-expanded for openai-compatible where base URL is required), "Cleanup provider" (inherited display + explicit override), "Behavior" (`skip_below_words`), "History & privacy" (capture toggle, retention caps, disclosure block). Never the flat full-knob dump.
- **Get Started card.** When no STT key resolves: a card at the top of Settings with three numbered inline steps (1 pick provider, 2 paste key + Store, 3 Test), each earning a check as completed. On a passing test the card swaps to its success state: "Hold [chord keycaps] and speak into any text field." The card disappears permanently after the first successful dictation (or explicit dismiss). No wizard screens, nothing modal.
- **Inline, persistent test results.** Pass: check icon + fixture transcript + latency (ms, mono). Fail: cause-specific one-liner (rejected key vs network vs unknown model). Rendered under the Test button and kept until the next test.
- **History.** Toolbar (search field, entry count, Clear all as a quiet danger button), day headers, virtualized rows: two-line final text, caption (relative time · voice · model), always-visible quiet copy/delete icon buttons. Copy affirms inline. Click expands a row: raw transcript (mono), stt/cleanup/total ms, full timestamp; Esc or re-click collapses. Empty states: "Dictations appear here. Hold [chord] and speak." and "No matches for '<query>'."
- **Stats.** Under 10 dictations: a progress placeholder ("3 of 10 dictations to unlock stats"), never zeros. Unlocked: 2x2 stat cards (dictations, words, speaking time, avg release-to-inject) plus a "time saved vs typing at 40 WPM" line, a since-date caption, and Reset stats (its own confirm; copy notes history is untouched).
- **Dictionary.** Pinned add field (Enter adds), inline edit, per-row delete; header caption: "Corrections apply on this device after transcription; entries are also sent to your STT provider as accuracy hints."
- **Confirms.** Built-in `egui::Modal`; the destructive button carries the verb and count ("Delete 214 entries") and danger styling; Cancel is the focused default.
- **Keyboard + focus.** Tab reaches every control in visual order; focus ring = 2 px accent stroke, visible in both themes; Esc closes modals and expanded rows; Ctrl+F focuses search in History. CP3/CP4 acceptance includes a full mouse-unplugged pass.
- **State coverage rule.** Every panel ships its empty, in-progress, and error states; a blank region is a bug.
- **Deferred to Phase 5 (recorded, not forgotten):** a floating always-on-top recording pill with waveform (Wispr-style) and press-length hotkey dual mode (tap = toggle, hold = push-to-talk). Tray states + the status footer are this phase's recording feedback (guardrails §2 permits either).

## 4. Checkpoints

One commit per CP; fmt + clippy `-D warnings` + full tests green at each; CHANGELOG.md + package.json bump every commit (`.claude/rules/commit-changelog.md`). Patch unless marked Minor.

- **CP0, hark-store:** crate, migrations 001, insert/query/delete/clear/reset APIs, pruning, WAL, capture semantics; in-memory-DB tests (schema, pruning boundaries, stats independence from clears, fixed-id upsert).
- **CP1, keychain writes + config:** `store_key`/`delete_key`/`key_status`; config `version` stamp, `[history]` table, validation, TOML save path; tests (empty-key rejection, NoEntry-tolerant delete, round-trip save/load, no key material in any error).
- **CP2, hark-app shell (Minor):** binary crate, eframe 0.35 window on the main thread, key resolution, `PipelineEvent` channel added to hark-pipeline, pipeline start/stop, clean shutdown, debug/release console split. Close = quit for now. The design system lands here, not later: `theme.rs` tokens (fonts, type scale, spacing, light/dark `Visuals`, §3.10), the sidebar shell + status footer (§3.11; the footer is the live status surface), and the icon decision (phosphor vs curated glyphs). First execution step: compile a minimal `App` impl to confirm the 0.35 trait in practice, then build on it.
- **CP3, settings + onboarding (Minor):** full settings form per §3.6 incl. key section, test-connection (inline persistent results), cleanup provider section, dictionary editor, privacy disclosure; save-validate-restart flow; Get Started card on missing key (§3.11). Acceptance: progressive disclosure holds (essentials-only default view) and a full keyboard-only pass with the mouse unplugged.
- **CP4, history + stats (Minor):** storage thread consuming `Injected` events, writes + pruning live; history panel (virtualized, search, day grouping, copy/delete, expandable raw, clear-all confirm, both empty states); stats panel (unlock gate, stat cards, reset confirm). Verify a DB write never precedes injection.
- **CP5, tray (Minor):** tray icon + menu (voices, Open Settings, Quit), state icons incl. error states, close-to-hide, event draining + repaint. The app now launches hidden when a key resolves.
- **CP6, live validation — DONE (2026-07-17).** Ran the interactive checklist through hark-app on real Windows hardware; confirmed working end to end (hold-key dictation injects). A microphone-picker (input-device selection, `hark_audio::list_input_devices`, WASAPI-COM-off-the-UI-thread) was added during this pass and is in the shipped build. Per-metric figures (release-to-inject p50/p95, per-voice results) were not transcribed into the spec this session; add them here if captured. The signed-release pipeline (a pulled-forward Phase 5 item, §3.8-adjacent) also landed and is validated: `v0.13.4` published a signed, timestamped `Hark-<ver>-windows-x64.exe` via `.github/workflows/release.yml` (see `.github/RELEASING.md`).
- **CP7, retire hark-cli — NEXT:** delete the crate (decision §2.5), sweep references (README, docs, scripts; the release workflow already builds `-p hark-app` only), confirm the workspace builds and tests green without it. Handoff: `tasks/2026-07-17-handoff-phase4-cp7.md`.

macOS: everything is built macOS-correct by construction (main-thread UI, tray after loop start) but **validated only on Windows** this phase; Mac validation still waits for hardware, unchanged.

## 5. Risks / open questions

- **eframe 0.35 `App` trait shape: resolved.** Confirmed from docs.rs source (2026-07-16): `ui()` is the sole required method, `logic()` is defaulted. The residual unknown is only how event draining + `request_repaint` timing behave across the two callbacks; prototype early in CP2.
- **Icon/toast ecosystem lags egui 0.35** (egui-phosphor and egui-notify pin 0.34, catppuccin-egui 0.33 as of 2026-07-16). Contained: the theme is hand-rolled, confirms use the built-in `Modal`, and §3.10 defines a curated-glyph icon fallback; at worst one thin dep-patch for phosphor.
- **Theme hexes are starting values.** §3.10 colors must be contrast-checked (roughly 4.5:1 for body text) in both themes on real hardware at CP2/CP3; expect a tuning pass.
- **Tray-icon inside eframe's abstracted loop.** The documented pattern is winit-level; creating the tray in the first App callback should satisfy "loop running, main thread". If eframe fights it, fallback is `NativeOptions::event_loop_builder` customization. Contained to CP5.
- **macOS Keychain prompt behavior unverified** (version research could not confirm whether first GUI access prompts). Harmless on Windows Credential Manager; note for Mac validation later.
- **Pipeline restart races:** the old `PipelineHandle` must fully drop (LL hook unregistered) before `run` re-registers; Drop ordering is already load-bearing, but rapid save-save-save needs a manual test at CP3.
- **wgpu default renderer** may be heavier than Hark needs (binary size, GPU init time); measure at CP2, switch to `glow` feature if it hurts startup.
- **Voice swap without pipeline restart** (tray picker) is an optimization; restart is the correct-by-construction baseline. Decide at CP5.
- **Settings save rewrites config.toml from the struct**, dropping unknown keys a user may have hand-added. Acceptable while the schema is additive and documented here; revisit if hand-editing becomes a supported workflow.

## 6. Lessons Learned / Gotchas

Pre-seeded (2026-07-16, planning research); confirm or amend during implementation, then route durable ones to LL-G via `/add-lesson`:

- **eframe 0.34 replaced `App::update` with `logic` + `ui` callbacks and made wgpu the default renderer; 0.35 deleted all deprecated items.** Any pre-0.34 example or template will not compile. Verify the trait from source before writing the impl. (Candidate LL-G entry once confirmed at CP2.)
- **tray-icon must be created on the main thread after the event loop is running** (macOS hard requirement; earliest is effectively the first frame inside eframe). Creating it before `run_native` fails only on Mac, so a Windows-only dev loop will not catch it: get it right by construction.
- **keyring v4 has no `delete_password`; deletion is `delete_credential`.** `set_password`/`get_password` names are unchanged. Keyring 4.1.3 was yanked and re-released within days (4.1.5); keep the exact pin.
- **SQLite upserts key on stable identifiers, never display names** (LL-G `sqlite/upsert-by-name-collision`): the stats row uses a fixed id.
- **Numbered migrations are immutable once applied** (BP FOUNDATIONAL `never-renumber-applied-migrations`); conflicts get documented, not renumbered.
- **Config `version` stamp lands ahead of need** (BP `versioned-config-migration-backup`): additive schema needs no migration machinery yet, but the stamp means the first breaking change ships backup-then-migrate cleanly. Backup naming when that day comes: `config.toml.v{version}.bak`.
- **DB writes are post-injection and off the UI thread by architecture** (storage thread), not by discipline: the hot path cannot regress via a slow disk.
- **The DB is the sanctioned transcript store; logs still never carry content.** `DictationRecord` moves transcripts between threads; nothing formats it into a log line.
- **No test may touch the real OS keyring** (Phase 3 lesson, still binding for the new write API): empty-key and NoEntry paths are testable through the pure layer and env overrides.
- **`windows_subsystem` split from day one** (`cfg_attr(not(debug_assertions))`): debug builds keep the console for logs; release builds are windowless, and any future console child still needs `CREATE_NO_WINDOW` (LL-G HIGH, standing).
- **`Rounding` became `CornerRadius` in egui 0.31** (u8 storage; `Shadow` fields shrank to i8/u8): any pre-0.31 styling snippet fails to compile.
- **egui cannot interpolate variable-font weights** (emilk/egui#1862): embed one static TTF per weight, each as its own font family name; `FontData::index` picks faces out of a TTC.
- **egui companion crates habitually lag core minors** (2026-07-16: phosphor/notify at 0.34, catppuccin at 0.33, egui-modal at 0.30). Prefer built-ins (`egui::Modal` since 0.30, `Context::animate_value_with_time`, hand-rolled `Visuals`); budget a dep-patch only for thin crates like an icon font. (Candidate LL-G entry once confirmed at CP2.)
- **Per-theme custom `Visuals` under OS-follow exist since egui 0.29** (`ThemePreference::{Dark, Light, System}`), but the exact `Context` setter names were not pinned during planning; confirm them from the `Context` docs at CP2.

Filled in during implementation:

- **(CP0, 2026-07-16) libsqlite3-sys 0.38.1 requires a Rust toolchain newer than 1.94 and does not say so.** Its build script uses `cfg_select!` (unstable pre-1.95-ish, fine on 1.97.1) and the crate declares no `rust-version`, so an older toolchain fails with an opaque E0658 in the build script instead of an MSRV warning. rusqlite 0.40.1 pins `libsqlite3-sys ^0.38.1` exactly, so there is no downgrade escape hatch: the toolchain must move. Fixed by `rustup update` (1.94.0 to 1.97.1) and workspace `rust-version = "1.97"`. Candidate LL-G entry (rust or sqlite).
- **(CP0) An in-memory SQLite DB reports `journal_mode = memory`, not `wal`.** The open path must accept whatever `PRAGMA journal_mode = WAL` returns instead of asserting on "wal", or every in-memory test breaks.
- **(CP1) Unset `Option` config fields are omitted from saved TOML via explicit `#[serde(skip_serializing_if = "Option::is_none")]` on every Option field**; we deliberately did not rely on the toml 1.x serializer's own None handling.
- **(CP1) `keyring::Error` variants are constructible in tests** (`NoEntry`, `PlatformFailure(Box::new(io::Error::other(..)))`), which lets the delete/status outcome mapping be pure functions fully tested with zero real-keyring contact.
- **(CP1) `std::fs::rename` replaces an existing file on Windows** (verified by a save-twice test), so temp-write + rename is a safe repeated-save path with no truncation window.
- **(CP2, 2026-07-16) eframe 0.35 `App::ui` receives a root `&mut egui::Ui` (no margin, no background), not a `Context`.** Wrap content in `CentralPanel::default().show(ui, ..)`. `logic(ctx, frame)` gets the `Context`, runs before every `ui` call, and ALSO runs while the window is hidden whenever `request_repaint()` fires; that makes it the correct slot for event draining (and later tray-event polling at CP5). Confirms and sharpens the pre-seeded 0.34-trait-split lesson. (LL-G candidate, confirmed.)
- **(CP2) egui 0.35 unified the panels:** `SidePanel`/`TopBottomPanel` are gone; use `egui::Panel::left("id")` / `::bottom("id")` with `.exact_size(px)`, and `show_inside` is deprecated (renamed `show`, taking the parent `Ui`). Any 0.34-era layout snippet fails or warns.
- **(CP2) egui-phosphor 0.12.0 still pins egui ^0.34 (checked 2026-07-16), so the companion-crate-lag candidate is confirmed.** Resolution: vendor `res/Phosphor.ttf` plus the needed codepoint constants from the same crate package (`src/variants/regular.rs`), so glyphs and font cannot drift; swapping back to the crate when it bumps is a drop-in. Assets provenance documented in `crates/hark-app/assets/README.md`. (LL-G candidate, confirmed.)
- **(CP2) Per-theme Context setters confirmed for 0.35:** `ctx.set_visuals_of(Theme::Dark | Theme::Light, visuals)`, `ctx.all_styles_mut(..)` for shared text styles/spacing, `ctx.set_theme(ThemePreference::System)` for OS-follow. Closes the §5 open question.
- **(CP2) Inter 4.1 release zip carries the static per-weight text TTFs under `extras/ttf/`** (`Inter-{Regular,Medium,SemiBold}.ttf`, ~410-420 KB each; `InterDisplay-*` variants live alongside, don't grab those); license at zip root. JetBrainsMono 2.304 zip mirrors the repo layout (`fonts/ttf/JetBrainsMono-Regular.ttf`, OFL.txt at root).
- **(CP2) The worker-to-UI wake-up pattern:** a tiny pump thread `recv()`s pipeline events, forwards them to a UI-side channel, and calls `ctx.request_repaint()` per event. Zero idle cost, no frame polling, and it dies naturally when either side's channel closes on pipeline stop/restart.
- **(CP2) Dictation records label what actually ran:** when the cleanup call is skipped, gated, or fails, the record says `verbatim` with no cleanup model/ms, so history never blames a model that did not shape the text (guardrails: disappointing output must have an obvious cause).
- **(CP2) This build machine's Windows Application Control policy blocks `cargo build --release` (os error 4551 while executing a freshly built release artifact); debug builds, tests, and clippy are unaffected.** Consequence: the §5 wgpu-vs-glow size/startup measurement could not run here and moves to the real-hardware pass at CP6. Do not attempt to work around the policy from the build environment.
- **(CP3, 2026-07-16) The test-connection fixture is ~10 s, not the planned ~1 s.** hark-stt's verified `spike_clip.wav` (known transcript, existing Levenshtein check) was reused and embedded via a new `hark_stt::fixture` module rather than manufacturing a shorter speech asset on a machine with no recording path; a test pins the embedded bytes to the on-disk file. For Groq a test costs exactly the 10 s billing minimum the UI copy already discloses.
- **(CP3) `hark_pipeline::provider_config` went pub so Test connection builds the exact `ProviderConfig` the pipeline runs with**; a passing test validates the real path, not a parallel reimplementation. The cleanup test deliberately does NOT reuse the pipeline's fail-open `build_cleanup`: a test must report the failure, not silently degrade to verbatim.
- **(CP3) A stored key must restart the pipeline immediately.** Keys are resolved only at `run`; without the restart, dictation keeps failing against the old or missing key with no visible cause until the next Save. `KeySection::show` returns a keychain-changed signal for exactly this, gated on the draft provider matching the saved one.
- **(CP3) The theme radio persists via egui memory (eframe `persistence`), not config.toml, and `theme::apply` must re-apply the current preference instead of forcing System**, or every launch clobbers the restored choice. Unit-tested against a bare `egui::Context`; that eframe restores memory before app construction is verified from source but gets its real-hardware confirmation at CP6.
- **(CP3) `CollapsingHeader::open(Some(true))` for a single frame latches the header open** (openness persists in egui memory afterwards); that plus `default_open` is the entire auto-expand mechanism for Model & endpoint on openai-compatible.
- **(CP3) `egui::Modal` closes on backdrop click via `ModalResponse::should_close()`; Esc is handled explicitly inside the content closure.** Cancel takes focus on the modal's first frame (`request_focus`), making it the safe Enter default per §3.11.
- **(CP3) Get Started's success state needs a latch.** Once the key stores, "no key resolves" turns false, so a naive visibility condition hides the card before it can ever say "hold the chord and speak". `active` latches at startup (pipeline stopped, key-related) and only dismissal or the first injection clears it; on a passing test during onboarding the save-restart flow runs automatically so the success copy is true when shown.
- **(CP3) `hark_voice::Voice::from_str(voice_name.label())` bridges the config/voice parallel enums** without introducing a third match statement; the lowercase labels are the contract.
- **(CP3) Switching provider kind clears a typed model/base URL** (they almost certainly do not exist on the new provider); the per-kind default then shows as the field hint. "Empty = default" is the whole Option-field editing model.
- **(CP4, 2026-07-16) `ScrollArea::show_rows` assumes uniform row heights; the history list is deliberately heterogeneous** (day group headers, expandable rows), so the §3.7 "virtualize via show_rows" plan was replaced by windowed LIMIT queries (100 rows per window) rendered in a plain `ScrollArea` behind a "Show more" sentinel. The retention cap bounds the worst case; idle frames never touch the DB because the cache keys on (write generation, search, windows loaded).
- **(CP4) The §3.3 stats schema had no `total_ms` sum, but §3.7 wants "avg release-to-inject derived from sums".** Deriving it from `stt_ms + cleanup_ms` would silently omit encode and inject time, mislabeling the product metric. Migration 002 (`ALTER TABLE stats ADD COLUMN total_ms INTEGER NOT NULL DEFAULT 0`) closes the gap; pre-002 rows contribute 0, so the UI shows "n/a" until real totals accumulate rather than claiming a 0 ms average.
- **(CP4) The record policy (capture + retention) travels with the pipeline run, not in a shared cell.** Every settings path already restarts the pipeline, so `PipelineController::start` snapshots the policy into the event pump and the storage thread stays stateless; a `Prune` command sent on every start makes retention changes land at save/startup instead of at the next dictation.
- **(CP4) Cross-thread cache invalidation is one `Arc<AtomicU64>`:** the storage worker bumps it after each successful write and calls `request_repaint`; History/Stats re-query only when the generation (or their own query params) move. No polling, no dirty flags per page.
- **(CP4) `StorageHandle` joins its worker on Drop (after dropping its sender) so the last write commits before the process exits.** This is deadlock-free only because `HarkApp` declares `pipeline` before `storage`: field drop order stops the pipeline (whose event pump holds a storage sender clone) first. The DB-write-never-precedes-injection acceptance holds by construction: the pump tees only `Injected`, which the worker emits after injection completes.
- **(CP4) std Rust has no local-timezone calendar; jiff 0.2.32 provides it** (bundled tzdb on Windows, no registry parsing). Day grouping must compare civil dates, not 24-hour buckets: a dictation from two hours ago can be "Yesterday". Tests pin a fixed offset zone (`TimeZone::fixed`) so they never depend on the machine.
- **(CP4) Restartable fade affirmations ("Copied") are two `animate_value_with_time` calls:** snap to 1.0 with time 0.0 on the click, then decay toward 0.0 with the fade time on every later frame. Stale invisible state needs no cleanup; re-copying the same row just re-snaps.
- **(CP5, 2026-07-17) tray-icon 0.24.1 / muda 0.19.3 re-verified current at execution; one API landmine: `MenuEvent::set_event_handler` and `MenuEvent::receiver()` are mutually exclusive** (installing a handler disables channel delivery). The pump-thread pattern uses `receiver()`; never mix in a handler later. The §3.8 "drained each frame" plan was replaced by the pumps for exactly the reason the CP4 handoff predicted: a hidden, idle window paints no frames to drain anything.
- **(CP5) The global `MenuEvent`/`TrayIconEvent` receivers never disconnect,** so the pump threads park in `recv()` forever and die with the process (they hold only a `Context` clone and a sender; nothing joins them). Corollary: spawn them at most once per process, or every event double-delivers. `Tray::create` runs once, guarded by a `tray_failed` no-retry flag.
- **(CP5) Native `CheckMenuItem`s toggle themselves on every click, so a radio group is a reconciliation problem:** after handling a voice selection, unconditionally `set_checked` every item (a click on the already-selected voice would otherwise just uncheck it and the diff-based sync would see no change to fix).
- **(CP5) Launch-hidden works as `with_visible(false)` at build time plus the show decision in `HarkApp::new`** (`!pipeline.is_running()` = the window has something to say), which avoids duplicating key resolution in `main`. One explicit `request_repaint()` in `new` guarantees the first `logic` frame while hidden; that frame creates the tray and flushes the queued visibility command.
- **(CP5) Close-to-hide is gated on the tray existing.** A hidden window with no tray is an unreachable app, so tray-creation failure shows the window and leaves close = quit. Same reasoning gives Quit a `quitting` flag that lets the close request pass instead of being cancelled into a hide.
- **(CP5) Windows truncates tray tooltips at 127 characters mid-word;** the tooltip builder truncates to the cap with an ellipsis itself so failure details stay readable.
- **(CP6, 2026-07-17) Microphone enumeration for the input-device picker must run off the UI thread** (`hark_audio::list_input_devices`): WASAPI COM must not be initialized on the egui/main thread, so the device list is gathered at `SettingsPage::new` and re-scanned on an explicit "Rescan", never per frame. A configured-but-unplugged device stays selectable ("(not connected)") so opening the picker cannot silently reset the choice; capture falls back to the system default until it returns.
- **(CP6) Signed Windows releases via Azure Trusted Signing in GitHub Actions produced three durable gotchas, routed to LL-G** (2026-07-17; no `github-actions`/`signing` LL-G category existed): (1) the service is now branded "Artifact Signing" and `Azure/trusted-signing-action` redirects to `Azure/artifact-signing-action@v2`; (2) v2 reads the service-principal creds from its **`with:` inputs** (`azure-client-id`/`azure-tenant-id`/`azure-client-secret`), which it promotes to action-scoped env for `DefaultAzureCredential` — passing them via the step's `env:` is silently clobbered and auth fails "not fully configured"; (3) an empty/missing signing secret surfaces as an opaque `SignerSign() failed` deep inside signtool, so a preflight step that fails on any empty secret (lengths only, never values) is worth the lines. Full workflow: `.github/workflows/release.yml`.
