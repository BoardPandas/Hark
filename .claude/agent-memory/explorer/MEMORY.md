# Explorer Memory Index

- [Hark STT stack risk (2026-07-15)](hark_stt_stack_risk.md) — sherpa-onnx+Parakeet TDT hotwords bug (#3267) is the real Phase-1 blocker, not availability.
- [sherpa-onnx Rust API specifics (2026-07-15)](sherpa_onnx_rust_api.md) — exact Cargo.toml, struct/field names, model download URLs, and code sketch for the Rust spike; 3 items flagged verify-during-spike.
- [Hark cloud STT Rust stack (2026-07-15)](hark_cloud_stt_rust_stack.md) — reqwest::blocking+multipart vs ureq, Deepgram SDK deps, WAV/FLAC/Opus size math, whisper-rs fallback candidate, for the BYOK cloud STT pivot.
- [Cloud STT provider landscape (2026-07-15)](hark_cloud_stt_providers.md) — BYOK pivot: OpenAI/Groq share one adapter shape, Deepgram best for dictionary biasing; comparison table + gotchas.
- [Phonetic post-correction crates (2026-07-16)](hark_phonetic_correction_crates.md) — rphonetic 3.0.6 + strsim 0.11.1 picks, n-gram sliding-window matching shape, short-word/Unicode/hyphenated-term pitfalls for the dictionary correction pass.
- [LLM cleanup chat models (2026-07-16)](hark_llm_cleanup_chat_models.md) — gpt-5-nano vs llama-3.1-8b-instant picks, pricing, temperature-lock conflict with "low temperature" spec for the Phase 3 shared OpenAI/Groq adapter.
- [Local STT model + Rust stack (2026-07-21)](hark_local_stt_2026.md) — Parakeet TDT 0.6B v2 + sherpa-onnx primary, transcribe-rs/Handy precedent runner-up; hotwords bug now moot (phonetic post-correction covers biasing); CPU latency ~1.5-2s/5s-utterance vs cloud's sub-1s.
