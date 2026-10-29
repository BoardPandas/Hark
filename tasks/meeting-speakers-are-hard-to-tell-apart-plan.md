# Plan: who said what in a meeting transcript

- **Spec:** [../intent/meeting-speakers-are-hard-to-tell-apart/spec.md](../intent/meeting-speakers-are-hard-to-tell-apart/spec.md)
- **Intent:** [../intent/meeting-speakers-are-hard-to-tell-apart/intent.md](../intent/meeting-speakers-are-hard-to-tell-apart/intent.md)
- **Written:** 2026-10-29. Execute in a fresh session, one checkpoint per commit.
- **Version:** a Minor bump (new feature). Follow `.claude/rules/commit-changelog.md` for every commit: stage `CHANGELOG.md` and `package.json` *before* running the commit command.

Requirements live in the spec. This plan only says what to build, in what order, and how to prove it. Section numbers like "§2.6" refer to the spec.

## Before starting

- Re-read `CLAUDE.md`, `crates/hark-app/CLAUDE.md`, `.claude/references/design-guardrails.md` and `.claude/rules/{rust,tests}.md`.
- `rustup update stable`. A stale toolchain hides new clippy lints (`.claude/rules/rust.md`).
- `git status`. `ui/meetings/mod.rs` and the tray files had uncommitted edits from another session on 2026-10-29. Rebase onto whatever landed, and do not overwrite someone else's work.
- Check whether the theme plan's F2 has landed: `git log -S "enum Hue" -- crates/hark-app/src/theme.rs`. CP3 depends on the answer. The contract is in `intent/the-wizard-theme-feels-half-finished/spec.md` §3.3 and the F2 steps in `tasks/the-wizard-theme-feels-half-finished-plan.md`.

## 3. Implementation steps

### Foundation

**CP1: speaker identity and roster, no behavior change.**
1. `crates/hark-meeting/src/speakers.rs` (new; export it from `lib.rs`):
   - `SpeakerKey { Me, Them, Diarized(u32), Added(u32) }`, with `parse(&str) -> Option<Self>` and `as_str()` (§2.3), and `transcribed(channel, speaker)`.
   - `default_label(key)`. Move the Gemini window decode here from `export::speaker_label`, unchanged.
   - `Roster::new(segments: impl Iterator<(channel, speaker, override)>, names: &[(key, name)])` with `label`, `slot`, `people()` (menu order, merged by name), `effective`, and `notes_names()`. The last one returns `(key string, default label, display label)` triples for hark-voice.
   - Pattern: pure functions with an inline `#[cfg(test)]`, like `export.rs`.
2. `crates/hark-meeting/src/export.rs`: `speaker_label` delegates to `default_label`, so existing tests still pass. Add `slot: u8` to `ExportLine` and set it to 0 in fixtures.
- **Gotchas:**
  - Keep a single resolver. Every reader must go through `Roster`, or one will keep showing the old label (LL-G `architecture/clearing-legacy-field-exposes-old-readers`, `architecture/singular-field-vs-per-variant-array`).
  - Slots come from *transcribed* first appearance, never effective, or reassigning a line repaints people (spec §2.3).

**CP2: migration 007 and the store API.**
1. `crates/hark-store/migrations/007_meeting_speakers.sql`, exactly as in spec §4. Register it in `MIGRATIONS` (`hark-store/src/lib.rs:34`). It needs no Rust backfill hook.
2. `crates/hark-store/src/meetings.rs`:
   - `StoredSegment`, `SpeakerName` and `NameSource`, none deriving `Debug`.
   - `meeting()` selects `id`, `speaker_override` and `notes_stale_ms`, and reads `meeting_speaker_names`.
   - `set_speaker_name(id, key, name)`, which replaces `rename_meeting_speaker`, plus `apply_speaker_names(id, &[NamedSpeaker])`. The latter uses an upsert guarded by `WHERE meeting_speaker_names.source = 'guess'`, so user rows survive.
   - `assign_segment_speaker(id, segment_id, Option<&str>, now_ms) -> Result<Option<bool>, _>`. It runs in one transaction: update the override `WHERE id = ? AND meeting_id = ?`, and stamp `notes_stale_ms` only if `notes_json IS NOT NULL`. It returns `None` when the segment is not in that meeting.
   - `set_refreshed_notes(id, json, requested_ms)`. It clears the stamp only when the stamp is `<= requested_ms`.
   - `reprocess_meeting_segments(id, segments, baked_notes: Option<&str>)`. It clears `meeting_speaker_names` and the stamp in the existing transaction.
