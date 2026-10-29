# Plan: Choosing a transcription setup

- **Spec:** ../intent/new-users-cannot-choose-a-transcription-setup/spec.md
- **Intent:** ../intent/new-users-cannot-choose-a-transcription-setup/intent.md
- **Baseline:** `2770475` (0.63.1). Written 2026-10-29.
- **Owner:** engineer. Requirements (R*), acceptance criteria (AC*) and copy live in the spec; this file
  says only how and in what order.

Tracks A (Settings and first run) and B (model check) share no files and can run in parallel
sessions. Each checkpoint below is one commit, sized well under half a session's context.

## Before starting

- `rustup update stable` (`.claude/rules/rust.md`: a stale toolchain misses new clippy lints).
- `git status`: another session may be editing `theme.rs`, onboarding or Meetings (wizard-theme and
  meeting-speakers intents). Do not stage `AGENTS.md`/`agents.md` or files you did not change. If the
  Roundtable file reservations are available, reserve `crates/hark-app/src/theme.rs` and
  `crates/hark-app/src/ui/settings/*` first.
- Re-read the spec §2.3-2.5. Copy comes from there verbatim; do not re-word it in code review.

## Foundation (Track A)

### CP1: helpers and pure decisions (no visible change)

1. `crates/hark-app/src/theme.rs`: add `pub const TIP_PADDING: i8 = 10;` beside the spacing tokens
   (`:103-112`) and `pub fn tip(ui, icon: &str, add_contents) -> InnerResponse` that draws a
   `Frame` with `fill(tint(v))`, `corner_radius(CONTROL_RADIUS)`, `inner_margin(TIP_PADDING)`, then
   `icon_text(icon)` coloured `accent` beside the contents. Pattern: `surface_frame`/`card` (`:188-203`).
   - Gotcha: render the icon with `theme::icon_text`, never `RichText::new(icons::X)` (crate
     `CLAUDE.md`: five Inter glyphs collide with Phosphor codepoints).
2. `crates/hark-app/src/theme/tests.rs`: add `tip_fill_is_the_contrast_tested_tint` asserting the
   frame's fill equals `tint(&v)` for every entry in `palettes()`. Expose a `tip_frame(v)` fn so the
   test needs no `Ui`.
3. New `crates/hark-app/src/ui/settings/provider_help.rs` (~90 lines): `pub struct ProviderHelp {
   line: &'static str, key_page: Option<&'static str> }` and `pub fn help(kind: ProviderKind) ->
   ProviderHelp`, filled from spec §2.5. `{model}` is substituted by the caller from
   `Provider::resolved_model()`. A module doc comment: "Verified 2026-10-29; re-check every URL and
   the Gemini data-use line before each release." Tests:
   - every `provider_choices()` kind has a non-empty line;
   - every `key_page` starts with `https://` and contains no `?`;
   - OpenAI-compatible has none;
   - the Groq line mentions the 10-second minimum (pins a known billing quirk).
4. New `crates/hark-app/src/ui/settings/stt_card.rs`, pure functions first (UI comes in CP2):
   - `mode_when_enabled(key_stored: bool) -> LocalMode` (R7).
   - `callout_visible(engine_available: bool, key: &KeyStatus, mode: LocalMode) -> bool` (R9). Pass
     availability in as a parameter: under `--no-default-features` `LocalEngine::is_available()` is
     always false, so a test that called it would only ever see one branch.
   - `use_on_device(draft: &mut Settings)` (R9): sets `Primary`, and `Verbatim` only when
     `resolve_cleanup_provider(...)` gives `Resolved` with `CleanupKeySource::ReuseSttKey`
     (`hark-pipeline/src/lib.rs:306-321` shows the resolution).
   - `privacy_line(mode)` and `mode_line(mode, fallback_after_ms)` (spec §2.4).
   - Unit tests for each, including: callout hidden when `Stored`, shown for `Backend`, hidden in
     Primary, hidden when the engine is unavailable; `use_on_device` leaves a voice with its own
     cleanup account untouched.
5. `crates/hark-local-stt/src/model.rs:113`: replace "Latest revision; 25 languages." with a comment
   that says it is the pinned multilingual build, that English-only v2 and `parakeet-unified-en-0.6b`
   (2026-04) exist, and points at the spec's Track B. Do not touch the pins.
6. Register both new modules in `ui/settings/mod.rs`. Run the CP checks (below). Commit.

## Core (Track A)

### CP2: the Speech to text card

