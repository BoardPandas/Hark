<!-- PAGE_ID: hark_07_transcription -->
<details>
<summary>Relevant source files</summary>

The following files were used as evidence for this page:

- [crates/hark-stt/src/lib.rs](../../crates/hark-stt/src/lib.rs)
- [crates/hark-stt/src/config.rs](../../crates/hark-stt/src/config.rs)
- [crates/hark-stt/src/openai_compatible.rs](../../crates/hark-stt/src/openai_compatible.rs)
- [crates/hark-stt/src/openai_transcribe.rs](../../crates/hark-stt/src/openai_transcribe.rs)
- [crates/hark-stt/src/deepgram.rs](../../crates/hark-stt/src/deepgram.rs)
- [crates/hark-stt/src/gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)
- [crates/hark-stt/src/wav.rs](../../crates/hark-stt/src/wav.rs)
- [crates/hark-stt/src/error.rs](../../crates/hark-stt/src/error.rs)
- [crates/hark-stt/src/metrics.rs](../../crates/hark-stt/src/metrics.rs)
- [crates/hark-stt/src/meeting.rs](../../crates/hark-stt/src/meeting.rs)

</details>

# Transcription (STT Providers)

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md), [On-Device STT](ON_DEVICE_STT.md), [Spellbook](SPELLBOOK.md), [Voice Cleanup](VOICE_CLEANUP.md), [Meetings](MEETINGS.md)

---

<!-- BEGIN:AUTOGEN hark_07_transcription_trait -->
## Batch and Live Provider Traits

`hark-stt` separates finished-clip transcription from streaming. `SttProvider::transcribe` accepts a complete 16 kHz mono WAV and blocks on the pipeline worker. `LiveStt` opens a single-use `LiveSession`; the session accepts 16 kHz mono samples while the key is held and finalizes at release ([lib.rs](../../crates/hark-stt/src/lib.rs)).

`Transcript` always contains `text`, optionally marks a fused provider result as `cleaned`, and records provider request time. That optional field is a capability signal: the pipeline skips its separate cleanup call when a provider already performed cleanup ([lib.rs](../../crates/hark-stt/src/lib.rs)).

