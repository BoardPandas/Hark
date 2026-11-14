# Plan — improvement sweep (2026-11-14)

**Status:** implemented in 0.70.0 on 2026-11-14. Baseline `9fe1c25` (0.69.6).
Not run on real hardware before the release was cut.
**Authorization:** the owner reviewed a ranked list of improvements on 2026-11-14 and said
"do all your suggestions in one, two, and three", adding that any other file over 500 lines
worth splitting should be split too. That go-ahead stands in for a separate intent and spec.

## Scope

| # | Item | Outcome |
|---|---|---|
| 1.1 | Stalled live turn | Done. Gemini Live stall timeout 3 s → 1.5 s; on-device engine preloaded at pipeline start in Backup and Primary mode |
| 1.2 | Never lose words | Done. A paste failure keeps the finished text; a transcription failure keeps the last clip in memory for a retry from History |
| 1.3 | Batch outage wait | Done. Per-attempt batch timeout is 6 s + half the audio, capped at 15 s |
| 1.4 | Cleanup hang | Done. Cleanup timeout is 4 s + 5 ms per character, capped at 10 s |
| 2.1 | Stale docs | Done |
| 2.2 | Click-to-correct | Already shipped in 0.27.0 to 0.29.0 (the audit was wrong). Gaps against plan §2 closed instead |
| 2.3 | F-003 / F-007 | **Owner decision, not built** |
| 2.4 | `tasks/` tidy | Done |
| 2.5 | macOS hardware validation | **Needs a Mac; not possible on this machine** |
| 3.1 | File splits | Done for ten files; see below |
| 3.2 | CI | Done. `cargo deny` validated locally; the coverage job has never run |
| 3.3 | `CLAUDE.md` | Done |

## Decisions

- **OpenAI live keeps its 3 s stall timeout.** The 1.5 s figure rests on Gemini data (4 of
  1,902 good dictations exceeded 1.5 s release-to-inject). There is no such data for OpenAI.
- **Preload costs RAM.** Backup-mode users now hold the model in memory (about 1.2 GB resident) from
  startup instead of from the first rescue. No config knob; the Settings copy says so.
- **Timeout floors are generous on purpose.** OpenAI-compatible endpoints can be slow
  self-hosted servers, and LLM cleanup latency has a fat tail. A 3 s cleanup cap would
  intermittently inject uncleaned text, which is worse than a rare long wait.
- **Recovered text is never auto-pasted.** A retry runs from Hark's own window, so the cursor
  is not where the dictation was aimed. The text goes to History and a Copy button.
- **A retry gets one cloud attempt.** The worker refuses push-to-talk while a retry runs, so
  it must be short, and the pill says "Processing" for its length.
- **Failed audio stays in memory only.** One clip, never written to disk, never logged.
- **Undelivered rows are History-only.** They add nothing to stats or Insights: words that
  never reached the cursor saved no time.
- **Linux-only files are not split.** `hook_linux.rs` and `loopback/linux.rs` cannot be
  compiled on this machine, and Linux must keep compiling.
- **Lookup tables stay whole** (`keycode.rs`, `known.rs`), per the root `CLAUDE.md`.

## File splits

| File | Before | After | Seams |
|---|---|---|---|
| `hark-stt/src/openai_live.rs` | 2,589 | 352 | budget, wire, server_error, turn, resample, session, tests |
| `hark-stt/src/gemini_live.rs` | 1,752 | 274 | budget, mode, wire, session, tests |
| `hark-config/src/lib.rs` | 1,616 | 384 | one module per settings section, plus load_save |
| `hark-pipeline/src/worker.rs` | 1,591 | 611 | shape, held, engines, record, tests |
| `hark-meeting/src/detect.rs` | 1,279 | 417 | apps, loopback, tests |
| `hark-pipeline/src/meeting/coordinator.rs` | 1,253 | 809 | devices, recovery, tests |
| `hark-pipeline/src/lib.rs` | 1,198 | 351 | route, plan, tests |
| `hark-hotkey/src/edges.rs` | 1,129 | 328 | chord, tests |
| `hark-app/src/pipeline.rs` | 999 | 566 | tests |
| `hark-audio/src/mp3.rs` | 994 | 403 | decode, tests |