1. `ui/settings/local.rs`: delete `section` (`:15-53`). Add `pub(super) fn on_device_block(ui,
   draft, download, key_stored)` (R4-R8): checkbox via a local `bool` derived from
   `mode.uses_local()`; on change, set `mode_when_enabled(key_stored)` or `Off`; radios for Primary
   and Backup; `mode_line`; `card(...)`. On any mode change call `download.refresh_status()` (keep
   the existing behaviour at `:45-47`). When off and `download.is_busy()`, draw the R8 progress line
   with Cancel, reusing `state_line`.
   - Gotcha: egui `checkbox` needs a `&mut bool`; do not keep a separate persisted bool, derive it
     from the draft every frame or the draft and the toggle drift.
2. `ui/settings/stt_card.rs`: `impl SettingsPage { pub(super) fn speech_to_text_card(&mut self, ui,
   saved, pipeline) }`. It takes over the body of the first `theme::card` in `dictation_section`
   (`sections.rs:157-173`) and adds, in spec order (R2, R3, R9-R11):
   - if not Primary: `form::provider_section`, the provider line, `stt_keys.show_with_link(...)`, the
     callout (when `callout_visible`), Test connection, `form::model_endpoint_section`;
   - if Primary: the one-line "cloud is off" note;
   - a `theme::fading_rule`, then `local::on_device_block`.
   - The callout's button calls `use_on_device(&mut self.draft)` then `download.refresh_status()`.
     Never call `download.start` from here (spec R9; a 670 MB transfer stays behind its own button).
   - Pattern: `onboarding.rs` already uses `impl SettingsPage` in a sibling module.
3. `ui/settings/sections.rs`: remove `Section::OnDevice` everywhere (`:17`, `:29`, `:41`, `:54`,
   `:125-127`), change `ALL` to `[Self; 7]`, make `dictation_section` call `speech_to_text_card`,
   and replace the fixed privacy label (`:187`) with `stt_card::privacy_line(self.draft.local_stt.mode)`.
4. `ui/settings/keys.rs`: add `show_with_link(&mut self, ui, link: Option<(&str, &str)>)`; `show`
   becomes `show_with_link(ui, None)` so the cleanup caller is unchanged. Draw `ui.hyperlink_to` on
   the status row only when the status is not `Stored` (spec A3). Extend `status_line` tests only if
   the copy changes.
5. `ui/settings/form.rs`:
   - `provider_section`: add `.on_hover_text(provider_help::help(kind).line…)` on each radio.
   - `model_endpoint_section`: the label from R12, `.id_salt("stt-model-endpoint")`, and the
     "Leave blank to use the recommended model." line. Keep `force_endpoint_open` exactly as is.
   - Gotcha: LL-G `egui-collapsingheader-controlled-open-latch`. `open(Some(true))` for one frame
     only, never every frame.
   - Gotcha: egui seeds a `CollapsingHeader` id from its label. Without `id_salt`, switching between
     "(optional)" and "(required)" resets the open state.
   - Net growth here must stay small; `form.rs` is already 531 lines. Anything larger goes in
     `stt_card.rs`.
6. `ui/settings/cleanup.rs` (R14): in the `account == stt_account` branch (`:96-104`) and the
   inherited `Resolved` branch, when `draft.local_stt.mode == Primary`, show a `KeySection` for the
   STT account instead of "Uses the key already stored…". After a store there, call
   `stt_keys.refresh()` so the hidden STT section's cached status does not go stale; this needs the
   STT section passed in or a returned "changed" flag handled in `sections.rs`. Watch the file
   size (282 lines today).
7. `ui/settings/mod.rs` `save` (`:240-255`): extract `fn save_notice(running: bool, primary: bool,
   model_ready: bool) -> String` (R15) and unit-test its three outcomes.
8. Run the CP checks. Commit.

### CP3: first run

1. `ui/settings/get_started.rs`: remove the `local` field. Keep `ready(local, model_ready,
   provider_passed)`; its test stays valid.
2. `ui/settings/onboarding.rs`:
   - `choose_engine` (`:124-175`): stop writing `get_started.local`; add the R17 `theme::tip` under
     the two options; add the size to the on-device detail line via `format_bytes`.
   - `configure_first_run` (`:177-228`): derive `let local = self.draft.local_stt.mode ==
     LocalMode::Primary;` and use it for the heading, the view, `ready(...)` and the hint (R18).
   - Gotcha: a click on the callout inside Configure changes the draft mid-frame; the view flips on
     the next frame. That is fine in immediate mode, but do not cache `local` across frames.
3. Run the CP checks. Commit.

## Polish (Track A)

### CP4: docs, review, size

1. Update "Settings → On-device" wording and the section table:
   - `README.md:206-208`;
   - `Docs/features/DESKTOP_UI.md:131-133` (drop the On-device row, extend Dictation) and `:152`
     (setup readiness now follows the selected mode);
   - `Docs/features/ON_DEVICE_STT.md:21` (cite `local.rs`/`stt_card.rs` by their new line ranges).
   Keep AUTOGEN markers intact; run `npm run check:docs`.
