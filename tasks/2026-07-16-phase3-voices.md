# Phase 3: Voice layer + cleanup BYOK

**Date:** 2026-07-16. **Status:** PLANNED (execution in a later session). **Prereq:** Phase 2 complete (main @ `6919af6`, 170 tests green, CP6 user-validated on real Windows hardware).
**Master plan:** `tasks/plan-repo.md` §8 Phase 3 (lines 143-146) and §9 (config/secrets table).

## 1. Goal

After STT and dictionary correction, an optional single low-temperature chat-completions call rewrites the transcript in the user's chosen **voice** before injection: **Verbatim** (no call at all), **Clean** (default: fix punctuation, casing, fillers, false starts; preserve wording and tone), **Professional**, **Casual**, or **Custom** (user-supplied prompt). The cleanup provider is BYOK and may be the same provider+key as STT (both OpenAI) or different (Deepgram STT + OpenAI cleanup); config models this explicitly. Latency stays the product: a word-count gate skips cleanup for short send-ready utterances, Verbatim never calls, and cleanup failures never block injection.

Decided with the user (2026-07-16):

- **No tray in Phase 3.** Voice selection is a `[voice]` config key plus a `hark-cli --voice` override; all tray/egui work stays in Phase 4.
- **Length gate is word-count based, default 5**: corrected transcripts with fewer than 5 words skip the cleanup call (STT always runs). Configurable; 0 disables the gate.

Out of scope: tray/UI (Phase 4), streaming cleanup, multiple simultaneous voices, per-app voice rules, history/stats of cleanup results (Phase 4 storage).

## 2. Design

### 2.1 New crate: `hark-voice`

Voices, prompt assembly, the length gate, and the one chat adapter. Mirrors `hark-stt`'s structure and discipline: I/O-thin blocking adapter, pure testable request/response functions, an error taxonomy that can never carry key material, `reqwest` 0.13 blocking on the worker thread, no tokio.

Only one adapter exists: the OpenAI-compatible `POST {base_url}/chat/completions` contract covers OpenAI, Groq, and any compatible endpoint. Deepgram has no chat product and is rejected as a cleanup provider kind at validation.

Dependencies: identical to `hark-stt` (`reqwest` with `blocking`/`rustls`/`webpki-roots`/`json`, `serde`, `serde_json`, `thiserror`, `log`). Copy the TLS feature flags from `crates/hark-stt/Cargo.toml` verbatim; reqwest 0.13 renamed the 0.12 umbrella feature (LL-G `reqwest-013-tls-feature-rename`).

Public API sketch:

```rust
pub enum Voice { Verbatim, Clean, Professional, Casual, Custom }

/// A configured cleanup adapter (the analog of SttProvider; a trait so the
/// worker tests can script it like MockProvider).
pub trait CleanupProvider: Send {
    /// Blocking; called from the pipeline worker thread. Returns the
    /// rewritten text plus wall time. Never logs keys or text.
    fn clean(&self, text: &str) -> Result<Cleaned, CleanupError>;
    fn label(&self) -> &str;
}

pub struct Cleaned { pub text: String, pub request_ms: u128 }

/// Word-count gate: true when `text` has fewer than `min_words` words
/// (min_words == 0 disables the gate). Pure.
pub fn skips_cleanup(text: &str, min_words: u32) -> bool;
```

### 2.2 Voices and prompt assembly

Each non-Verbatim voice maps to a system prompt template. Exact wording is tuned at CP0/CP5; the required shape:

