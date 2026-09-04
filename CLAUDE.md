# Hark — Project Rules

Hark is a single-user, **push-to-talk voice dictation desktop app** for Windows + macOS + Linux, written in **Rust**. Hold a key, speak, release; polished English text is injected at the cursor in any app. Transcription is **BYOK cloud by default** (the user's own STT provider key, multi-provider adapters) with an **opt-in on-device engine** alongside it; history, stats, and the spellbook are local-only; cleanup is optional and uses the user's own LLM key. (Pivoted away from on-device STT on 2026-07-15, then reintroduced it as an optional second engine in 0.18.0 — the cloud path remains primary. See `tasks/plan-repo.md`.)

> This is a **native desktop app**. There is no web frontend, server, database service, auth service, or hosting platform. `.claude/references/infrastructure.md` now says exactly that at the point of use — the template's Northflank/Cloudflare/Better Auth stack was removed from it, not just disclaimed here.

## Stack

| Layer | Choice |
|---|---|
| Language / process model | Rust; single process, **UI on main thread, pipeline on worker threads** |
| Audio | `cpal` (16 kHz mono ring buffer, pre-roll + tail) |
| Push-to-talk | Native low-level key hooks: **CGEventTap (macOS), `WH_KEYBOARD_LL` (Windows), `evdev` on `/dev/input` (Linux)** — NOT the `global-hotkey` crate, and NOT an X11 grab (invisible under Wayland) |
| STT (cloud, primary) | **BYOK cloud via an `SttProvider` trait**: OpenAI-compatible `/audio/transcriptions` adapter (OpenAI, Groq) + Deepgram nova-3 adapter (`keyterm` biasing) |
| STT (on-device, optional) | `hark-local-stt`: sherpa-onnx Parakeet, behind the `engine` feature — **on by default** via `hark-app`'s `local-engine`. Off/Backup/Primary per `[local_stt] mode`. Never on the cloud hot path unless selected |
| STT transport | `reqwest` 0.13 blocking + multipart + rustls on pipeline worker threads; one long-lived `Client`; **no global tokio runtime** |
| Spellbook | Phonetic post-correction (primary, provider-agnostic) + per-provider biasing (OpenAI/Groq `prompt`, Deepgram `keyterm`) |
| Invocations | Trigger phrase → canned text (`hark_spellbook::Expander`); same guarded matcher as the spellbook at a **0.90** confirm threshold vs 0.85. A fired invocation **must** skip cleanup — control flow, never a prompt clause |
| Voices / cleanup | BYOK OpenAI-compatible endpoint (optional); one low-temp call |
| Injection | Clipboard paste (stash → set → paste → restore); `enigo` fallback. **Linux picks its key-synthesis backend by session:** enigo/XTEST on X11, a `uinput` virtual keyboard on Wayland |
| Tray + window | `tray-icon` + `eframe`/`egui` (native, no webview) |
| Storage | `rusqlite` (history + stats); TOML (settings + spellbook); `keyring` (BYOK key in OS keychain / Secret Service on Linux) |

Full rationale and phases: [`tasks/plan-repo.md`](tasks/plan-repo.md). UI/latency/accessibility SLA: [`.claude/references/design-guardrails.md`](.claude/references/design-guardrails.md). Tools: [`.claude/references/tools.md`](.claude/references/tools.md).

## The one hard rule: threading

- **macOS requires all UI on the main thread.** The main thread owns the tray + egui event loop; the dictation pipeline (hotkey, audio, STT, HTTP, injection) runs on **worker threads only**. Getting this wrong causes hangs that surface only on Mac.
- **The Linux tray is the single, deliberate exception, and it is not a loophole.** `tray-icon` renders a Linux tray through libappindicator, i.e. GTK widgets, which are `!Send`, need `gtk::init()` on their own thread, and only do anything while a GTK main loop iterates — and winit already owns the main thread's loop. So `hark-app/src/tray/surface_gtk.rs` runs them on a dedicated thread. That is legal precisely because the main-thread rule is an **AppKit** rule: `surface_gtk.rs` never compiles on macOS, and nothing on Linux requires the tray to share the window's thread. Do not generalise this into "UI can go on worker threads".
- **Latency is the product.** All perceived latency is release-to-inject: WAV encode + one HTTPS POST to the STT provider + inject. Reuse one long-lived HTTP client (keep-alive + TLS resumption); at most one retry, on timeout only; history/stats writes happen after injection, off the hot path.

## Stack gotchas (verified 2026-07-15, re-check before relying on them)

Full detail + citations: `.claude/agent-memory/explorer/hark_cloud_stt_providers.md`, `hark_cloud_stt_rust_stack.md`, and [`tasks/plan-repo.md`](tasks/plan-repo.md) §11. The load-bearing ones:

- **OpenAI and Groq share the multipart `/audio/transcriptions` contract**; one adapter covers both. Deepgram is its own adapter (`/v1/listen`, `Token` auth, raw `audio/wav` body).
- **Groq bills a 10 s minimum per transcription request**; every short utterance costs as 10 s.
- **Deepgram `keyterm` (nova-3+, unweighted) and legacy weighted `keywords` (nova-2) are mutually exclusive by model generation.**
- **Transport is `reqwest` blocking on worker threads.** The `deepgram` crate is pre-1.0 and drags in full tokio (call its REST API directly instead); `ureq` multipart is unstable as of 3.3.0.
- **Never log API keys or raw audio**; errors must not echo Authorization headers or request bodies.
- **Windows tray binary has no console:** any console child process must set `CREATE_NO_WINDOW` (0x0800_0000) or a console flashes and steals focus (LL-G `kb/rust/gui-subsystem-console-child-window.md`).
- **On Linux there is no `LLKHF_INJECTED`.** A uinput device is indistinguishable from real hardware once its events reach `/dev/input`, so our own synthesized Ctrl+V would feed straight back into the chord tracker — on a Ctrl-containing chord, a dictation that re-triggers itself forever. The filter is the device *name*: `hark-inject` names its keyboard `Hark Virtual Keyboard`, `hark-hotkey` skips any device whose name starts with `Hark`. Both ends key off the app name so they cannot drift; each has a test pinning it.
- **`libappindicator` is `dlopen`ed, not linked** (libappindicator-sys uses `libloading`), so it never appears in the binary's `DT_NEEDED` and neither `dpkg-shlibdeps` nor rpm's `auto-req` can infer it. The `.deb`/`.rpm`/PKGBUILD name it explicitly; drop that and the packages still build, still install, and the tray silently never appears. `release.yml` gates on it.
- **Linux cannot swallow one key.** Suppressing a keystroke needs `EVIOCGRAB`, which takes the whole device exclusively and would stop every other key reaching the focused app. `[hotkey] swallow_lock_keys` is therefore inert on Linux and logs that it is; do not "fix" it with a grab.
- **Linux does not self-update.** The binary belongs to `dpkg`/`rpm`/`pacman`; overwriting it in place fails their integrity checks and is reverted by the next upgrade. `hark-update` checks and reports; the package manager installs.
- **If a future streaming adapter introduces tokio,** keep the runtime scoped to that adapter and blocking IO off the executor (LL-G `kb/rust/blocking-io-on-tokio.md`).

## Coding standards

- Handle errors explicitly (`Result`/`?`); never swallow. Validate at boundaries (mic input, model output, BYOK responses, file/DB I/O).
- Avoid premature abstraction. Three similar lines beat a forced helper.
- Comment only the non-obvious "why", not self-explanatory code.
- Prefer splitting a file once it passes ~500 lines **and there is a natural seam**. This is a smell to investigate, not a cap to trim to: 14 files are legitimately over it (largest `hark-config/src/lib.rs`, 1168), and lookup tables like `hark-hotkey/src/keycode.rs` and `known.rs` are exempt — splitting a table on an arbitrary line makes it worse. Never compress a file to duck the number.
- Prefer editing existing files over creating new ones.
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` must pass. Run tests before declaring done (this machine is coding-only: build/test/lint here; run the app on real macOS/Windows/Linux).

## Hierarchical CLAUDE.md architecture

CLAUDE.md loads top-down: global user → this project file → subfolder. Only relevant files load.

- Root `CLAUDE.md` (this file) — project-wide rules, stack, threading rule, gotchas.
- Per-crate `crates/<crate>/CLAUDE.md` — only when a crate has distinct conventions (e.g. `hark-app` egui/main-thread rules, `hark-stt` adapter/key-handling discipline). Create only once the crate exists.
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

## Commands

```bash
npm run check:claude
```

## Verifying your work

A change is not done until these are green:

```bash
npm run check:claude
```

Never skip, delete, or narrow a failing check to make it pass — the checks exist because
the defects they catch are otherwise silent. If a check is genuinely wrong, change it
deliberately and say so in the changelog.

## Things Claude Gets Wrong

Corrections that have been needed twice. Add to this list when a mistake repeats; move the
generalisable version to LL-G via `/add-lesson`.

- **Folding the changelog edit into the commit command.** The gate is a `PreToolUse` hook, so it runs *before* the command. `edit && git commit` cannot satisfy it. Stage first, commit second.
- **Writing `Tool(pattern)` as a hook `matcher:`.** That is permissions syntax; in a matcher it matches nothing and the hook silently never runs. Use `matcher: "Bash"` plus `if:` on the handler.
- **Reaching for `$CLAUDE_FILE_PATH` in a hook.** It does not exist, expands to `""`, and an empty path argument makes most tools walk the entire repo. Parse `tool_input.file_path` from stdin and hard-guard on non-empty.
- **Adding a `paths:` glob that matches nothing.** A rule scoped to a path that does not exist never fires and says nothing. The wiring guard fails the build on it.
- **Leaving a skill with no `model:` and no `agent:`.** It then runs on whatever model the session happens to be using. Declare one or bind the other.
- **Budgeting context by line count.** A line budget keeps passing while single lines grow to thousands of characters. Budget by bytes.

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