3. Update the store callers (`hark-app/src/storage/meetings.rs`, `ui/meetings/{detail,share}.rs`) to the new read types. They still render through `Roster`.
- **Gotchas:**
  - Key speakers by `speaker_key`, never by name (LL-G `sqlite/upsert-by-name-collision`).
  - Never renumber migrations (BP `database/never-renumber-applied-migrations`).
  - Do **not** drop `meeting_speakers`. Rollback depends on it (§11).
  - LL-G has no entry on SQLite table-copy migrations; this is a lesson candidate.

**CP3: speaker hues (no new color values).**
1. **If F2 has not landed,** land it first as its own commit, exactly as the theme plan's F2 describes. That means `theme/hues.rs`, `Palette.hues`, `Hue`/`Hue::ALL`, `hue`, `hue_wash`, and the values and tests from the theme spec §3.3. Do not add the Parchment row; that belongs to the theme work's F3. **If F2 has landed,** skip this step.
2. `crates/hark-app/src/theme.rs`: implement the reserved `pub fn speaker_hue(slot: usize) -> Hue` with the order from spec §2.4 (Me = Blue, then Gold, Violet, Teal, Rose, Green, with slots 6+ wrapping over 1–5).
3. `crates/hark-app/src/theme/tests.rs`: the four `speaker_hue` tests from spec §2.4, plus Light ink ≥ 4.5:1 on pure white.
- **Gotchas:**
  - hark-app is a binary crate, so an unused `pub` item trips `dead_code` under `-D warnings`. Either land `speaker_hue` together with its first caller (CP4), or use a scoped `#[allow(dead_code)]` with a one-line reason, the way `theme::icons` does, and remove it in CP4. The same trap is described in the theme plan's F2.
  - Do not tune or add hue values here. The theme spec owns them. If more than six hues are needed, raise it on the theme spec instead of forking the table.
  - Do not re-apply the theme preference on startup (LL-G `rust/eframe-set-theme-clobbers-persisted-preference`).

### Core

**CP4: speakers card, transcript reassign, storage commands.**
1. `crates/hark-app/src/storage/meetings.rs`:
   - Add `SetSpeakerName`, `SpeakerNames` and `AssignSpeaker` (which takes a `reply`, the same pattern as `Delete` at `:62`).
   - Remove `RenameSpeaker`.
   - Log ids and counts only.
2. `crates/hark-app/src/ui/meetings/speakers.rs` (new, ≤300 lines):
   - Move `speakers_card` out of `detail.rs`. List every key, including Me.
   - Swatch, rename, merge hint, and the guessed marker with its tooltip (spec §2.5).
   - Saving an unchanged guessed name sends `SetSpeakerName`, which flips the source to user.
3. `crates/hark-app/src/ui/meetings/transcript.rs` (new, ≤300 lines):
   - Move `transcript()` out of `detail.rs`. Add the 3 px rule, the chip button, the reassign popup, "New speaker…", and the moved-line icon.
   - Follow the popup pattern in `share::menu` (`share.rs:47`).
   - Give the menu's scroll area its own `id_salt`.
4. `crates/hark-app/src/ui/meetings/detail.rs`:
   - Build the `Roster` once per refresh, cached on the existing `(generation, id)` key.
   - Hold the pending `AssignSpeaker` reply, polled the same way as `deleting`, and show its error line.
   - Add `mod speakers; mod transcript;` to `ui/meetings/mod.rs`. Touch nothing else there.
