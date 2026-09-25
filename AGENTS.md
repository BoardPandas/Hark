# Hark Agent Guide

This is the tool-neutral entry point for AI agents and new contributors. It
describes what Hark is, which documents are authoritative, and the invariants
that matter before changing code.

## Product contract

Hark is a single-user, system-wide push-to-talk dictation desktop app for
Windows, macOS, and Linux. Hold a configured chord, speak, release it, and Hark
injects polished English text at the cursor in the focused application.

Windows and Linux currently have end-to-end push-to-talk implementations.
macOS UI, tray, keychain, and injection paths exist, but its CGEventTap hotkey
hook is still a planned seam; current source returns `UnsupportedPlatform`.

- It is one native Rust process: tray daemon, native egui window, and worker
  threads. There is no Hark web service, account system, hosted database, or
  browser frontend.
- Cloud transcription is bring-your-own-key. Deepgram, OpenAI, Groq,
  OpenAI-compatible endpoints, and Gemini Live are supported.
- An optional on-device Parakeet engine can be Off, Backup, or Primary.
- History, stats, settings, invocations, and the spellbook are local. API keys
  live in the operating-system keychain.
- Cleanup is optional. Most providers use a separate OpenAI-compatible cleanup
  call; Gemini Live Smart mode can format the transcript in the transcription
  turn itself.
- Hark is English-first, latency-sensitive, and deliberately has no operated
  backend.

## Read order and authority

Read these before substantive work:

1. `AGENTS.md` — this fast orientation and the cross-tool rules.
2. `CLAUDE.md` — detailed repository policy, platform gotchas, artifact chain,
   and mandatory knowledge-base checks. Its rules apply even when the active
   agent is not Claude Code.
3. `README.md` — product behavior, installation, architecture sketch, privacy,
   and workspace map.
4. `Docs/README.md` — the documentation index. Use
   `Docs/_meta/GENERATION.md` and `Docs/_meta/SUMMARY.md` to determine the
   generation baseline and any acknowledged coverage gaps.
5. The relevant `crates/<crate>/CLAUDE.md`, when present, before editing that
   crate.
6. Current source and tests — the final authority when prose conflicts with
   implementation.

`tasks/` contains plans and handoffs. Treat it as design history unless a file
explicitly says it is active. `CHANGELOG.md` is the fastest way to discover
behavior added after a documentation baseline.

## Runtime architecture

The latency-critical path is:

```text
key down -> capture into ring buffer -> optional Gemini Live stream
key up   -> append tail -> assemble and gate clip
         -> finish live turn, or transcribe the finished clip
         -> on eligible cloud failure, optionally use local STT
         -> spellbook correction -> invocation expansion
         -> optional cleanup -> second spellbook correction
         -> inject at cursor -> persist history and stats
```

Important branches:

- Gemini Live starts a session on key-down and streams while the chord is held.
  If it cannot carry the dictation, Hark replays the preserved ring-buffer clip
  through the normal batch path.
- Primary on-device mode must not contact a cloud provider, including opening a
  speculative live session.
- A fired invocation replaces the transcript with its configured expansion
  verbatim. Provider-cleaned transcript text may help match the trigger, but
  the canned expansion must bypass every subsequent cleanup or rewrite step.
- At most one retry is allowed. A replay after a failed live stream consumes
  that retry budget.
- History and stats writes happen after injection, off the perceived hot path.

## Workspace map

The Cargo workspace has one application binary and focused library crates:

| Area | Crates | Responsibility |
|---|---|---|
| Shell | `hark-app`, `hark-single-instance` | Main-thread UI/tray, orchestration, one-process guard |
| Input | `hark-hotkey`, `hark-audio` | Native chord observation and continuous microphone capture |
| Recognition | `hark-stt`, `hark-local-stt` | Cloud adapters, Gemini Live streaming, optional Parakeet engine |
| Text | `hark-spellbook`, `hark-voice` | Correction, invocation matching, and optional cleanup |
| Output | `hark-inject` | Clipboard paste and platform key synthesis |
| Orchestration | `hark-pipeline` | Release-to-inject state machine, retries, fallbacks, reporting |
| State | `hark-config`, `hark-keychain`, `hark-store` | TOML settings, OS keychain access, SQLite history/stats |
| Desktop integration | `hark-autostart`, `hark-update` | Login startup and platform-appropriate update handoff |

The workspace membership in `Cargo.toml` is authoritative when crates are added
or removed.

## Hard invariants

- macOS UI and native tray work stay on the main thread. Linux's GTK tray thread
  is a platform-specific exception, not permission to move arbitrary UI work.
- Never block, allocate, lock, or perform a syscall in the cpal input callback.
- Never block the Windows low-level keyboard hook or stop its message pump.
- Observe hotkeys by default. Lock-key swallowing is a narrow, tested Windows
  exception; Linux never grabs an input device.
- Never log API keys, raw audio, transcript text, invocation phrases, expansion
  text, or cleanup prompts. Log counts, labels, durations, and status instead.
- Preserve the ring-buffer batch path when adding streaming optimizations. A
  failed optimization must not lose the user's dictation.
- Keep provider runtimes scoped to their adapters. Blocking HTTP remains on
  worker threads; the main thread never performs network or transcription work.
- Preserve clipboard restoration and the injected-event/device filters that
  prevent Hark from triggering its own push-to-talk chord.

## Change workflow

- Begin read-only: inspect the relevant docs, source, tests, and recent
  changelog entries before editing.
- For work larger than a narrow fix, follow the intent -> spec -> plan -> diff
  artifact chain documented in `CLAUDE.md` and `intent/README.md`.
- Prefer existing seams and files. Do not introduce a web stack, global async
  runtime, or cross-platform abstraction that erases required platform behavior.
- Update user-visible behavior in `CHANGELOG.md` under `Unreleased`. Version
  bumps are required before commits according to
  `.claude/rules/commit-changelog.md`; do not bump merely for an uncommitted
  working-tree change.
- Do not claim target-platform validation from this coding environment. Native
  microphone, hook, injection, tray, installer, and signing behavior require
  real Windows, macOS, or Linux validation as applicable.

## Verification

Run the narrowest relevant tests while iterating, then run the repository gates
before declaring a change complete:

```bash
npm run check:claude
npm run check:docs
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

If a full Rust gate is not appropriate for a documentation-only change, run the
two npm guards and explicitly report which Rust commands were not run.

## Documentation maintenance

- `Docs/` is the canonical case-sensitive docs root. Do not create `docs/`.
- `Docs/_toc.yaml` maps source files to generated pages. Generated sections are
  bounded by `BEGIN:AUTOGEN` and `END:AUTOGEN`; preserve material outside them.
- Update `Docs/_meta/GENERATION.md` and append a verified result to
  `Docs/_meta/SUMMARY.md` after a documentation sync.
- `npm run check:docs` compares source changes since the recorded baseline with
  the mapped page changes. A green result proves mapped pages changed with their
  sources; it does not prove the prose is exhaustive or correct.
- Keep `README.md`, `CLAUDE.md`, this file, and the overview aligned on product
  scope. Put deep feature detail in `Docs/features/`, not in always-loaded agent
  instructions.
