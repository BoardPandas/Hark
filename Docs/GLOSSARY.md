<!-- PAGE_ID: hark_14_glossary -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [README.md](../README.md)
- [CLAUDE.md](../CLAUDE.md)
- [crates/hark-stt/src/lib.rs](../crates/hark-stt/src/lib.rs)
- [crates/hark-audio/src/ring.rs](../crates/hark-audio/src/ring.rs)
- [crates/hark-hotkey/src/edges.rs](../crates/hark-hotkey/src/edges.rs)
- [crates/hark-pipeline/src/events.rs](../crates/hark-pipeline/src/events.rs)

</details>

# Glossary

> **Related Pages**: [Overview](OVERVIEW.md), [Architecture](core/ARCHITECTURE.md)

---

<!-- BEGIN:AUTOGEN hark_14_glossary_terms -->
## Terms

- **BYOK (bring your own key):** The user supplies credentials for cloud transcription and optional cleanup. Keys live in the OS keychain, not `config.toml`; local-primary STT needs no cloud key ([README.md:1-10](../README.md#L1-L10)).
- **Cleanup pass:** An optional OpenAI-compatible chat request that applies the selected voice after transcription. Verbatim mode, a fired invocation, or a Gemini Smart transcript skips this call ([Architecture](core/ARCHITECTURE.md#the-release-to-inject-pipeline)).
- **Fused cleanup:** Transcription and tidying performed by one provider turn. Gemini Smart returns one tidied string in both transcript fields and marks it so Hark does not clean it a second time ([Transcription](features/TRANSCRIPTION.md#deepgram-and-gemini-live)).
- **Invocation:** A guarded spoken trigger that replaces matching transcript text with user-authored canned text. A fired invocation is injected without a cleanup model rewriting it ([Invocations](features/INVOCATIONS.md)).
- **Live STT:** A session opened on key-down and fed while the user speaks. The finished ring-buffer window remains available for gates and fallback ([lib.rs:44-70](../crates/hark-stt/src/lib.rs#L44-L70)).
- **Local primary / local backup:** On-device STT can be the only transcription engine or a fallback after a bounded cloud attempt. Model weights are downloaded explicitly and stored locally ([On-Device STT](features/ON_DEVICE_STT.md)).
- **OpenAI-compatible:** The Whisper-family multipart `/audio/transcriptions` contract, including compatible third-party endpoints. OpenAI's newer gpt-transcribe models use a separate adapter because their bias fields differ ([Transcription](features/TRANSCRIPTION.md#openai-compatible-and-gpt-transcribe-adapters)).
- **Provider-cleaned transcript:** A `Transcript` with `cleaned: Some(...)`, signaling that the STT provider already formatted the text and Hark should suppress the separate cleanup call ([lib.rs:30-42](../crates/hark-stt/src/lib.rs#L30-L42)).
- **Push-to-talk chord:** One or more configured keys that must all be held to record. The first release ends the hold; injected keys are ignored, and missed releases are reconciled against physical state ([edges.rs:161-180](../crates/hark-hotkey/src/edges.rs#L161-L180), [edges.rs:295-435](../crates/hark-hotkey/src/edges.rs#L295-L435)).
- **Release-to-inject:** The product latency between releasing the chord and text appearing at the cursor. Gemini streaming can upload most audio before this interval begins ([README.md:7-10](../README.md#L7-L10), [Architecture](core/ARCHITECTURE.md#retry-and-latency-discipline)).
- **Ring buffer:** A fixed-size, allocation-free audio buffer written at the input device's native rate. Absolute sample indexes support pre-roll, tail, a live reader, and finished-window assembly ([ring.rs:1-12](../crates/hark-audio/src/ring.rs#L1-L12)).
- **Spellbook:** Local terms used for provider-specific vocabulary bias and guarded phonetic correction after transcription. Bias wire formats include `prompt`, `keywords[]`, `keyterm`, and Gemini custom vocabulary ([README.md:19-32](../README.md#L19-L32)).
- **Tray daemon:** Hark's normal hidden state: the pipeline remains active while the settings, history, and stats window opens only on demand ([README.md:63](../README.md#L63)).
- **Verbatim / Smart:** Gemini rendering modes. Verbatim preserves the literal transcript and is the default; Smart removes disfluencies and formats within the same provider turn ([Transcription](features/TRANSCRIPTION.md#deepgram-and-gemini-live)).
<!-- END:AUTOGEN hark_14_glossary_terms -->

---

<!-- BEGIN:AUTOGEN hark_14_glossary_acronyms -->
## Acronyms

| Acronym | Expansion | Use in Hark |
|---|---|---|
| BYOK | Bring your own key | User-owned cloud provider credentials |
| PTT | Push to talk | Hold-to-record interaction |
| RMS | Root mean square | Audio loudness measurement used by the gate |
| STT | Speech to text | Cloud or on-device transcription |
| SPSC | Single producer, single consumer | Core ring-buffer ownership model; extra readers are immutable handles |
| UI | User interface | egui window, tray, overlay, and status surfaces |
| WAV | Waveform Audio File Format | Complete PCM16 container sent to batch adapters |
| WebSocket | WebSocket protocol | Bidirectional transport used by Gemini Live |
<!-- END:AUTOGEN hark_14_glossary_acronyms -->

---
