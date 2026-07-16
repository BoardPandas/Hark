---
name: hark-llm-cleanup-chat-models
description: OpenAI/Groq chat-completions model picks, pricing, and contract divergences for Hark's Phase 3 one-shot LLM cleanup call, verified 2026-07-16
metadata:
  type: project
---

Verified 2026-07-16 via WebFetch of OpenAI/Groq official docs (aggregator sites cross-checked, flagged where used). Hark's Phase 3 cleanup call is a hot-path, one-shot chat completion (5-200 word transcript in, rewritten text out), BYOK, must serve one shared adapter for both OpenAI and Groq's `/v1/chat/completions`-compatible endpoints.

## Recommended defaults

- **OpenAI default: `gpt-5-nano`** ($0.05/M in, $0.40/M out, 400K context). Fastest/cheapest GPT-5-family model, explicitly marketed as "fastest, cheapest version of GPT-5." Use `reasoning_effort: "minimal"` to cut reasoning-token latency for this kind of short deterministic rewrite task (OpenAI's own cookbook recommends minimal reasoning for "extraction, formatting, short rewrites, simple classification" — an exact match for Hark's use case). Some community reports flag inconsistent acceptance of `reasoning_effort` specifically on nano — verify empirically during the spike.
  - **Fallback if temperature control turns out to matter more than cost:** `gpt-4.1-mini` ($0.40/M in, $1.60/M out, non-reasoning, normal `temperature`/`max_tokens` semantics). See gotcha below — GPT-5-family locks temperature to the default (1), which conflicts with Hark's spec calling for "low temperature."
- **Groq default: `llama-3.1-8b-instant`** ($0.05/M in, $0.08/M out, 131K context, ~840 tok/s, production-tier not preview). Cheapest and fastest-to-first-response of Groq's production models; standard (non-reasoning) chat model, full `temperature` 0-2 range supported.
  - **Fallback/upgrade for quality:** `openai/gpt-oss-20b` (Groq-hosted OSS model, $0.075/M in, $0.30/M out, ~1000 tok/s) if 8B's rewrite quality proves insufficient — still cheap, still fast, still non-reasoning.
  - Groq's own `llama-4-scout-17bx16e` is explicitly a **preview model** ("should not be used in production, may be discontinued at short notice") — do not default to it.

## Pricing table (verified 2026-07-16)

| Provider | Model | Input $/M | Output $/M | Context | Notes |
|---|---|---|---|---|---|
| OpenAI | `gpt-5-nano` | $0.05 | $0.40 | 400K | reasoning model, temp locked to 1, use `reasoning_effort: minimal` |
| OpenAI | `gpt-5-mini` | $0.25 | $2.00 | 400K | reasoning model, same temp/max_completion_tokens constraints |
| OpenAI | `gpt-4.1-mini` | $0.40 | $1.60 | large | non-reasoning, normal temperature + max_tokens |
| OpenAI | `gpt-5.6-luna` | $1.00 | $6.00 | — | newer "nano-tier" latency per OpenAI's own docs, reasoning model, pricier than gpt-5-nano — not recommended as default given no clear latency win over nano for this use case |
| Groq | `llama-3.1-8b-instant` | $0.05 | $0.08 | 131K | production, non-reasoning, ~840 tok/s |
| Groq | `openai/gpt-oss-20b` | $0.075 | $0.30 | 131K | production, ~1000 tok/s |
| Groq | `meta-llama/llama-4-scout-17bx16e` | $0.11 | $0.34 | 131K | **preview only, don't use in production** |

## OpenAI vs Groq contract divergences affecting a shared adapter