5. Icons: look up the codepoint of a "guessed" glyph in the vendored Phosphor font (`assets/README.md`), and add it to `theme::icons` with the existing icon test. If it is absent, reuse an existing icon (spec A8).
- **Gotchas:**
  - Render every icon with `theme::icon_text` / `icon_label_job` (`crates/hark-app/CLAUDE.md`).
  - Keep dialogs in-window. A separate viewport brings the deferred-viewport bugs (LL-G `rust/egui-deferred-viewport-parent-pass-teardown`, `rust/egui-hidden-deferred-viewport-self-reveal`).
  - Siblings in the same scroll area need distinct `id_salt`s (LL-G `rust/egui-scrollarea-shared-id-salt`).
  - eframe is 0.35. Check snippets against `ui`/`logic` (LL-G `rust/eframe-034-app-trait-split`).
  - Do not latch popups or collapsibles open every frame (LL-G `rust/egui-collapsingheader-controlled-open-latch`).

**CP5: exports through the roster.**
1. `crates/hark-app/src/ui/meetings/share.rs` (`export_of`, `:100-135`): build `ExportLine.speaker` and `.slot` from `Roster` and the effective key.
2. `share/word.rs`: color the speaker run with the Light palette ink of `speaker_hue(slot)`. Read it through the same Light-palette table the hue tests cover, never a copied hex.
3. `share/excerpt.rs`: confirm it reads `ExportLine` only, and fix its fixtures.
- **Gotcha:** an override that some export ignores is a silent failure. Write the "moved line" test against every format (§9 T-EXP).

**CP6: notes carry speaker references.**
1. `crates/hark-voice/src/summary.rs`:
   - `SpeakerRefs { Tokens, Labels, Frozen }`, with serde default `Labels` and unknown values read as `Frozen`.
   - An always-appended `SPEAKER_REF_CLAUSE`, added after the template and before `JSON_OUTPUT_CLAUSE` in `build_summary_request` (`:205`).
   - `parse_notes` takes the call's label table and maps `{Label}` to `{{key}}`.
   - `MeetingNotes::render(&[(key, default, display)]) -> MeetingNotes` (spec §2.7 table).
   - `MeetingNotes::carry_done_from(&old)`.
   - `bake(names)`, which renders the notes and marks them `Frozen`.
2. `crates/hark-pipeline/src/meeting/finish.rs` `line()` (`:255`), `pending.rs:339` and `rerun.rs:39`: brace default labels, built from `default_label`. Keep the `[mm:ss]` shape.
3. `detail.rs` notes card: render through `Roster::notes_names()`. A checkbox writes `done` by index on the **stored** notes.
4. `share.rs:124`: export the rendered notes.
5. `storage/meetings.rs` `Notes`: resolve tokens in the title before `set_meeting_title`.
- **Gotchas:**
  - Rename is deterministic substitution, never an LLM edit (LL-G `llm-integration/recipe-context-does-not-preserve-unrequested-traits`).
  - Never write the rendered copy back. That would bake today's names into token notes, and later renames would stop working without any error.
  - Match default labels whole-phrase only: "Speaker 1" must not match inside "Speaker 12".
  - Never match names phonetically (LL-G `rust/phonetic-code-equality-needs-confirm-guard`).

**CP7: name guessing.**
1. `crates/hark-meeting/src/speakers.rs`: `guess_local(lines, guessable, your_name)` and the stoplist (spec §2.6). These are pure and table-tested.
2. `crates/hark-voice/src/speaker_names.rs` (new):
   - The request builder: numbered brace lines, guessable labels, no user names.
   - The prompt that treats the transcript as data.
   - `response_format: json_object`, a small `max_tokens`, and a 60 s timeout.
   - The parser reuses `extract_json_object`.
   - A shared `validate(guesses, lines, labels, your_name)`, which the local engine also calls.
