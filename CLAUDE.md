# Hark — Project Rules

Hark is an offline, single-user, **push-to-talk voice dictation desktop app** for Windows + macOS, written in **Rust**. Hold a key, speak, release; polished English text is injected at the cursor in any app. Transcription is on-device; history is local-only; cleanup is optional and uses the user's own LLM key.

> This is a **native desktop app**. There is no web frontend, server, database service, auth service, or hosting platform. The template reference `.claude/references/infrastructure.md` (Northflank/Cloudflare/Better Auth/Postgres/Redis) **does not apply to Hark** — ignore it.

## Stack

| Layer | Choice |
|---|---|
| Language / process model | Rust; single process, **UI on main thread, pipeline on worker threads** |
| Audio | `cpal` (16 kHz mono ring buffer, pre-roll + tail) |
| Push-to-talk | Native low-level key hooks: **CGEventTap (macOS), `WH_KEYBOARD_LL` (Windows)** — NOT the `global-hotkey` crate |
| STT | sherpa-onnx via the official **`sherpa-onnx` crate (v1.13.4+)** running **Parakeet TDT 0.6B v2 (English)** ONNX; NOT the deprecated `sherpa-rs` |
| Dictionary | Phonetic post-correction (primary) + hotword biasing (experimental — see gotchas) |
| Voices / cleanup | BYOK OpenAI-compatible endpoint (optional); one low-temp call |
| Injection | Clipboard paste (stash → set → paste → restore); `enigo` fallback |
| Tray + window | `tray-icon` + `eframe`/`egui` (native, no webview) |
| Storage | `rusqlite` (history + stats); TOML (settings + dictionary); `keyring` (BYOK key in OS keychain) |

Full rationale and phases: [`tasks/plan-repo.md`](tasks/plan-repo.md). UI/latency/accessibility SLA: [`.claude/references/design-guardrails.md`](.claude/references/design-guardrails.md). Tools: [`.claude/references/tools.md`](.claude/references/tools.md).

## The one hard rule: threading

- **macOS requires all UI on the main thread.** The main thread owns the tray + egui event loop; the dictation pipeline (hotkey, audio, STT, HTTP, injection) runs on **worker threads only**. Getting this wrong causes hangs that surface only on Mac.
- **Latency is the product.** All perceived latency is release-to-inject. Keep the model warm (load once + warmup inference at launch); Verbatim and skip-eligible short utterances never touch the network; history/stats writes happen after injection, off the hot path.

## Stack gotchas (verified 2026-07-15 — re-check before relying on them)

Full detail + citations: `.claude/agent-memory/explorer/hark_stt_stack_risk.md` and [`tasks/plan-repo.md`](tasks/plan-repo.md) §11. The load-bearing ones:

- **Hotword biasing on Parakeet TDT needs `modified_beam_search`, which has an open ~20% hallucination/empty-output bug (sherpa-onnx #3267).** Default to `greedy_search` + phonetic post-correction; gate decode-time biasing behind an experimental toggle. Re-check #3267 status before Phase 2.
- **No working Rust example of sherpa-onnx hotwords end-to-end** — spike it in Phase 1, don't assume C++/Python parity.
- **Verify CoreML / DirectML cargo feature flags in the `sherpa-onnx` crate source** before wiring GPU inference.
- **Windows tray binary has no console:** any console child process must set `CREATE_NO_WINDOW` (0x0800_0000) or a console flashes and steals focus (LL-G `kb/rust/gui-subsystem-console-child-window.md`).
- **If the BYOK HTTP call uses async,** keep blocking IO off the executor (`tokio::fs`/`spawn_blocking`) or use a blocking client on a worker thread (LL-G `kb/rust/blocking-io-on-tokio.md`).

## Coding standards

- Handle errors explicitly (`Result`/`?`); never swallow. Validate at boundaries (mic input, model output, BYOK responses, file/DB I/O).
- Avoid premature abstraction. Three similar lines beat a forced helper.
- Comment only the non-obvious "why", not self-explanatory code.
- Files over 500 lines should be split. Prefer editing existing files over creating new ones.
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` must pass. Run tests before declaring done (this machine is coding-only: build/test/lint here; run the app on real macOS/Windows).

## Hierarchical CLAUDE.md architecture

CLAUDE.md loads top-down: global user → this project file → subfolder. Only relevant files load.

- Root `CLAUDE.md` (this file) — project-wide rules, stack, threading rule, gotchas.
- Per-crate `crates/<crate>/CLAUDE.md` — only when a crate has distinct conventions (e.g. `hark-ui` egui/main-thread rules, `hark-stt` binding discipline). Create only once the crate exists.
- `.claude/rules/*.md` — path-scoped via `paths:` frontmatter; load only when matching files are touched (Rust source → `rust.md`; tests → `tests.md`).

Keep each file focused and under ~200 lines. Prune after model updates.

## Subagent usage

Aggressively offload research, doc fetching, log analysis, and codebase exploration to subagents to keep the main context narrow. **Always include a "why"** in every subagent prompt. Spin up parallel `explorer` agents for competing approaches. Use the custom `explorer` agent, never the built-in `Explore` type (it loads every MCP schema and blows the context window).

## Planning

- Planning is **phase-based**, not timeline-based: Foundation → Core → Polish → Ship.
- Plan in one session, execute in another. Save plans to `tasks/`.
- Every plan MUST end with a **Lessons Learned / Gotchas** section. After implementation, route discoveries to LL-G via `/add-lesson` — not to local files only.

## Context management

- Break tasks small enough to finish under 50% context. `/compact` proactively around 50%. Start fresh conversations for unrelated topics.
- Lock the tool list and model at session start to preserve the prompt cache.
- Use `/handoff` before ending a session; load it as sole context in the next.

## RULE 0 — Read-Only First (MANDATORY)

Gather information before acting. Read-only/diagnostic commands first; state-changing commands only with user approval; destructive operations never without explicit request. (BP `safety/read-only-first-rule`.)

## RULE 1 — Check LL-G Before Scripting (MANDATORY)

Before writing any code, automation, or scripts, fetch the LL-G index and load relevant entries:

1. Fetch `https://raw.githubusercontent.com/BoardPandas/LL-G/main/llms.txt`
2. For each technology you'll use (Rust, SQLite, Bash, Windows, WiX/MSI…), fetch its `kb/<tech>/llms.txt`
3. Read ALL HIGH-severity entries; read MEDIUM entries matching your task

Every plan's Lessons Learned section feeds back to LL-G. Lessons kept local stay local.

## RULE 3 — Check BP Before New Work / Config

When starting a feature or touching tooling/config, load the BP index and applicable practices:

1. Fetch `https://raw.githubusercontent.com/BoardPandas/BP/main/llms.txt`
2. Read the relevant concern indexes; load FOUNDATIONAL entries and RECOMMENDED entries matching this stack.

## Date awareness

Best practices and library versions must reflect the current date — verify with WebSearch, don't assume cached knowledge. Convert relative dates to absolute in saved plans.

## Skills & agents

Skill triggers and the agent registry are documented in [`instructions.md`](instructions.md) and [`agents.md`](agents.md). Key agents: `architect` (planning), `builder` (implementation), `explorer` (research), `reviewer`, `security`, `performance`, `tester`, `ux-reviewer`. Pre-commit changelog + version-bump discipline is enforced by `.claude/rules/commit-changelog.md`.