1. **Reasoning vs non-reasoning model families diverge on `temperature`.** OpenAI's GPT-5 family (`gpt-5-nano`, `gpt-5-mini`) are reasoning models and **reject any non-default temperature** — API error: `"Unsupported value: 'temperature' does not support {value} with this model. Only the default (1) value is supported."` Groq's `llama-3.1-8b-instant` and `gpt-oss-20b` are plain chat models and accept full `temperature` 0-2 range normally. **A shared adapter must special-case or omit `temperature` for OpenAI's GPT-5 family** — sending Hark's "low temperature" setting straight through will 400 on OpenAI but work fine on Groq. This is the single biggest gotcha for the shared adapter.
2. **Token-limit param name has converged, not diverged, as of today.** Both OpenAI (GPT-5 family) and Groq's current docs specify `max_completion_tokens` as the parameter (Groq's docs explicitly call `max_tokens` deprecated/superseded). Good news for the shared adapter — use `max_completion_tokens` for both. (`gpt-4.1-mini` fallback still accepts legacy `max_tokens` too, but `max_completion_tokens` should work there as well per OpenAI's general GPT-4.1+ guidance — verify during spike since this wasn't independently confirmed for 4.1-mini specifically.)
3. **`reasoning_effort` is OpenAI-only** — GPT-5 family supports `reasoning_effort` (and `verbosity`); Groq's plain chat models have no equivalent parameter and would presumably error or ignore it if sent. Adapter must only set this for the OpenAI provider path.
4. **Rate-limit headers are identical in name between the two** — both expose `x-ratelimit-limit-requests`, `x-ratelimit-remaining-requests`, `x-ratelimit-limit-tokens`, `x-ratelimit-remaining-tokens`, `x-ratelimit-reset-requests`, `x-ratelimit-reset-tokens` (Groq also adds `retry-after`). Shared header-parsing code should work for both without provider-specific branches.
5. **Error body JSON shape**: OpenAI's documented shape is the long-standing `{"error": {"message", "type", "param", "code"}}` envelope (not independently re-confirmed this pass — OpenAI's own error-codes doc page didn't show raw JSON, this is carried from prior knowledge, re-verify against a live 400 response during the spike). Groq's error body shape was **not confirmed** this pass (their rate-limits doc didn't show the JSON either) — since Groq's endpoint is marketed OpenAI-compatible, the working assumption is it mirrors the same envelope, but this is unverified and should be checked against a real error response before the adapter's error-parsing code ships.

## Other OpenAI-compatible providers worth naming (one line each, not deep-dived)

- **Together AI** — broad model catalog, OpenAI-compatible endpoint, good "catalog depth" fallback if OpenAI/Groq models are ever unavailable.
- **Fireworks AI** — similar catalog-depth positioning to Together, markets itself on fast inference.
- **Cerebras** — wafer-scale hardware, claims ~2,200 tok/s on Llama 3.3 70B (fastest raw throughput found in this pass), narrow model catalog, now also powers Mistral's Le Chat "Flash Answers."
- **Mistral** — Le Chat's Flash Answers feature (Cerebras-backed) markets sub-second responses; Mistral's own API has historically been OpenAI-compatible for chat (unconfirmed this pass for their current chat completions endpoint specifically).

## Gotchas

- **Aggregator pricing sites disagree with each other and with official docs** — this pass saw "gpt-5.4-nano" and "gpt-5.6-luna" naming inconsistencies across aggregator vs. official OpenAI docs pages (some aggregators appear to be tracking a faster release cadence, e.g. "GPT-5.4"/"GPT-5.6" point releases, that the plain `gpt-5-nano`/`gpt-5-mini` model-ID pages didn't reflect). **Trust `developers.openai.com/api/docs/models/<id>` pages over third-party pricing aggregators** (costgoat, benchlm, tldl, aipricing.guru, pricepertoken all showed up as noisy secondary sources here) — re-verify pricing against that same official page immediately before shipping, since GPT-5-family pricing/tiers are evidently still moving.
- **GPT-5-nano's temperature lock directly conflicts with Hark's plan wording ("low temperature")** — this needs a product decision, not just an adapter shim: either (a) accept GPT-5-nano's fixed temp=1 and rely on the system prompt/instruction wording alone for determinism, or (b) default to `gpt-4.1-mini` for OpenAI specifically because it honors real temperature control, accepting the ~5x higher output-token cost. Flag this explicitly in the Phase 3 plan rather than silently picking one.
- **`reasoning_effort: minimal` support on `gpt-5-nano` specifically is inconsistently reported** in OpenAI's developer community as of this pass — confirm empirically (does the param get accepted, does it measurably cut latency) during the Phase 3 spike before hard-coding it into the default request shape.

## Sources (fetched/verified 2026-07-16)

- https://developers.openai.com/api/docs/pricing (redirect target of platform.openai.com/docs/pricing)
- https://developers.openai.com/api/docs/models/gpt-5-nano
- https://developers.openai.com/api/docs/models/gpt-5-mini
- https://developers.openai.com/api/docs/models/gpt-5.6-luna
- https://developers.openai.com/api/docs/models/gpt-4.1-mini
- https://developers.openai.com/api/docs/guides/reasoning
- https://cookbook.openai.com/examples/gpt-5/gpt-5_new_params_and_tools
- https://community.openai.com/t/temperature-in-gpt-5-models/1337133
- https://community.openai.com/t/gpt-5-nano-accepted-parameters/1355086
- https://console.groq.com/docs/api-reference
- https://console.groq.com/docs/models
- https://console.groq.com/docs/rate-limits
- https://groq.com/pricing (secondary, cross-checked against console.groq.com)

**Why:** Hark's Phase 3 adds an optional, hot-path, one-shot LLM cleanup call (BYOK, OpenAI + Groq via a single OpenAI-compatible adapter); model choice and the temperature/token-param divergence directly shape the adapter's request-building code and whether "low temperature" is even achievable on the default OpenAI model.

**How to apply:** Paste the pricing table and the temperature-lock gotcha directly into the Phase 3 plan's model-selection section. Resolve the temperature-lock product decision (gpt-5-nano fixed-temp vs gpt-4.1-mini real-temp) with the user before writing adapter code, and empirically verify `reasoning_effort: minimal` behavior and Groq's error-body JSON shape during the spike, both flagged unconfirmed above. Cross-reference [[hark-cloud-stt-providers]] and [[hark-cloud-stt-rust-stack]] for the sibling BYOK STT adapter work — the shared-adapter pattern (one OpenAI-compatible adapter covering OpenAI + Groq) mirrors the STT adapter design already chosen there.