3. `crates/hark-pipeline/src/meeting/speakers.rs` (new):
   - `name_speakers(settings, segments)` chooses the engine (spec §2.6) and maps labels to keys.
   - It adds the `your_name` row and emits `MeetingEvent::SpeakerNames`.
   - Call it from `finish.rs`, `pending.rs` and `rerun.rs`, after segments are known and before notes.
   - Reuse `finish.rs::summarize`'s provider resolution: extract a `notes_provider(settings) -> Result<SummaryConfig, _>` instead of copying it.
4. `crates/hark-app/src/meeting.rs` pump: route `SpeakerNames` to `MeetingCmd::SpeakerNames`.
5. `crates/hark-config/src/meeting.rs`: add `guess_speaker_names` and `your_name` with `#[serde(default)]`, and extend the round-trip test. `crates/hark-app/src/ui/settings/meetings.rs`: the two controls and the reworded notes checkbox (spec A5).
- **Gotchas:**
  - Every field in the guess prompt can steer it. Send default labels only, never user names or titles (LL-G `llm-integration/every-prompt-field-can-steer-a-classifier`).
  - The prompt cannot enforce JSON. Fail closed to the local rules (LL-G `llm-integration/prompt-instructions-cannot-require-a-tool-call`; BP `llm-resilience/llm-json-parse-retry-fail-closed`).
  - A reasoning model can return empty content. Treat that as failure (LL-G `llm-integration/reasoning-model-spiral`).
  - Anchor every guess on the deterministic "name in cited line" check (BP `llm-resilience/reconcile-llm-self-assessment-directionally`).
  - Size the output budget small and salvage truncated JSON (BP `llm-resilience/scale-output-budget-and-salvage-truncated-json`).
  - A user's confirmed names live in the DB and are never re-asked of the model (LL-G `llm-integration/full-chat-history-does-not-preserve-binding-workflow-decisions`).

**CP8: notes refresh after a reassignment, and the rerun bake.**
1. `crates/hark-pipeline/src/meeting/notes_refresh.rs` (new):
   - A worker shaped like `rerun.rs::spawn`. It summarizes from `NotesRequest`, then calls `carry_done_from`.
   - It emits `NotesRefreshStarted`, then `NotesRefreshed` or `NotesRefreshFailed`, where the failure reason is a slug.
2. `coordinator.rs`:
   - `Command::RefreshNotes` keeps per-id state `{running, queued: Option<NotesRequest>, discard}`.
   - A finished job whose id has a queued request is dropped, and the queued request starts.
   - A `Rerun` for the id sets `discard` and clears `queued`.
3. `crates/hark-app/src/meeting.rs`: `MeetingController::refresh_notes(id, request)` and `notes_status(id)`. The pump routes `NotesRefreshed` to `MeetingCmd::RefreshedNotes`.
4. `detail.rs`:
   - When an `AssignSpeaker` reply is `Ok(true)`, read the fresh detail from `storage.reader()`, build the `NotesRequest` (effective keys), and call `refresh_notes`.
   - Show the stale line and the **Update notes** button whenever `notes_stale_ms` is set and no refresh is running.
5. `storage/meetings.rs` `Reprocessed`: load the detail, build a `Roster`, `bake` the notes, and pass the result to `reprocess_meeting_segments`.
   - `rerun.rs`: after a successful replacement, call `name_speakers`. If notes are on, send a notes refresh from the new segments.
   - `detail.rs` `FinalPassAction`: the new confirm text (spec §2.7).
- **Gotchas:**
  - If a worker struct is joined on `Drop`, declare its sender field before the join handle (LL-G `rust/join-on-drop-sender-field-order`).
  - Wake the UI from the pump with `app::wake_ui`, never `request_repaint` (`crates/hark-app/CLAUDE.md`).
  - A result from before a rerun must never land after it. That is what `discard` is for, so test it.

### Polish