`build` constructs one of five internal adapter kinds: Whisper-family OpenAI-compatible, OpenAI gpt-transcribe, Deepgram, Gemini Live, or the internal Gemini batch adapter. `build_live` returns a streaming adapter only for Gemini Live builds with the `live` feature ([lib.rs](../../crates/hark-stt/src/lib.rs), [config.rs:1-20](../../crates/hark-stt/src/config.rs#L1-L20)). The app-facing provider choices currently route to Deepgram, OpenAI-compatible Whisper, OpenAI gpt-transcribe, Gemini Live, or optional local STT.

Separate meeting-only paths sit beside these traits: `hark-stt::meeting` sends a long-form Deepgram request, while `meeting_gemini` processes independent track windows. Neither implements the dictation `SttProvider` trait. Saved-recording re-runs explicitly send the retained stereo MP3 unchanged to Deepgram; Gemini is selected for processing after Stop (see [Meetings](MEETINGS.md#gemini-files-final-pass)).

```mermaid
classDiagram
    class SttProvider {
        +transcribe(wav_bytes) Transcript
        +label() str
    }
    class LiveStt {
        +start_session() LiveSession
    }
    class LiveSession {
        +push(samples)
        +finish() Transcript
    }
    class GeminiLive
    SttProvider <|.. GeminiLive
    LiveStt <|.. GeminiLive
    LiveStt --> LiveSession
```

The process shares one blocking HTTP client with 3-second connect and 15-second total request bounds. Every Hark HTTP client trusts only Mozilla's root set compiled in from `webpki-root-certs` (`tls_certs_only`), never the OS certificate store, so a locally installed CA cannot intercept requests carrying the user's key. reqwest 0.13.2+ removed the `webpki-roots` feature that used to provide this, and requesting it silently held reqwest at 0.13.1 ([lib.rs](../../crates/hark-stt/src/lib.rs)). Gemini alone owns a private current-thread Tokio runtime for WebSocket I/O; it does not turn the rest of Hark into an async application ([lib.rs](../../crates/hark-stt/src/lib.rs), [gemini_live.rs:728-765](../../crates/hark-stt/src/gemini_live.rs#L728-L765)). The meeting final pass reuses the same shared client but its own, much longer timeout: `FINAL_PASS_TIMEOUT_MS` is 900,000 ms (15 min), because the request both uploads roughly 230 MB per recorded hour and waits on Deepgram's own processing, both far past a dictation's 15-second budget ([meeting.rs:23-25](../../crates/hark-stt/src/meeting.rs#L23-L25)).
<!-- END:AUTOGEN hark_07_transcription_trait -->

---

<!-- BEGIN:AUTOGEN hark_07_transcription_openai -->
## OpenAI-Compatible and gpt-transcribe Adapters

Both adapters POST a hand-built multipart body to `{base_url}/audio/transcriptions`, authenticate with a Bearer token, and parse the same JSON `text` envelope. Buffering multipart data keeps transport failures classifiable and makes boundary construction testable ([openai_compatible.rs:1-10](../../crates/hark-stt/src/openai_compatible.rs#L1-L10), [openai_compatible.rs:95-195](../../crates/hark-stt/src/openai_compatible.rs#L95-L195)).

They intentionally use different vocabulary fields:

| Contract | Models | Spellbook bias |
|---|---|---|
| OpenAI-compatible | Whisper-family endpoints, including compatible third parties | One ordered, budgeted comma-separated `prompt` glossary ([openai_compatible.rs:52-93](../../crates/hark-stt/src/openai_compatible.rs#L52-L93)) |
| OpenAI gpt-transcribe | OpenAI's gpt-transcribe contract | Repeated `keywords[]` fields with `languages[]=en`; no synthetic `prompt` ([openai_transcribe.rs:53-75](../../crates/hark-stt/src/openai_transcribe.rs#L53-L75)) |

The split is contractual, not cosmetic. Selecting a gpt-transcribe model uses `OpenAiTranscribe`, while Whisper-compatible models keep the prompt-based adapter. Both return verbatim `Transcript` values with `cleaned: None` ([openai_transcribe.rs:77-126](../../crates/hark-stt/src/openai_transcribe.rs#L77-L126)).
<!-- END:AUTOGEN hark_07_transcription_openai -->

---

<!-- BEGIN:AUTOGEN hark_07_transcription_deepgram -->
## Deepgram and Gemini Live

Deepgram uses `POST {base_url}/v1/listen`, `Token` authentication, a raw `audio/wav` body, `smart_format=true`, and one URL-encoded `keyterm` parameter per spellbook term. Its parser defensively walks `results.channels[0].alternatives[0].transcript` ([deepgram.rs:18-99](../../crates/hark-stt/src/deepgram.rs#L18-L99)).

Gemini Live uses a WebSocket session and can receive audio during the key hold. `GeminiLiveSession::push` converts samples to PCM16, sends them, and drains incoming transcript segments so a long hold does not fill the receive buffer. `finish` ends the turn or uses already completed segments and returns a normal `Transcript` ([gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)).

Meetings reuse the replay path for their live-transcript chunks through `build_meeting_chunks` ([lib.rs](../../crates/hark-stt/src/lib.rs)), which tunes Gemini Live with `Finalize::MEETING_CHUNK`: a 15 s per-frame and 45 s total finalise wait instead of dictation's 8 s and 15 s, and a turn in which the server produced no word at all (no interim, no final) is an empty transcript rather than a timeout. A 20–30 s chunk can pass the loudness gate on a cough or keystrokes and hold no speech, and Gemini answers such a turn with silence; the first real 20-minute call logged 19 of those as failures. Dictation keeps the timeout: the user pressed the key to say something.

Gemini supports two rendering modes:

- `Verbatim` is the default and leaves fillers and self-corrections intact.
- `Smart` asks Gemini to format and remove disfluencies in the same round trip. The returned string is placed in both `text` and `cleaned`; the latter tells the pipeline not to clean it again ([gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs), [gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)).

Streaming is an accelerator, never the only copy of a dictation. The pipeline pump reads the audio ring without consuming it, so an unavailable or broken live session can fall back to batch. Smart mode is the deliberate exception to replay because spoken edit commands can legally erase a turn, making a replay semantically unsafe ([gemini_live.rs:170-177](../../crates/hark-stt/src/gemini_live.rs#L170-L177), [Architecture](../core/ARCHITECTURE.md#the-release-to-inject-pipeline)).

**The meeting final pass** is a third, unrelated use of Deepgram: one `multichannel=true&diarize=true&utterances=true` request over a whole stereo recording (left = Me/microphone, right = Them/system audio), sent after the call ends to replace the live Me/Them transcript with diarized "Speaker N" utterances within Them. Deepgram diarizes channel 0 too, so `speaker` is forced to `None` there rather than splitting "Me" into two speakers ([meeting.rs:1-6](../../crates/hark-stt/src/meeting.rs#L1-L6), [meeting.rs:52-75](../../crates/hark-stt/src/meeting.rs#L52-L75)). The request body is streamed rather than buffered (an hour of stereo 16 kHz PCM16 is ~115 MB), which reopens the multipart-masks-transport-errors failure mode even without the `multipart` feature; `classify_final_pass_error` falls back to walking the transport error's `source()` chain for the underlying `io::Error` before giving up and reporting a generic transport error ([meeting.rs:6-15](../../crates/hark-stt/src/meeting.rs#L6-L15)). Parsing clamps every utterance's timestamps into `[0, audio_ms]` (Deepgram has invented an end time past the audio length before) and drops blank transcripts ([meeting.rs:112-121](../../crates/hark-stt/src/meeting.rs#L112-L121)). See [Meetings](MEETINGS.md#deepgram-final-pass) for where this fits in a call's lifecycle.
<!-- END:AUTOGEN hark_07_transcription_deepgram -->

---

<!-- BEGIN:AUTOGEN hark_07_transcription_wav -->
## WAV and Streaming Audio

Batch and fallback adapters receive complete 16 kHz, mono, PCM16 WAV. `encode_wav_16k_mono` clamps `f32` samples, writes the RIFF header, and produces two-byte PCM samples ([wav.rs:8-37](../../crates/hark-stt/src/wav.rs#L8-L37)).

`parse_wav_16k_mono` walks chunks rather than assuming a fixed 44-byte header, then rejects any non-PCM16, non-mono, or non-16 kHz input. Gemini's batch fallback uses it to recover samples from the same WAV contract ([wav.rs:54-118](../../crates/hark-stt/src/wav.rs#L54-L118), [gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)).

Live sessions bypass the WAV container and accept already-resampled `f32` samples. This preserves a single sample contract while avoiding container work during the hold. Finished-clip peak normalization cannot be reproduced in a stream because the peak is unknown until release; the pipeline logs any gain the live path therefore skipped ([Architecture](../core/ARCHITECTURE.md#the-release-to-inject-pipeline)).
<!-- END:AUTOGEN hark_07_transcription_wav -->

---

<!-- BEGIN:AUTOGEN hark_07_transcription_errors -->
## Errors and Metrics

`SttError` distinguishes transport, authentication, rate limiting, timeout, bad-audio, and provider failures. Adapter diagnostics use trusted labels, status codes, and structural information without keys, authorization headers, audio, transcript text, vocabulary, or response snippets ([error.rs](../../crates/hark-stt/src/error.rs)).

HTTP 401/403 errors retain only an allowlisted machine-readable reason, 429 retains seconds-form `Retry-After`, and other unsuccessful responses retain the HTTP status. Transport errors use fixed categories, never the underlying error string or URL; connect-class failures retain the stable `connect failed` prefix used by the retry predicate. Malformed JSON reports only its error category and numeric line/column, because a parser's full message can quote user text. These rules also apply to fused Gemini responses and Deepgram meeting final-pass parsing ([error mapping](../../crates/hark-stt/src/error.rs), [Gemini parsing](../../crates/hark-stt/src/gemini.rs), [meeting adapter](../../crates/hark-stt/src/meeting.rs), [privacy regressions](../../crates/hark-stt/tests/error_privacy.rs)).

Gemini Live reports WebSocket error categories, HTTP status, I/O kinds, and numeric close codes without copying request URLs or server close reasons. Unexpected message diagnostics retain known protocol field names but replace unknown keys with `<unknown>`, preventing arbitrary JSON keys from becoming logged content ([gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)).

`request_ms` covers the adapter's observed request/session time. Pipeline history separately records full release-to-inject time, so provider latency and product latency remain distinguishable.
Live finalization requires `generationComplete` or `turnComplete`. A socket
close or EOF before that marker is an error even if some final transcript
segments arrived, allowing the pipeline to replay the full preserved recording
within its existing retry budget. A frame containing both final text and the
completion marker retains both. The intentional quiet-meeting timeout still
returns an empty result; a disconnected meeting socket does not count as silence
([finalizer and local WebSocket regressions](../../crates/hark-stt/src/gemini_live.rs)).
<!-- END:AUTOGEN hark_07_transcription_errors -->

---

<!-- BEGIN:AUTOGEN hark_07_transcription_gotchas -->
### Gemini Files meeting adapter

`hark-stt::meeting_gemini` is a blocking adapter for explicitly selected post-call
processing. It uploads one mono window, requests a structured transcript, and
attempts remote deletion on every result. It uses a reused client with redirects
disabled, the same static trust roots as the shared client, bounded response bodies, credential-free errors, and no global runtime.
Window-local speaker identity and conservative omission checks are described in
[Meetings](MEETINGS.md#gemini-files-final-pass). It does not implement `SttProvider`
or replace Gemini Live dictation
([adapter](../../crates/hark-stt/src/meeting_gemini.rs),
[window worker](../../crates/hark-pipeline/src/meeting/gemini_final.rs)). Its
tests run the full upload, finalize, inference, and DELETE sequence against a
local mock server. That server switches each accepted socket back to blocking:
on Windows an accepted socket inherits the listener's non-blocking mode, and
the tests failed at random with `WouldBlock` until it did.

## Provider Gotchas

- Never log provider configuration with a derived `Debug`; `ProviderConfig` redacts the key explicitly ([config.rs:25-66](../../crates/hark-stt/src/config.rs#L25-L66)).
- Whisper-compatible `prompt`, OpenAI `keywords[]`, Deepgram `keyterm`, and Gemini vocabulary are different wire contracts. Do not merge their adapters because their URLs happen to look similar.
- `Transcript::cleaned` is not a second independent Gemini transcript in Smart mode. It mirrors `text` and acts as the skip-cleanup flag; use Verbatim when a guaranteed literal record is required ([gemini_live.rs](../../crates/hark-stt/src/gemini_live.rs)).
- A live failure may replay through batch only within the single retry budget. Authentication and rate-limit failures do not retry ([Architecture](../core/ARCHITECTURE.md#retry-and-latency-discipline)).
- Feature-gating removes Gemini WebSocket support cleanly: batch adapters remain blocking, and `build_live` returns `None` without the `live` feature ([lib.rs](../../crates/hark-stt/src/lib.rs)).
- The meeting final pass and the dictation `SttProvider`s are unrelated adapters that happen to share a base URL: `hark-stt::meeting` builds its own request and has no `SttProvider` impl, so it is not selectable as a dictation provider and is never on the release-to-inject hot path.
<!-- END:AUTOGEN hark_07_transcription_gotchas -->

---