1. The voice instruction (Clean: fix punctuation, capitalization, fillers, false starts, repeated words; preserve wording, meaning, and tone; never add or remove content. Professional: polished business register. Casual: relaxed conversational register. Custom: the user's `custom_prompt` verbatim).
2. A protected-terms clause: "Leave these terms exactly as written: ..." built from dictionary terms, but **only terms actually present in the outgoing text** (case-insensitive containment). This keeps the prompt tiny for the common case and is capped by the same chars/4 token heuristic as `prompt_from_bias_terms` (budget ~400 tokens; order is the user's priority signal, same drop rule).
3. A closing instruction: return only the rewritten text, no commentary, no quotes.

The transcript rides as the single user message. Prompt assembly is a pure function (`system_prompt(voice, custom_prompt, present_terms) -> String`) with unit tests. Dictionary terms are user content: they may enter the request body (they already ride STT biasing) but never logs.

### 2.3 Chat adapter

Request (JSON body, buffered `Vec<u8>` via serde, `Content-Type: application/json`, Bearer auth):

- `model`, `messages` (system + user), `max_completion_tokens` (both providers have converged on this name; Groq deprecates `max_tokens`).
- `temperature`: **only serialized when configured** (`Option<f32>`, `skip_serializing_if`). OpenAI's GPT-5 family rejects any non-default temperature with a 400; Groq accepts 0-2. Presets set it per provider (below).
- `reasoning_effort`: **only serialized when configured** (`Option<String>`). OpenAI GPT-5 family only; "minimal" is OpenAI's own guidance for short deterministic rewrites. Groq preset leaves it unset.
- `max_completion_tokens` derives from input length via a pure function: estimate input tokens as chars/4, allow `2 * estimate + 256`, clamped to [512, 4096]. The floor is deliberately generous: reasoning models spend reasoning tokens from this budget, and a too-tight cap yields an empty `content` with `finish_reason: "length"` (verify headroom at CP0).

Response: `choices[0].message.content` via a pure `parse_response(provider, body)`; trim the result; empty content is a `CleanupError::Provider` (which the pipeline treats as fail-open, below).

Errors: `CleanupError` in `hark-voice`, mirroring `SttError`'s taxonomy and hygiene exactly (Http / Auth / RateLimited / Timeout / Provider; body snippets truncated; no variant can carry a key; no Debug on the config struct). A shared error crate between hark-stt and hark-voice is deliberate non-work: two small parallel enums beat a premature abstraction, and the pipeline maps both into log lines anyway.

Transport: clone `hark_stt::shared_client()` (one client per process is the rule; `Client` is an Arc internally) and set a **per-request timeout** of `CLEANUP_TIMEOUT_MS = 10_000` via `RequestBuilder::timeout`, tighter than STT's 15 s total. Rate-limit headers are identical across both providers (`x-ratelimit-*`), so the `Retry-After` parsing pattern carries over unchanged.

**No retry for cleanup.** STT retries once on timeout because failure means the dictation is lost; cleanup failure has a graceful fallback (inject the uncleaned text), so a retry only doubles worst-case hot-path latency for marginal benefit. This is a deliberate divergence from the STT policy, inside the "at most one retry" cap.

### 2.4 Config schema (hark-config)

Additive only; the existing `#[serde(default)]` + explicit `Default` + unknown-keys-tolerated pattern already covers it. No renames or retirements, so no version-stamped migration machinery this phase (conscious decision against BP `versioned-config-migration-backup`; it becomes mandatory the first time a breaking config change ships).

```toml
[voice]
default = "clean"          # verbatim | clean | professional | casual | custom
custom_prompt = ""         # required (validated) when default = "custom"
skip_below_words = 5       # gate: fewer words than this skips cleanup; 0 disables

[voice.provider]           # optional table; omit to inherit (see resolution rules)
kind = "openai"            # openai | groq | openai-compatible (deepgram invalid here)
base_url = "..."           # optional; defaults per kind; required for openai-compatible
model = "..."              # optional; defaults per kind (pinned at CP0)
temperature = 0.2          # optional; omitted from the request when absent
reasoning_effort = "minimal"  # optional; omitted when absent
key_account = "..."        # optional keychain account override (edge: two distinct
                           # openai-compatible endpoints would otherwise share a slot)
```

Provider resolution at pipeline build (pure function, unit-tested):

1. Explicit `[voice.provider]` wins.
2. Absent: if the STT provider kind is `openai` or `groq`, cleanup **inherits** it (same kind and base_url, the kind's default chat model, the kind's preset temperature/effort) and **reuses the already-resolved STT key** (the "share one provider+key" case; no second keychain read). An `openai-compatible` STT endpoint is **not** inherited: speaking `/audio/transcriptions` does not imply `/chat/completions`.
3. Still unresolved (Deepgram STT, or openai-compatible STT) while the effective voice is not Verbatim: **log one warning at startup and run Verbatim.** Defaults must keep working out of the box (default config is Deepgram STT + Clean voice); a hard error here would break `Settings::load` on a missing file and the first-run path.

Validation additions: `voice.default = "custom"` requires a non-empty `custom_prompt`; `voice.provider.kind = "deepgram"` is invalid; `voice.provider.kind = "openai-compatible"` requires `base_url` (same rule as STT).

Per-kind chat defaults (pinned by CP0 measurements; research baseline 2026-07-16, full citations in `.claude/agent-memory/explorer/hark_llm_cleanup_chat_models.md`):

| Kind | Default model | Preset temperature | Preset reasoning_effort | Pricing (in/out per M) |
|---|---|---|---|---|
| `openai` | `gpt-5-nano` (candidate; `gpt-4.1-mini` fallback if nano's rewrite quality or latency disappoints) | unset (GPT-5 family locks it) | `"minimal"` | $0.05 / $0.40 (nano) |
| `groq` | `llama-3.1-8b-instant` (candidate; `openai/gpt-oss-20b` upgrade) | 0.2 | unset | $0.05 / $0.08 |
| `openai-compatible` | none (explicit) | unset | unset | n/a |

### 2.5 Keychain (hark-keychain)

- New env override `HARK_CLEANUP_KEY` for the cleanup role; generalize the resolver (`resolve_key_for(env_var, account)`) and keep `resolve_key` as the STT wrapper so existing call sites and the label-stability test stand.
- **Account naming decision:** the keychain account stays the provider label, shared between STT and cleanup roles by design (one key per provider; the inherit path never touches the keychain twice). `key_account` in config covers the only real collision (two distinct openai-compatible endpoints). No dual-read key rotation (BP `dual-read-key-rotation` considered and declined: single-user local app, simple keyring overwrite is the whole story). Recorded here so it is a conscious decision, not a default.
- Cleanup key resolution failure (missing key, backend error) while a non-Verbatim voice is configured: same fail-open as provider resolution, warn once and run Verbatim. STT keeps working; the app never refuses to start over the optional feature.

### 2.6 Pipeline integration (hark-pipeline)

`Worker` gains `cleanup: Option<CleanupPlan>` (the boxed `CleanupProvider`, effective `Voice`, gate threshold). `dictate()` chain becomes:

```
transcribe_with_retry -> empty check -> corrected_text (dictionary pass 1)
  -> gate: fewer than skip_below_words words? inject as-is
  -> cleanup.clean(text): Ok -> corrected_text (dictionary pass 2) -> inject
                          Err -> log warn, inject the pass-1 text (fail-open)
```

**Ordering decision (dictionary both before and after cleanup):** pass 1 gives the model canonical spellings and makes the protected-terms clause match the actual text; pass 2 repairs any term the model re-mangles anyway. Phase 2 measured the pass at microseconds to low single-digit ms, so running it twice is free against a 100+ ms HTTPS round trip; the handoff's "both is a latency cost" concern dissolves on the numbers. Pass 2 runs only when cleanup actually ran.

- **Pre-warm:** the worker already pre-warms the STT base URL; when the cleanup host differs, fire a second pre-warm GET at the cleanup base URL (same fire-and-forget pattern, failures only cost warmth).
- **Logging discipline (unchanged):** counts, millis, and config labels only. `cleanup: voice=clean model=<label> {in}->{out} chars, request {n} ms`; `cleanup skipped (short utterance: N words)`; `cleanup failed ({e}); injecting uncleaned transcript`. Never transcript or cleaned text, never prompts, never terms.
- `hark-cli` gains a `--voice <name>` argument (hand-rolled `std::env::args` parse; no clap for one flag) that overrides `voice.default` for the run; invalid names exit with the valid list.

## 3. Checkpoints

One commit per checkpoint. `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, full tests green at every CP. Patch bump per CP; **CP4 (feature-activating) takes the Minor**. CHANGELOG.md entry + package.json bump every commit per `.claude/rules/commit-changelog.md`.

### CP0: crate scaffold + pure functions + cleanup model spike
- `crates/hark-voice` in the workspace; deps copied from hark-stt (TLS feature names verbatim).
- Pure layer with tests: `chat_completions_url` (trailing-slash tolerant), request-body builder (temperature/effort omission when None), `max_completion_tokens` derivation (floor/cap/headroom), `parse_response` (happy, missing-choices, empty-content, junk), `CleanupError` taxonomy + `error_for_status`/`error_for_transport` analogs.
- Spike example (`cargo run --example cleanup_spike -p hark-voice`, keys from env, precedent: Phase 1 `transcribe_spike`): run fixture transcripts (short/medium/long, fillers, dictionary terms) against `gpt-5-nano` (minimal effort), `gpt-4.1-mini`, `llama-3.1-8b-instant`, `openai/gpt-oss-20b`. Measure p50/p95 warm request time (N=10+), eyeball rewrite quality per voice prompt, and empirically verify: GPT-5 temperature rejection (expected 400), `reasoning_effort` acceptance on nano (community reports are inconsistent), the `{"error": {...}}` envelope on a forced 400/401/429 (research did not confirm live), JSON-path transport errors classify via `is_timeout()`/`is_connect()` (the multipart masking bug, LL-G HIGH, should not reproduce on JSON bodies: verify, do not assume), and reasoning-token headroom in `max_completion_tokens`.
- **Exit: default models and preset params pinned in §2.4's table from measurements; spike verdict recorded in this file.**

### CP1: config schema + keychain
- `[voice]` + `[voice.provider]` tables, validation rules, per-kind chat defaults, provider resolution + inheritance as a pure function.
- `hark-keychain`: `HARK_CLEANUP_KEY`, generalized resolver, STT wrapper kept.
- Tests: empty TOML yields Clean voice + no provider table; inherit from openai/groq STT; deepgram STT resolves to Verbatim-with-warning marker; deepgram-as-cleanup rejected; custom voice requires prompt; `key_account` override honored; label stability test extended to the cleanup path.

### CP2: voices, prompt assembly, gate
- `Voice` enum + `FromStr` (config and CLI share it); system prompt templates; protected-terms subsetting (present-in-text, case-insensitive) + token budget cap; `skips_cleanup` word gate.
- Tests: each voice template contains its instruction and the closing return-only clause; terms absent from the text stay out of the prompt; budget drop order; gate boundaries (0 disables; exactly-threshold does not skip; "fewer than" semantics), unicode words count sanely.

### CP3: live adapter
- `OpenAiCompatibleChat` implementing `CleanupProvider`: buffered JSON body, Bearer auth, per-request 10 s timeout, status/transport error mapping, no retry.
- Same test shape as hark-stt's adapter: the live path is thin over the CP0 pure layer; unit tests cover construction (preset omission of temperature/effort) and the label; network behavior was proven by the CP0 spike and is re-proven at CP5.

### CP4: pipeline integration (Minor bump)
- Worker chain per §2.6 (gate, fail-open, dictionary pass 2), `CleanupPlan` wiring in `hark_pipeline::run` (provider resolution, key reuse on inherit, warn-and-Verbatim degradation), second pre-warm, `hark-cli --voice`.
- Tests with a scripted `MockCleaner` (pattern: worker.rs `MockProvider`): cleaned text is injected; cleanup error injects pass-1 text; short utterance never calls the cleaner; Verbatim never constructs a cleaner; terms re-mangled by the mock are repaired by pass 2; gate threshold 0 always calls.

### CP5: interactive gate (real hardware, Windows)
- User dictates across voices: Verbatim (verify no cleanup request in logs), Clean default (fillers removed, meaning intact), Professional/Casual (register shifts), Custom (their prompt), short utterances skip (log line present), dictionary terms survive cleanup, wrong-key and unplugged-network cases inject uncleaned text visibly and fast.
- Measure release-to-inject with cleanup on (target: STT p95 + cleanup p95 well under 2 s warm for a sentence); tune prompt wording and `skip_below_words` default if real usage disagrees.
- macOS deferral rule unchanged: nothing here is platform-specific; validation waits for Mac hardware.

## 4. Risks / open questions

- **GPT-5 temperature lock vs "one low-temp call".** The locked default (1) may produce rewrite variance. CP0 measures whether nano at minimal effort is deterministic enough in practice; if not, `gpt-4.1-mini` at 0.2 becomes the OpenAI default at ~5x output cost (still fractions of a cent per dictation).
- **Reasoning latency.** gpt-5-nano even at minimal effort may lose to non-reasoning models on time-to-full-response for 50-word rewrites. CP0's p50/p95 decides; Groq's 840 tok/s llama-3.1-8b-instant is the latency benchmark to beat.
- **Prompt-injection-shaped speech.** The transcript is user speech placed in the user message; a spoken "ignore previous instructions" could derail the rewrite. Single-user app, own risk, but the return-only clause and low temp bound the blast radius; note behavior at CP5 if observed.
- **Model deprecation drift.** Chat model IDs churn faster than STT models; defaults live in one place (per-kind table) and `model =` overrides always win. Re-verify IDs at execution time, not just planning time.
- **Error envelope assumption.** The shared `{"error": {...}}` shape was not live-verified during research; CP0 forces real 4xx responses before error-parsing code hardens.

## 5. Lessons Learned / Gotchas

Pre-implementation, seeded from research (2026-07-16); confirm or amend during implementation, then route durable ones to LL-G via `/add-lesson`:

- **OpenAI GPT-5 family locks `temperature` to the default (1)**; any other value is a 400. Serialize temperature only when configured; per-provider presets, never a hard-coded field. (Candidate LL-G entry once confirmed at CP0.)
- **Both OpenAI and Groq have converged on `max_completion_tokens`** (Groq deprecates `max_tokens`); no per-provider branching for the token cap.
- **Reasoning models spend reasoning tokens from `max_completion_tokens`**: a tight cap returns empty `content` with `finish_reason: "length"`, which looks like a provider bug. Keep generous headroom and treat empty content as fail-open.
- **`reasoning_effort` acceptance on gpt-5-nano is reported inconsistent** in the wild; verify at CP0 before baking it into the openai preset.
- reqwest 0.13: TLS features are `rustls` + `webpki-roots` (the 0.12 `rustls-tls-webpki-roots` umbrella is gone); copy hark-stt's Cargo.toml stanza, do not re-derive (LL-G `reqwest-013-tls-feature-rename`).
- The multipart error-masking bug (LL-G HIGH `reqwest-multipart-masks-transport-errors`) should not apply to buffered JSON bodies, but CP0 verifies `is_timeout()`/`is_connect()` classification on the JSON path rather than assuming it.
- Config change is additive: serde defaults + tolerated unknown keys carry it; version-stamped migration (BP `versioned-config-migration-backup`) deliberately deferred until the first breaking config change.
- Keychain: account = provider label, shared across roles; simple overwrite, no dual-read rotation (BP practice considered and declined for a single-user local app). `key_account` config override exists for the openai-compatible collision edge.
- No LL-G entries exist yet for `keyring` or TOML/serde migration pitfalls; Phase 3 is the first likely generator of them (e.g. Windows Credential Manager vs macOS Keychain behavior differences). Route findings back via `/add-lesson`.
- Cleanup is fail-open by design at every layer (unresolvable provider, missing key, request failure, empty response): the uncleaned dictionary-corrected transcript always injects. A dictation must never be lost to the optional feature.

Filled in during implementation:

- _(add as found)_