**CP9:**
- Speaker names in the notes card drawn in their color. Make `render` return spans, used by a `LayoutJob`; keep the text identical.
- The guessed tooltip shows the evidence time.
- A keyboard-only pass over the chips and menu.
- A Gemini fixture with 6 windows × 3 speakers, to check the card and menu stay usable.
- A long-meeting check: 3 h, about 2,500 lines. Measure the frame time of the detail view headlessly (`Context::run_ui`), as a one-off measurement and never a test assertion (`.claude/rules/tests.md`: no wall-clock asserts). If the transcript costs more than about 8 ms, render only the lines the viewport shows, using `ScrollArea::show_viewport` rather than `show_rows`, because rows are variable-height (LL-G `rust/egui-show-rows-uniform-height`).

### Ship

**CP10:**
- Update the meetings pages under `Docs/` (doc-sync markers).
- `CHANGELOG.md` Minor section and `package.json` version: user-facing entries for colors, renaming Me, moving lines, guessed names, notes that follow renames, and the two settings.
- Gates: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run --workspace` (or `cargo test --workspace`), and `npm run check:claude`.
- Native validation checklist for Windows first, then macOS:
  - Colors in all themes.
  - Menu keyboard navigation.
  - A real Deepgram meeting with introductions.
  - Word export opened in Word.

## 9. Test plan

Tests are inline `#[cfg(test)]` unless marked `tests/`. No test asserts on wall-clock timing.

| ID | Where | Cases | AC |
|---|---|---|---|
| T-KEY | `hark-meeting/speakers.rs` | Key parse/format round-trip, including Gemini high-bit ids. Unknown strings → `None`. `transcribed` for each channel/speaker combination. | AC2, AC21 |
| T-ROSTER | same | Label order (name row > default). Merge ignores case and spaces. Me is slot 0. Slots stay fixed after an override. Added keys come after transcribed keys. A merged key takes the earliest slot. Slots are not all equal (LL-G `architecture/derived-signal-goes-constant`). | AC8, AC9 |
| T-LOCAL | same | Table of lines: "Hi, this is Sam from Acme" → Sam. A single "Thanks, Sam" → nobody. Two "Thanks, Sam" lines after the same speaker → that speaker. A stoplist word is never a name. One name on two keys, the higher score wins and a tie names neither. `your_name` is excluded. | AC12, AC14 |
| T-MIG | `hark-store/tests/meetings.rs` | Build a 006-era file DB (pattern: `migration_004_applies_to_a_003_era_database_with_data_intact`, `tests/meetings.rs:410`). Seed renames, including a Gemini id. Open it. Assert `them:<n>` rows with source user, and `meeting_speakers` still present. Use `typeof()` checks (LL-G `sqlite/text-affinity-converts-integer-fixture`). | AC21 |
| T-STORE | `hark-store/tests/meetings.rs` | Set/clear a name. A guess never overwrites a user row, and does overwrite a guess. Assign with a segment from another meeting → `None`, nothing changes. Assign stamps stale only with notes. A refreshed write with an older `requested_ms` leaves the stamp. Reprocess clears names and the stamp and writes baked notes atomically: inject a failure and assert a full rollback. Cascade delete removes the new rows. | AC2, AC3, AC13, AC16, AC19 |
| T-THEME | `hark-app/theme/tests.rs` | `speaker_hue(0)` is Blue. No slot ≥ 1 is Blue. Slots 1–5 are the other five hues, once each. Slot 6 equals slot 1. Light ink is ≥ 4.5:1 on white. The theme spec's hue AA, wash and OKLab tests stay green across every palette. | AC5, AC6 |
| T-NOTES | `hark-voice/summary.rs` | `{Speaker 2}` → `{{them:1}}`. An unknown brace is stripped. Render tokens. Render legacy labels, where "Speaker 1" is not replaced inside "Speaker 12". `Frozen` stays untouched. Owner fields resolve. The speaker clause is appended to a custom template. `carry_done_from` keeps matched ticks and appends unmatched ticked items. An unknown `speaker_refs` value → `Frozen`. | AC15, AC17 |
| T-GUESS | `hark-voice/speaker_names.rs` | The request contains only default labels and numbered lines, never names: run the real builder. The parser handles fences and truncated JSON. Validation rejects: name not in the cited line, unknown label, 4 or more words, a stopword, `your_name`, and a label with two names. Accepts same-name merges. Empty content → error. Mock HTTP for 200, 500, timeout, and reasoning-only content. | AC10, AC11 |
| T-PIPE | `hark-pipeline/meeting/speakers.rs`, `coordinator.rs` | Engine choice for each setting/provider combination. LLM failure → local. Coalescing: five requests → at most two jobs, the last result written, the first discarded. `Rerun` discards an in-flight refresh. | AC10, AC12, AC16, AC19 |
| T-SW | `hark-app/storage/meetings.rs` | `AssignSpeaker` reply `Ok(true/false)`, and an error with rollback. `SpeakerNames` respects user rows. `Notes` resolves title tokens. `Reprocessed` bakes. | AC2, AC13, AC19 |
| T-EXP | `share.rs`, `share/word.rs`, `excerpt.rs`, `hark-meeting/export.rs` | One moved line and one renamed Me appear correctly in Markdown, text, SRT, Word XML (including the color run) and the excerpt. Notes in the exports are rendered. | AC1, AC4 |
| T-UI | `detail.rs`, `speakers.rs`, `transcript.rs` | Pure helpers: menu entries for a roster, and "As transcribed" shown only when moved. Reassigning to the transcribed key sends `None`. The stale-line text for each failure slug. A checkbox writes the stored notes, never the rendered copy. Headless `Context::run_ui` renders a fixture without panics across all palettes. | AC2, AC3, AC7, AC18, AC22 |
| T-LOG | grep in review + unit | New `Debug` impls print counts only. Review every new `log::` call: ids, counts, slugs only. | AC20 |

