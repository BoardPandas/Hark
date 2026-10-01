<!-- PAGE_ID: hark_09_voice_cleanup -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [crates/hark-voice/src/lib.rs](../../crates/hark-voice/src/lib.rs)
- [crates/hark-voice/src/voices.rs:1-281](../../crates/hark-voice/src/voices.rs#L1-L281)
- [crates/hark-voice/src/openai_compatible.rs:1-286](../../crates/hark-voice/src/openai_compatible.rs#L1-L286)
- [crates/hark-voice/src/error.rs:1-160](../../crates/hark-voice/src/error.rs#L1-L160)
- [crates/hark-voice/src/summary.rs:1-45](../../crates/hark-voice/src/summary.rs#L1-L45)
- [crates/hark-config/src/voice.rs:1-338](../../crates/hark-config/src/voice.rs#L1-L338)
- [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs)
- [crates/hark-pipeline/src/lib.rs](../../crates/hark-pipeline/src/lib.rs)
- [crates/hark-pipeline/src/worker.rs:440-618](../../crates/hark-pipeline/src/worker.rs#L440-L618)
- [crates/hark-pipeline/src/meeting/finish.rs](../../crates/hark-pipeline/src/meeting/finish.rs)
- [crates/hark-stt/src/lib.rs](../../crates/hark-stt/src/lib.rs)
- [crates/hark-stt/src/gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)

</details>

# Voice Cleanup

Meeting notes are an independent post-transcription request. The selected
Deepgram/Gemini final pass does not change the notes provider. An explicit re-run
of a saved recording preserves its notes and checked action items; it does not
regenerate them from the replacement transcript. Dictation's Verbatim setting
does not disable new-meeting summaries
([finisher](../../crates/hark-pipeline/src/meeting/finish.rs),
[replacement](../../crates/hark-store/src/meetings.rs)).

> **Related Pages**: [Transcription](TRANSCRIPTION.md), [Spellbook](SPELLBOOK.md), [Invocations](INVOCATIONS.md), [Configuration and Secrets](../core/CONFIGURATION.md), [Meetings](MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_overview -->
## Overview

Voice cleanup is Hark's optional rewrite stage between transcription/spellbook processing and text injection. For a resolved non-Verbatim voice, Hark sends one corrected transcript to an OpenAI-compatible chat-completions endpoint; `Voice::Verbatim`, an unresolved provider, short text, an invocation, or an already-cleaned fused transcript makes no separate cleanup call ([lib.rs:1-10](../../crates/hark-voice/src/lib.rs#L1-L10), [worker.rs:550-580](../../crates/hark-pipeline/src/worker.rs#L550-L580)).

Cleanup is deliberately fail-open. There is no retry: any adapter, timeout, parsing, expansion-guard, or provider failure injects the pre-cleanup text instead of losing the dictation or doubling worst-case latency. The crate uses blocking `reqwest` on the existing pipeline worker, through the process-wide client from `hark-stt` (static Mozilla trust roots, no OS certificate store; [Transcription](TRANSCRIPTION.md)), and never logs API keys, prompts, spellbook terms, or transcript text ([lib.rs:7-10](../../crates/hark-voice/src/lib.rs#L7-L10), [openai_compatible.rs:192-195](../../crates/hark-voice/src/openai_compatible.rs#L192-L195), [worker.rs:560-617](../../crates/hark-pipeline/src/worker.rs#L560-L617)).

```mermaid
graph TD
    A["Spellbook pass 1"] --> B{"Invocation fired or provider already cleaned?"}
    B -->|"Yes"| C["Skip separate cleanup"]
    B -->|"No"| D{"Cleanup plan and word gate pass?"}
    D -->|"No"| E["Keep pass-1 text"]
    D -->|"Yes"| F["One chat-completions call"]
    F -->|"Error or excessive expansion"| E
    F -->|"Accepted"| G["Spellbook pass 2"]
    C --> H["Inject"]
    E --> H
    G --> H
```

The configured voice and the provider are resolved independently. A non-Verbatim voice may inherit an OpenAI, Groq, or Gemini STT provider and key, use an explicit cleanup provider/key, or degrade to Verbatim with a startup warning when no chat-capable provider resolves ([voice.rs:248-338](../../crates/hark-config/src/voice.rs#L248-L338)).

Meeting notes reuse this same fail-open shape at a coarser grain: `hark-voice::summarize` calls a chat-completions provider once, after the call ends rather than on the hot path, and returns validated `MeetingNotes` (title, summary, key points, decisions, action items) or an error the meeting keeps its transcript through — never a partial or invented result ([summary.rs:1-12](../../crates/hark-voice/src/summary.rs#L1-L12)). See [Meetings](MEETINGS.md#notes) for where that call fits in a call's lifecycle.

Sources: [crates/hark-voice/src/lib.rs](../../crates/hark-voice/src/lib.rs), [crates/hark-config/src/voice.rs:248-338](../../crates/hark-config/src/voice.rs#L248-L338), [crates/hark-pipeline/src/worker.rs:550-618](../../crates/hark-pipeline/src/worker.rs#L550-L618), [crates/hark-voice/src/summary.rs:1-12](../../crates/hark-voice/src/summary.rs#L1-L12)
<!-- END:AUTOGEN hark_09_voice_cleanup_overview -->

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_voices -->
## Voice Presets

Hark currently recognizes 11 voice names. Parsing is case-insensitive and whitespace-tolerant; configuration and the CLI share the same names ([voices.rs:8-98](../../crates/hark-voice/src/voices.rs#L8-L98)).

| Voice | Intended transformation |
|---|---|
| `verbatim` | No cleanup adapter and no call |
| `grammar` | Correct grammar, spelling, punctuation, and capitalization without rephrasing |
| `clean` | Remove fillers/false starts/repetition while keeping wording and tone |
| `professional` | Polish into a business register |
| `casual` | Keep a relaxed conversational register |
| `notes` | Produce concise first-person work/ticket notes |
| `concise` | Remove repetition and redundancy while preserving facts |
| `direct` | Lead with the point and remove hedging/warm-up phrases |
| `plain` | Use courteous, non-technical wording without adding explanations |
| `pirate` | Novelty pirate diction while retaining facts and length |
| `custom` | Use the user's prompt text as the instruction |

The built-in instructions are defined together so their behavior is explicit and testable ([voices.rs:170-216](../../crates/hark-voice/src/voices.rs#L170-L216)). Every built-in prompt also receives a no-em/en-dash clause and a length-discipline clause; `custom` is the escape hatch and receives neither, because deliberate expansion or formatting may be the user's request. All prompts end with the return-only clause, and present spellbook terms are protected within a bounded 400-token-equivalent budget ([voices.rs:110-168](../../crates/hark-voice/src/voices.rs#L110-L168), [voices.rs:243-280](../../crates/hark-voice/src/voices.rs#L243-L280)).

The default settings are `clean`, skip below five words, and reject built-in output above a 1.4x ratio (with three words of absolute slack); zero disables either gate. The post-response expansion guard does not apply to `custom` ([voice.rs:49-84](../../crates/hark-config/src/voice.rs#L49-L84), [voices.rs:218-240](../../crates/hark-voice/src/voices.rs#L218-L240)).

Sources: [crates/hark-voice/src/voices.rs:8-120](../../crates/hark-voice/src/voices.rs#L8-L120), [crates/hark-voice/src/voices.rs:145-280](../../crates/hark-voice/src/voices.rs#L145-L280), [crates/hark-config/src/voice.rs:49-84](../../crates/hark-config/src/voice.rs#L49-L84)
<!-- END:AUTOGEN hark_09_voice_cleanup_voices -->

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_adapter -->
## Cleanup Adapter

`OpenAiCompatibleChat` is the live cleanup adapter. It sends buffered JSON to `POST {base_url}/chat/completions` with Bearer authentication and the process-wide blocking HTTP client. The same contract serves OpenAI, Groq, Gemini's OpenAI-compatible endpoint, and user-supplied compatible endpoints; Deepgram has no chat product and is rejected by configuration validation ([openai_compatible.rs:1-18](../../crates/hark-voice/src/openai_compatible.rs#L1-L18), [voice.rs:87-160](../../crates/hark-config/src/voice.rs#L87-L160), [voice.rs:191-223](../../crates/hark-config/src/voice.rs#L191-L223)).

The provider resolver supports these paths:

| Configuration | Result |
|---|---|
| Verbatim voice | No provider or key is needed |
| Explicit `[voice.provider]` | Uses the configured kind/base URL/model and a cleanup account key |
| OpenAI, Groq, or Gemini STT with no override | Inherits the provider and already-resolved STT key; Gemini defaults to its `/v1beta/openai` chat URL and `gemini-3.5-flash-lite` model |
| Deepgram or generic OpenAI-compatible STT with no override | Degrades to Verbatim with a warning because STT support does not prove a chat endpoint exists |

The resolution rules and presets live in `hark-config`, while pipeline construction performs key lookup, builds the adapter, and converts every missing-key/build failure into `None` so STT still starts ([voice.rs:115-189](../../crates/hark-config/src/voice.rs#L115-L189), [voice.rs:225-338](../../crates/hark-config/src/voice.rs#L225-L338), [lib.rs](../../crates/hark-pipeline/src/lib.rs)).

Meeting notes resolve the same way, through the same `resolve_cleanup_provider`, but ask for it as if a non-Verbatim voice were selected — a `Verbatim` dictation setup still has a text provider that can write notes, since the summary call is independent of the dictation cleanup voice ([finish.rs](../../crates/hark-pipeline/src/meeting/finish.rs)).

Each request builds a system prompt from the effective voice and terms present in that transcript, sends the transcript fenced in `<transcript>` tags, and opens the system prompt with `TRANSCRIPT_IS_DATA_CLAUSE`, which tells the model the fenced text is dictation to edit, never a request to answer. Without it, dictation that sounds like an instruction ("Proceed however you recommend...") was answered instead of edited. The adapter also strips an echoed fence from the response. It derives `max_completion_tokens` from input length with a 512-to-4096 clamp, applies a 10-second request timeout, and never retries ([openai_compatible.rs:20-85](../../crates/hark-voice/src/openai_compatible.rs#L20-L85), [openai_compatible.rs:236-281](../../crates/hark-voice/src/openai_compatible.rs#L236-L281)). `CleanupConfig` has a manual `Debug` implementation that redacts the API key and custom prompt and reports only the spellbook-term count ([openai_compatible.rs:146-190](../../crates/hark-voice/src/openai_compatible.rs#L146-L190)).

Sources: [crates/hark-voice/src/openai_compatible.rs:1-286](../../crates/hark-voice/src/openai_compatible.rs#L1-L286), [crates/hark-config/src/voice.rs:87-189](../../crates/hark-config/src/voice.rs#L87-L189), [crates/hark-config/src/voice.rs:225-338](../../crates/hark-config/src/voice.rs#L225-L338), [crates/hark-pipeline/src/lib.rs](../../crates/hark-pipeline/src/lib.rs), [crates/hark-pipeline/src/meeting/finish.rs](../../crates/hark-pipeline/src/meeting/finish.rs)
Nonempty content is rejected when `finish_reason` is `length`, `content_filter`,
`tool_calls`, or `function_call`: a returned prefix is not a complete cleanup.
The existing fail-open path keeps the original transcript. Compatible endpoints
that omit the reason remain supported, and error messages include only trusted
reason labels, never response text
([parser](../../crates/hark-voice/src/openai_compatible.rs),
[regressions](../../crates/hark-voice/tests/chat_pure.rs)).
<!-- END:AUTOGEN hark_09_voice_cleanup_adapter -->

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_pipeline -->
## Pipeline Behavior

After a non-empty transcription, the worker applies spellbook pass 1 and then checks invocations. A fired invocation is user-authored canned text, so it skips cleanup and the second spellbook pass entirely. A fused provider result also skips the separate cleanup call, preventing a second rewrite and bill ([worker.rs:451-476](../../crates/hark-pipeline/src/worker.rs#L451-L476), [worker.rs:550-558](../../crates/hark-pipeline/src/worker.rs#L550-L558)).

For an ordinary transcript with a cleanup plan:

1. Text below `skip_below_words` passes through unchanged.
2. The adapter performs one rewrite request.
3. A built-in voice response beyond `max_expansion_ratio` plus the three-word grace is rejected; `custom` is exempt.
4. A built-in voice response that reads as a reply rather than an edit is rejected by `reads_as_reply`. It uses the prompt's own vocabulary ("transcript", "rewrite") that the speaker never said, or it keeps none of the speaker's words of four or more letters when the input has at least three. `custom` is exempt, because a translation prompt keeps no words.
5. An accepted response goes through spellbook pass 2 to repair any protected term the model still changed.
6. Any error returns pass-1 text with no cleanup timing/model attribution.

This control flow is implemented in `cleaned_text` and keeps history honest: cleanup metadata appears only when its response actually shaped the injected text ([worker.rs:542-618](../../crates/hark-pipeline/src/worker.rs#L542-L618)).

Gemini Live has a distinct fused `smart` mode. In that mode the single returned string is put in both `Transcript.text` and `Transcript.cleaned`; the marker tells the worker to skip Hark's separate voice call. This reduces one round trip but cannot preserve a guaranteed verbatim transcript, which is why the configured default remains `verbatim` ([lib.rs](../../crates/hark-stt/src/lib.rs), [gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs), [lib.rs](../../crates/hark-config/src/lib.rs)). History labels that result as voice `smart` and attributes the STT model as the cleanup model; ordinary skipped/failed cleanup is labeled `verbatim` with no cleanup model ([worker.rs:496-530](../../crates/hark-pipeline/src/worker.rs#L496-L530)).

Meeting mode never sees Gemini Live's fused `smart` result: the live transcriber uses the ordinary batch `SttProvider::transcribe` path per chunk (or the on-device engine), never the streaming session, so a meeting line always goes through the spellbook corrector as text, with no fused-cleanup shortcut to skip ([live.rs:60-77](../../crates/hark-pipeline/src/meeting/live.rs#L60-L77)).

The schema-6 meeting echo setting controls an earlier audio stage: it filters the
microphone before recording and STT. Meeting notes still use the independent
summary setting and text-provider request after transcription
([echo processing](MEETINGS.md#reduce-speaker-echo),
[configuration](../../crates/hark-config/src/meeting.rs)).

Sources: [crates/hark-pipeline/src/worker.rs:451-530](../../crates/hark-pipeline/src/worker.rs#L451-L530), [crates/hark-pipeline/src/worker.rs:542-618](../../crates/hark-pipeline/src/worker.rs#L542-L618), [crates/hark-stt/src/gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs), [crates/hark-config/src/lib.rs](../../crates/hark-config/src/lib.rs), [crates/hark-pipeline/src/meeting/live.rs:60-77](../../crates/hark-pipeline/src/meeting/live.rs#L60-L77)
Insights reports usage by the configured voice and completion latency, alongside measured local Spellbook replacement counts. It does not infer how many edits the cleanup provider made or assign an accuracy score. Replacement counts include the second Spellbook pass only when that pass runs; the fired-invocation branch still bypasses cleanup and later rewriting. All numeric persistence follows insertion ([worker](../../crates/hark-pipeline/src/worker.rs), [Insights aggregation](../../crates/hark-store/src/insights/query.rs)).
<!-- END:AUTOGEN hark_09_voice_cleanup_pipeline -->

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_api -->
## Public API

The crate root keeps the reusable behavior small and provider-neutral ([lib.rs](../../crates/hark-voice/src/lib.rs)).

| Item | Contract |
|---|---|
| `CleanupProvider` | Blocking, `Send` trait with `clean(text)` and a safe provider label; pipeline tests can replace it with a scripted cleaner |
| `Cleaned` | Accepted text plus full request wall time |
| `Voice` / `UnknownVoice` | Runtime voice enum and parsing error with the valid names |
| `system_prompt` / `present_terms` | Pure per-request prompt assembly and protected-term filtering |
| `skips_cleanup` / `over_expanded` / `reads_as_reply` | Pure pre-request and post-response gates |
| `CleanupError` / mapping helpers | Log-safe error taxonomy and pure status/transport classification |
| `CONNECT_TIMEOUT_MS` / `CLEANUP_TIMEOUT_MS` | 3-second connection bound and 10-second per-request bound |
| `summarize` / `MeetingNotes` / `SummaryConfig` | Meeting-only: one long-context call over a full transcript, validated into structured notes; `SUMMARY_TIMEOUT_MS` is 120 s, not the dictation `CLEANUP_TIMEOUT_MS` ([summary.rs:37](../../crates/hark-voice/src/summary.rs#L37)) |

The concrete `CleanupConfig` and `OpenAiCompatibleChat` stay in the public `openai_compatible` module rather than being re-exported from the root. Construction rejects `Verbatim`, because reaching the adapter for a voice that promises no call is a caller bug handled by the pipeline's fail-open build path ([openai_compatible.rs:146-234](../../crates/hark-voice/src/openai_compatible.rs#L146-L234)). `summary` follows the same discipline in reverse: it reuses `openai_compatible`'s URL-building, response-parsing, and status/transport error mapping rather than duplicating them, so only the notes-specific prompt, JSON schema, and validation are new code ([summary.rs:6-12](../../crates/hark-voice/src/summary.rs#L6-L12)).

Sources: [crates/hark-voice/src/lib.rs](../../crates/hark-voice/src/lib.rs), [crates/hark-voice/src/openai_compatible.rs:146-234](../../crates/hark-voice/src/openai_compatible.rs#L146-L234), [crates/hark-voice/src/summary.rs:1-45](../../crates/hark-voice/src/summary.rs#L1-L45)
<!-- END:AUTOGEN hark_09_voice_cleanup_api -->

---

<!-- BEGIN:AUTOGEN hark_09_voice_cleanup_errors -->
## Error Handling

`CleanupError` intentionally mirrors the STT error categories while remaining a separate small enum. Adapter diagnostics contain only trusted labels, status codes, and structural information: no API key, Authorization header, prompt, spellbook term, transcript text, or provider response body ([error.rs](../../crates/hark-voice/src/error.rs)).

| Variant | Trigger and sanitization |
|---|---|
| `Http` | Fixed transport category; the underlying error string and request URL are never included |
| `Auth` | HTTP 401/403 plus an allowlisted `error.code`/`error.type`, if recognized; unknown values and prose are omitted |
| `RateLimited` | HTTP 429, with optional seconds-form `Retry-After` |
| `Timeout` | Connect or request timeout, reporting the bound actually hit |
| `Provider` | HTTP status, a fixed failure category, or JSON error category with numeric line/column; no response snippets |

Even a short machine-readable field can contain user text, so authentication reasons must match a fixed allowlist such as `invalid_api_key` or `insufficient_quota`. A 401 adds “check your API key” only when no recognized reason exists; a 403 does not add that hint. Transport mapping distinguishes the shared 3-second connect timeout from the 10-second request timeout without formatting the underlying error ([error.rs](../../crates/hark-voice/src/error.rs)).

Successful HTTP status is not sufficient: parsing requires a non-empty `choices[0].message.content`. Missing/empty content becomes `Provider`, retaining only an allowlisted `finish_reason` such as `length`; arbitrary values become `unknown`. JSON parser errors expose the category and numeric line/column because the parser's full error string can quote an offending transcript value. This rule also covers meeting-summary responses and stored notes JSON. Ordinary cleanup failures keep the uncleaned transcript and record no cleanup attribution ([response parser](../../crates/hark-voice/src/openai_compatible.rs), [summary parser](../../crates/hark-voice/src/summary.rs), [pipeline](../../crates/hark-pipeline/src/worker.rs)).

Sources: [error mapping](../../crates/hark-voice/src/error.rs), [response parsing](../../crates/hark-voice/src/openai_compatible.rs), [summary parsing](../../crates/hark-voice/src/summary.rs), [privacy regression tests](../../crates/hark-voice/tests/error_privacy.rs)
<!-- END:AUTOGEN hark_09_voice_cleanup_errors -->

---