2. `wc -l crates/hark-app/src/ui/settings/*.rs`: every touched UI module at or under ~300 except
   `form.rs` (pre-existing 531; must not grow by more than ~10).
3. Run the `ux-reviewer` agent on `stt_card.rs`, `local.rs`, `onboarding.rs` with the spec's
   wireframes (§2.2). Why: BP `laws-of-ux-code-review` is FOUNDATIONAL and this change is all UI.
   Fix Important findings only (`REVIEW.md` definition).
4. Run the CP checks. Commit.

## Track B: on-device model check

Runs independently of Track A. **Two approval gates belong to the user: installing a Python package
and downloading any model file.** Ask before each, stating name, source and size.

### E1: tool and corpus instructions (no downloads)

1. New `tools/parakeet-ab/` (standalone, outside the Cargo workspace; pattern:
   `tools/meeting-aec-bakeoff/`):
   - `README.md`: how to record the corpus (spec §3), where to keep it (outside the repo, e.g.
     `%USERPROFILE%\hark-ab\clips\`), the `references.tsv` format (`file<TAB>text`), and how to run.
   - `requirements.txt` pinning `sherpa-onnx` to the version in `Cargo.lock` (1.13.8 on
     2026-10-29; re-read the lockfile, do not copy this number) and `numpy`.
   - `ab.py`: for each model dir, build `sherpa_onnx.OfflineRecognizer.from_transducer(...,
     model_type="nemo_transducer", decoding_method="greedy_search", num_threads=2)`, decode every
     WAV, record load time and per-clip decode time, write `hyp-<model>.tsv`.
     Verify the `from_transducer` signature against the installed wheel's docstring before relying
     on it; the explorer did not fetch it.
   - `score.py`: stdlib-only Levenshtein WER (normalized and formatted), Spellbook/proper-noun recall
     (terms read from the user's `spellbook.toml` path argument), 10,000-sample paired bootstrap
     over clips, and a markdown summary with a pass/fail line per spec §3 criterion.
   - Unit tests for `score.py` on hand-made strings (`python -I -m unittest`).
2. Commit the tool (no clips, no transcripts, no models).

### E2: run (gated)

1. **Gate:** product owner answers spec Q1. Drop any candidate they reject.
2. **Gate:** ask before `pip install -r requirements.txt` into a fresh venv.
3. **Gate:** ask before downloading each model. Sources are the `csukuangfj/sherpa-onnx-nemo-*`
   Hugging Face repos that `model.rs` already uses for v3 (the existing v3 download in the user's
   data dir can be reused). Put each in its own empty directory, record the sha256 of every file,
   and compare v3's against `model.rs:114-143`.
4. Run `ab.py` then `score.py` with `python -I`, on the same machine for all arms.
5. Write `tools/parakeet-ab/RESULTS.md`: the date, machine, sherpa-onnx version, corpus size
   (clips, words), every metric per model, and the verdict per criterion. Aggregates only; no
   transcript text. Show the owner the per-clip extremes locally.
6. Commit `RESULTS.md`.

### E3: decision

1. The product owner reads `RESULTS.md` and decides. Record it, dated, in `intent.md` Decisions.
2. Keep v3 (the default): nothing else changes.
3. Switch: open a new plan (`/spec-developer` or plan mode). It covers a new pinned `ModelSpec`, the
   licence string, `DEFAULT_MODEL`, keeping v3 in `CATALOG` for existing configs, how current users
   move (never automatically), and the docs. It is out of scope here.

## Ship (Track A)

### CP5: release

1. Re-verify spec §2.5: open each key URL (OpenAI's was not loaded at spec time) and the Gemini
   pricing page's free-tier data-use statement. Update the copy table date.
2. `CHANGELOG.md`: a `## [x.y.0] - YYYY-MM-DD` section (Minor: new UI capability). Bump
   `package.json`. Stage both, then commit in a separate step (`.claude/rules/commit-changelog.md`;
   the gate runs before the command, so `edit && git commit` cannot pass).
3. Hardware validation (this machine cannot run the app), Windows first, then macOS; Linux compile
   only (`memory: platform-priority`):
   - Tab through the whole Speech to text card in all three modes; every control operable by
     keyboard, focus ring visible.
   - Click each Get a key link: browser opens, no console flash.
   - Fresh profile, no key: callout → Use on-device instead → Download → Save → dictate.
   - First run: choose cloud, flip to Primary in Configure, confirm the view and gate switch.
   - Toggle off mid-download: progress line and Cancel visible.
   - Light, Dark, Solarized Light/Dark: tip legible.

## Checks per checkpoint

This machine's GNU toolchain cannot link sherpa-onnx (LL-G `sherpa-onnx-windows-gnu-no-static-lib`),
and MSVC is not installed. Record which command ran on which toolchain; never report a partial run as
"all tests pass".

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --all-targets -- -D warnings
```

```bash
cargo test --workspace --exclude hark-app
```

```bash
cargo test -p hark-app --no-default-features
```

```bash
npm run check:claude
```

```bash
npm run check:docs
```

`clippy` does not link, so it covers default features here. CI's MSVC and Linux runners cover the
default-feature test run; read their result before release.

## Test plan

| Test | File | Covers |
|---|---|---|
| tip fill equals `tint` in every palette | `theme/tests.rs` | AC13 |
| provider lines present; key URLs https, no query; none for OpenAI-compatible; Groq minimum | `provider_help.rs` | AC6 |
| `mode_when_enabled` both inputs | `stt_card.rs` | AC3 |
| `callout_visible` truth table (available × Missing/Stored/Backend × Off/Fallback/Primary) | `stt_card.rs` | AC2, AC5 |
| `use_on_device`: reuse-key voice → Verbatim; own-account voice untouched; never touches download | `stt_card.rs` | AC5 |
| `privacy_line`/`mode_line` per mode, timeout from config | `stt_card.rs` | AC8 |
| `save_notice` three outcomes | `mod.rs` | AC9 |
| `Section::ALL` length 7 and unique scroll ids | `sections.rs` (existing test, extend) | AC1 |
| `ready()` per engine (existing) | `get_started.rs` | AC11 |
| `score.py` WER, recall, bootstrap on fixed strings | `tools/parakeet-ab` | AC16 |
| Manual hardware checklist (CP5 step 3) | — | AC4, AC7, AC10-AC12, AC14 |

UI layout itself has no harness here (no `egui_kittest` in `hark-app` dev-dependencies); the pure
functions carry the logic so the manual pass only checks rendering and input.

## Rollback

Track A: revert CP1-CP4 (one commit each, in reverse). No config, schema or keychain change to undo.
Track B: delete `tools/parakeet-ab/`; nothing in the app references it.

## Risks

- **Concurrent edits.** The wizard-theme session is changing `theme.rs` and may restyle first run.
  Rebase before CP1 and CP3; if it adds a Parchment palette, make sure it is in `palettes()` so the
  tip's contrast is checked there too.
- **`cleanup.rs` coupling (CP2 step 6).** Two `KeySection`s can point at one account; the hidden one
  must be refreshed after the visible one stores, or the callout keeps showing after a key exists.
- **Copy rot.** Provider URLs and Gemini's data-use terms change without a Hark release (spec §8b).
- **New providers.** Another session is planning an OpenAI live provider (`tasks/2026-10-29-plan-openai-live.md`).
  The `provider_help` test over `provider_choices()` fails until any new kind gets a line and key
  page, which is the intended guard; do not weaken it to pass.

## Lessons Learned / Gotchas

Known before starting (from LL-G and exploration):

- LL-G `sherpa-onnx-windows-gnu-no-static-lib`: `check`/`clippy` pass and `test`/`build` fail to
  link on the GNU target; keep engine logic out of tests that run here.
- LL-G `egui-collapsingheader-controlled-open-latch`: one-shot `open(Some(true))`, never per frame.
- egui derives a `CollapsingHeader` id from its label (`IdSalt::new(text.text())`, egui 0.36.2
  `containers/collapsing_header.rs:396-398`; `Cargo.lock` pins 0.36.2); a conditional label needs
  `id_salt`.
- `LocalEngine::is_available()` is compile-time false under `--no-default-features`; pass it in.
- Dictation history keeps no audio, so a real-dictation A/B needs a separately recorded corpus.
- Hark's sherpa-onnx is 1.13.8 in `Cargo.lock`, not the 1.13.4 that `tasks/plan-local-stt.md` records.
- The `parakeet-unified-en` licence is NVIDIA Open Model License, not CC-BY-4.0; the card's licence
  line is per `ModelSpec`, so a switch must change it.

After implementation:

- [ ] Gotchas encountered → LL-G via `/add-lesson` (candidate: egui `CollapsingHeader` id follows its
      label text, so renaming or conditionally labelling a header silently resets its open state).
- [ ] Reusable patterns → BP via `/add-practice` (candidate: pass platform/feature availability into
      UI predicates so `--no-default-features` tests cover both branches).
- [ ] Repo-only notes → `.claude/agent-memory/patterns.md`.
- [ ] Failed approaches → record here with why.
- [ ] Copy the Track B verdict and its numbers into the intent's Lessons Learned section.