Unverifiable here and covered by the native checklist in CP10: on-screen color perception, OS keyboard focus rendering, live provider behavior, and how Word renders the colors.

## 11. Rollback

- **Code:** revert the checkpoint commits in reverse order. Each checkpoint compiles and passes on its own.
- **Database:** 007 is forward-only, but it adds without destroying. It creates a table, adds two nullable columns, and keeps `meeting_speakers`. An older build on a 007 database:
  - Skips the unknown migration (`migrate()` skips by `user_version`).
  - Reads `meeting_speakers`, so it sees renames as they were at upgrade.
  - Ignores overrides and the stale stamp.
- **Notes:** an older build ignores `speaker_refs`. Token notes show raw `{{them:1}}` text there. That is cosmetic, and the data is intact.
- **No manual DB steps.** Do not hand-edit `user_version`.
- **Later:** a future migration may drop `meeting_speakers` once a release with 007 has shipped and rollback is no longer needed.

## 12. Lessons Learned / Gotchas

Fill in during and after implementation. Default to LL-G and BP. Keep a lesson local only when it applies to this repo alone.

- [ ] Gotchas encountered → LL-G via `/add-lesson`. Candidates already known:
  - No LL-G entry covers SQLite table-copy migrations, or keeping the old table for rollback.
  - No LL-G entry covers prompt injection from transcript text in a name or summary prompt.
  - "Rendered copy written back over the stored notes" is a silent-failure shape worth recording if it bites.
- [ ] Proven reusable patterns → BP via `/add-practice`. Candidates:
  - Token references in LLM output for entities the user can rename later.
  - A deterministic evidence check on LLM extractions.
  - Per-key coalescing of background regenerations.
- [ ] Repo-only notes → `.claude/agent-memory/debugging.md` or `patterns.md`.
- [ ] Workflow improvements → `CLAUDE.md` "Things Claude Gets Wrong" if a mistake repeats.
- [ ] Failed approaches: record what was tried and why it failed.