`hark-stt/src/live_transport.rs` (new, 52 lines) holds what the two live adapters had
byte-for-byte in common: the socket type, the turn driver, and the current-thread runtime
constructor. Each adapter still builds and owns its own runtime.

## Phases

### Foundation
- [x] LL-G (rust, github-actions) and BP indexes read.
- [x] Hot path, failure paths and open plans audited.

### Core
- [x] Wave 1: timeouts (hark-stt, hark-voice); preload and failure recovery (hark-pipeline,
      hark-store, hark-app); click-to-correct gaps (hark-app UI); docs, `tasks/` and CI.
- [x] Independent review of the failure-recovery code; two findings fixed (retry bounded to
      one cloud attempt and shown as busy; "Open History" wording true with capture off).
- [x] Wave 2: splits in hark-stt; hark-config + hark-hotkey; hark-meeting + hark-audio.
- [x] Wave 3: splits in hark-pipeline and `hark-app/src/pipeline.rs`.

### Polish
- [x] Mapped `Docs/` pages updated for every changed source.
- [x] `CLAUDE.md` file-size note, spellbook plan status, CHANGELOG and version.

### Ship
- [x] `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test --workspace --exclude hark-app`, hark-app tests with
      `--no-default-features --features live-stt`, `npm run check:claude`, `npm run check:docs`.
- [ ] Real-hardware checks the owner still has to run: the recovery banner and "Try again";
      drag selection on the collapsed History preview and return-to-place after an Add; a
      forced Gemini stall in Backup mode; first dictation after launch in Primary mode.
- [ ] First CI run of the new `deny` and `coverage` jobs.

## Lessons Learned / Gotchas

- **An audit that greps for a feature's name can miss the feature.** The plans audit reported
  click-to-correct as unbuilt because no code matched "Suggest" or "click to correct". It had
  shipped as "highlight-and-add" four months earlier. Search for the behavior (the Add button
  in the History row), not the plan's working title.
- **A request-level `reqwest` timeout replaces the client's, it does not combine with it.**
  Adding per-request timeouts to the batch adapters silently stretched the on-device
  fallback's short cloud leash back out, because that leash lived on a separately built
  client. The adapter has to be built with the same fixed budget.
- **Worktree-isolated agents cannot see uncommitted work.** A worktree is cut from HEAD, and
  the harness refuses to let an isolated agent read from or write to the main checkout.
  Either commit first, or use a non-isolated agent on dependency-disjoint crates.
- **Merging worktree copies by file needs line endings normalised.** With
  `core.autocrlf=true` the checkout is CRLF and `git show HEAD:path` is LF, so
  `git merge-file` reported the whole file as one conflict until all three sides were LF.
- **A new enum variant can leave a silent dead arm.** `NotPasted` replaced
  `Failed { stage: Inject }`; the old arms still compile and never fire.
- **A retry path that reuses the normal policy inherits its worst case.** The first cut gave
  a retry the full attempt-plus-retry-plus-fallback budget while the worker refused every
  push-to-talk press, with nothing on screen. Bound it, and say "Processing".
- **Moving a test module changes nothing a line count can see.** The count of files over 500
  lines stayed at 46 after ten splits, because the extracted test files are themselves long.
  Count source files separately from tests, or the number says nothing.
- **This machine cannot link `hark-app` tests with default features.** Only the Windows GNU
  toolchain is installed and sherpa-onnx ships MSVC libraries. Use
  `--no-default-features --features live-stt` for that crate.

Durable ones still to route to LL-G via `/add-lesson`: the `reqwest` timeout override, and
the `git merge-file` line-ending trap.
