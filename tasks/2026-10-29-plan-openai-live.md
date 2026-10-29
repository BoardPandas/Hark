# Plan — OpenAI Live replaces Groq

**Created:** 2026-10-29
**Status:** APPROVED by the product owner on 2026-10-29 ("Approve as written"), including D2
(migrate Groq to `openai-compatible`, copying the key into an empty slot) and D4 (Minor,
0.64.0). CP0 returned GO on 2026-10-29; the results are in §5 Foundation.

**Core and Polish were implemented on 2026-10-29** as 0.64.0. An independent review found
four MEDIUM and four LOW issues, and all eight are fixed:

- **Empty transcripts:** an empty streamed transcript now falls back instead of counting as
  success.
- **Socket reads:** the drain no longer uses `timeout(ZERO)`, which costs about 15 ms per drain
  on Windows.
- **Fallback:** it is now REST `gpt-transcribe` (`batch_stt_route`), not a replay over the same
  socket.
- **Meetings:** meeting chunks take the batch route.
- **Keychain carry:** the copy is condition-based, so it is retried.
- **Commit matching:** a `committed` event counts only after our own commit.
- **Send errors:** a failed send now reports the server's refusal.
- **Checkbox:** it now reflects `uses_live_transcribe()`.

**Current step:** Ship. The owner tests real dictation on Windows, then commits and releases.
Nothing is committed yet.
**Origin:** product owner, 2026-10-29: "drop groq completely, replace it with an OpenAI-Live
option", with the live path "resampled straight from the device rate to 24 kHz". Revised the
same day: "a Live Transcribe toggle under the existing OpenAI option that way API Key does not
need re-entered".
**Research:** `.claude/agent-memory/explorer/hark_openai_realtime_transcription.md` (wire
contract, 2026-10-29). The code map is summarized in §4.

---

## 1. Goal

1. **Remove Groq** as both a speech-to-text provider and a cleanup provider. Existing configs
   that name it must keep loading.
2. **Add a "Live transcribe" toggle under the existing OpenAI provider** (`[provider] kind =
   "openai"`, `live = true`). It uses the same provider and the same keychain entry, so the key
   is never entered twice. When the toggle is on, Hark uses `gpt-live-transcribe`: it opens a
   realtime WebSocket on key-down, streams audio while the key is held, and commits on release.
   Only the tail is left between release and text. When it is off, OpenAI behaves as it does
   today (`gpt-transcribe` batch).
3. **The live stream is resampled from the device rate straight to 24 kHz**, which keeps the
   8–12 kHz band. Everything else stays 16 kHz.
4. The replay path (Test connection, the batch fallback when the live pump drops) upsamples the
   stored 16 kHz clip to 24 kHz.

## 2. Why

- Batch adapters upload the whole clip after release. A live session leaves only the tail and
  the commit, and latency is the product.
- OpenAI documents `gpt-live-transcribe` as the model to start with for realtime transcription.
  The REST multipart endpoint does not accept it, which is why Hark excludes it today
  (`hark-config/src/lib.rs:202`).
- Groq's 10 s billing minimum and its Whisper-only 224-token prompt make it the weakest option
  on the list.

## 3. Decisions

| # | Decision | Recommendation | Status |
|---|---|---|---|
| D1 | Groq is removed from STT **and** cleanup ("completely") | Yes | Assumed from the request |
| D2 | What a saved `kind = "groq"` becomes | Migrate to `openai-compatible` with Groq's base URL and model written out explicitly. Copy the `groq` keychain secret into the `openai-compatible` slot only if that slot is empty. Existing installs keep working with no named Groq option | **Owner to confirm.** The alternative is to reset to Deepgram and show "add a key" |
| D3 | OpenAI Live's API key | The toggle stays inside the OpenAI provider (`kind = "openai"`), so the existing `openai` keychain entry is used. No new provider kind and no new key slot | **Decided by owner, 2026-10-29** |
| D4 | Version bump | **Minor (0.64.0).** A provider is removed, but D2 migrates it, so no install breaks | **Owner to confirm.** Never Major without asking |
| D5 | Where the toggle sits | The picker becomes Deepgram, OpenAI, Gemini, OpenAI-compatible (Groq removed, no new row). With OpenAI selected, a "Live transcribe" checkbox appears under the radios, the same way Gemini's Verbatim/Smart row does (`form.rs:123`). A one-line cost note sits next to it. While it is on, the model box is hidden, because the live model is fixed | **Decided by owner, 2026-10-29** (layout: engineer) |
| D6 | Cargo feature | Rename `hark-app`'s `gemini-live` to `live-stt` and keep `gemini-live = ["live-stt"]` as an alias. Both live adapters share `hark-stt/live` | Engineer |
| D7 | `delay` setting | **`xhigh`**, hard-coded, with no new user setting. CP0 (n=1 per value): `medium`, `high` and `xhigh` reached 4% WER, the same as batch gpt-transcribe, while omitted/`minimal`/`low` reached 9%. Commit-to-final was 419–731 ms with no trend by delay. Hark injects only the final text, so earlier partials buy nothing. Re-check during the owner's dictation test | Engineer, 2026-10-29 |
| D10 | Wait for `session.updated` before appending? | **No.** Send `session.update`, then append straight away. Client events are processed in order on one connection, and in CP0 the ack took about 420 ms, which a short hold would pay at release. An `error` that arrives before `session.updated` fails the session, and the pump falls back to batch replay. This is UNCONFIRMED: add `PROBE_NO_WAIT=1` to the probe and verify it in the Ship run | Engineer, 2026-10-29 |
| D8 | Cleanup for OpenAI Live | Verbatim transcript (`cleaned: None`). The user's cleanup voice runs as usual, and cleanup inheritance treats it like OpenAI | Engineer |
| D9 | Default and routing | The toggle defaults to **off**, so existing OpenAI installs keep `gpt-transcribe` batch until the user opts in. OpenAI routes to the live adapter when `live = true` **or** a hand-edited `model` starts with `gpt-live-` (the contract belongs to the model, as with `gpt-transcribe`). A pinned batch model is kept and comes back when the toggle goes off | Engineer |

## 4. Architecture

### 4.1 Wire contract (from research; anything UNCONFIRMED gets probed in CP0)

- `wss://api.openai.com/v1/realtime?intent=transcription`, with `Authorization: Bearer <key>`.
  No `model` in the URL and no `OpenAI-Beta` header (GA).
- Send `session.update` with `session.type = "transcription"`, `audio.input.format =
  {type: "audio/pcm", rate: 24000}` (**24 kHz is the only accepted rate**) and
  `audio.input.transcription = {model, keywords[], languages: ["en"], delay}`.
  `turn_detection` is `null`, because VAD is unsupported and push-to-talk decides the turn.
- Audio goes in `input_audio_buffer.append {audio: base64 PCM16 LE mono}`. The turn ends with
  `input_audio_buffer.commit` and the final transcript arrives in
  `conversation.item.input_audio_transcription.completed {item_id, transcript}`, matched on the
  `item_id` from `input_audio_buffer.committed`. Failures arrive as
  `...transcription.failed {error}` or a top-level `error` event.
- Keywords must be one line each, with no `<`, `>`, CR or LF. Hark sanitizes them before sending,
  because a single bad term rejects the whole update.

### 4.2 Code changes by crate

**hark-audio (`resample.rs`)**
- Add `StreamResampler::with_target(src, dst)` and `resample_to(samples, src, dst)`, and give
  `resampled_len` a target argument.
- `new(src)`, `resample_to_16k` and a 16 kHz `resampled_len` stay as wrappers, so the meeting
  recorder and `assemble_window` do not change.
- Passthrough happens when `src == dst`.

**hark-stt**
- `LiveStt` gains `fn input_rate(&self) -> u32 { 16_000 }`. `LiveSession::push(samples)` is
  documented as taking samples at the provider's declared rate, and the parameter loses its
  `_16k` name.
- A new `openai_live.rs` follows `gemini_live.rs`'s layout:
  - Pure half, always compiled: URL, `session.update` builder, keyword sanitizer, append/commit
    messages, PCM16 encoding, server-event classifier, `item_id` matching.
  - `#[cfg(feature = "live")] mod session`: connect via `into_client_request()` with the
    Bearer header inserted, wait for `session.updated`, then send and drain interleaved.
  - It shares a private `current_thread` runtime, the same as Gemini.
  - `transcribe(wav)` does the replay: parse the 16 kHz WAV, `resample_to(.., 16_000, 24_000)`,
    stream, commit, finalize.
  - `input_rate()` returns 24_000.
- An internal adapter kind `hark_stt::ProviderKind::OpenAiLive` gets arms in `build` (replay)
  and `build_live` (streaming). It is an adapter routing value only. Users never see it, and
  config never stores it.
- `examples/openai_live_probe.rs` (`required-features = ["live"]`) connects and dumps raw
  frames with timings (LL-G `websocket-binary-frames-not-text`).

**hark-pipeline**
- `stream.rs:98` becomes `StreamResampler::with_target(rate, live.input_rate())`, read in
  `LiveTurn::start`.
- `provider_config` maps `kind = "openai"` with `uses_live_transcribe()` to
  `hark_stt::ProviderKind::OpenAiLive` and the model `gpt-live-transcribe`. It drops the Groq
  arms.
- In a build without the `live` feature, a `live = true` config falls back to `gpt-transcribe`
  batch, with one warning logged. It does not fail every dictation.
- The primary on-device gate (`lib.rs:543`, `mode.uses_cloud()`) already covers any
  `build_live` adapter. A test pins that for OpenAI Live.

**hark-config**
- Remove `ProviderKind::Groq`. No new kind is added.
- Add `Provider.live: bool` (`#[serde(default)]`, skipped on serialize when false). It mirrors
  `live_mode` with the doc comment "OpenAI only; ignored by every other provider".
- Add `Provider::uses_live_transcribe()` per D9. The keychain account stays `kind.label()`, so
  the key needs no change at all.
- Add a `CONFIG_VERSION` 6 → 7 migration with the existing versioned `.bak` backup. It does a
  text-level fix-up of `kind = "groq"` in `[provider]` and `[voice.provider]` before
  `toml::from_str`, per D2.
- Drop Groq's voice presets (`voice.rs:116-149, 308`).
- Update the `uses_keywords_biasing` doc comment, since `gpt-live-transcribe` now reaches an
  adapter, just not the multipart one.

**hark-app**
- `form.rs:68-79`: remove the Groq row.
- `form.rs:123`: a `live_transcribe_row` shown when `kind == Openai` and
  `cfg!(feature = "live-stt")`. It holds a checkbox bound to `draft.provider.live`, a cost note
  ("Streams while you hold the key. Billed per audio minute, about 4× gpt-transcribe."), and
  hides the model field while the box is ticked.
- The row is a small widget beside `live_mode_row`, because `form.rs` is near the 300-line UI
  module guideline. Split if it crosses it.
- `cleanup.rs:50`: remove the Groq row.
- `test.rs:96-105`: remove the 10 s minimum note.
- Feature rename per D6.

**hark-keychain** — the only change is the D2 copy from the `groq` account to
`openai-compatible`. It runs once, during migration, and is never logged. The toggle needs
nothing here.

**Docs** — `CLAUDE.md` (stack table; the Gemini-only runtime exception becomes "live
WebSocket adapters"; drop the two Groq gotchas), `AGENTS.md`/`agents.md`, `README.md`,
`.claude/rules/rust.md`, `config/default-config.toml`, `Docs/GETTING_STARTED.md`,
`Docs/OVERVIEW.md`, `Docs/core/CONFIGURATION.md`, `Docs/features/TRANSCRIPTION.md`,
`Docs/features/SPELLBOOK.md` and `Docs/features/VOICE_CLEANUP.md`. Historical `tasks/` and
CHANGELOG entries stay as they are.

## 5. Phases

### Foundation — CP0: live probe (about 1 h to write, about 15 min for the owner to run)

1. Write `openai_live_probe.rs`. It reads `OPENAI_API_KEY` from the environment, never prints
   it, and streams the 10 s fixture upsampled to 24 kHz, both paced in real time and burst.
2. The owner runs it on Windows. It records:
   - the frame type (text or binary)
   - whether deltas arrive before commit
   - commit-to-`completed` latency for each `delay` value
   - the minimum commit length (50 / 100 / 200 ms)
   - the error text for a 16 kHz rate
   - whether `delay` is accepted at all
   - the error shape for a bad key
3. The owner opens one idle socket for 60 s, then checks the usage dashboard for idle billing.
4. **Exit criteria.** GO if `completed` arrives reliably, and release-to-text p50 beats
   gpt-transcribe batch on the same fixture (measure it with `transcribe_spike`). Otherwise
   stop and report.

**CP0 result, 2026-10-29: GO.** The owner ran it on Windows (debug build, 10.3 s fixture,
paced in real time).

- **Frames:** every server frame was **text**, never binary. The adapter still accepts both.
- **Setup:** `session.created` arrives about 10 ms after connect, unprompted, and
  `session.updated` about 420 ms after the update is sent (see D10).
- **Streaming:** **deltas stream during the hold**, about 30 per turn before commit and
  roughly 0.5–1.5 s behind the speech.
- **Commit and final:** commit is answered by `input_audio_buffer.committed` (with
  `item_id`), then `conversation.item.added`, then `conversation.item.done`, then
  `...transcription.completed`.
- **Final latency:** commit to `completed` took **419 / 432 / 452 / 553 / 575 / 731 ms**
  (p50 about 500 ms). gpt-transcribe batch, warm, took **667 ms** for the same clip.
  **Live beats batch by roughly 150–250 ms on a 10 s clip.** The gain shrinks on short
  clips, where the batch upload is smaller.
- **Accuracy:** WER was 4% at `medium`/`high`/`xhigh`, 9% at omitted/`minimal`/`low`, and 4%
  for batch. `delay` is **accepted** for `gpt-live-transcribe`, so the API reference's
  "whisper-only" note is stale.
- **Minimum commit:** **100 ms.** Below that the server returns `error` with
  `code: "input_audio_buffer_commit_empty"` ("Expected at least 100ms of audio"). Exactly
  100 ms is accepted and returns `""`.
- **Rate:** `rate: 16000` fails `session.update` with `integer_below_min_value` ("Expected a
  value >= 24000").
- **Bad key:** the **handshake succeeds (HTTP 101)**. The refusal arrives as an `error`
  event with `code: "invalid_api_key"`, followed by a close frame with code `3000` and reason
  `invalid_request_error.invalid_api_key`. The message quotes a **masked key**, so the adapter
  scrubs `sk-...` fragments as well as the key itself.
- **Billing:** `usage` on `completed` was `{"type": "duration", "seconds": 11}` for a 10.3 s
  clip over a ~14 s session, so billing follows appended audio (rounded up to whole seconds),
  not socket time. Idle billing is **inferred from this, not measured**, because
  `PROBE_IDLE_SECS` was not run.
- **Connect time varied from 261 ms to 6408 ms** (debug build, so rustls is unoptimized).
  The connect overlaps the hold, but a short dictation on a slow connect would be faster
  through batch. See §7.

### Core (about 1 day)

1. hark-audio: rate-parameterized resampler plus tests (§6).
2. hark-config: Groq removal, `Provider.live` and `uses_live_transcribe()`, v7 migration plus
   tests.
3. hark-stt: `LiveStt::input_rate`, `openai_live.rs` (pure half first, then the session half),
   `build`/`build_live` arms.
4. hark-pipeline: `stream.rs` target rate, `provider_config` mapping, gate test.
5. hark-app: remove the Groq rows, add the OpenAI "Live transcribe" toggle row with its cost
   note, feature rename.
6. Groq clean-up in examples, tests and other crates (`hark-voice` included: confirm its 5 files
   before editing).

### Polish (about half a day)

1. Handshake 401/403/429 go through the existing auth-error formatting (status plus provider
   reason code).
2. History labels the model `gpt-live-transcribe`.
3. Docs per §4.2, plus a CHANGELOG entry at the D4 version.

### Ship

1. `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`,
   `npm run check:claude`.
2. The owner does real dictation on Windows: short, long, quiet mic, a spellbook term, Test
   connection, and the fallback (pull the network mid-hold and confirm the batch replay
   recovers the dictation).
3. Commit (CHANGELOG and package.json staged first).

## 6. Testing

- **Resampler:** 16k→24k and 48k→24k produce exact output lengths. Streaming in small chunks
  matches whole-clip output at the 24 kHz target, the same as the existing 16 kHz test.
  Passthrough at 24k→24k.
- **openai_live pure half:**
  - the exact `session.update` JSON
  - the keyword sanitizer (strips `<>` and CR/LF, drops empty terms)
  - PCM16 LE base64 encoding
  - classifying committed, delta, completed, failed and error
  - `completed` is matched on `item_id` and ignores another turn's `completed`
  - text and binary frames are both parsed
- **Privacy:** no error string, `Debug` impl or log line contains the key or the
  `Authorization` header. Copy Gemini's tests at `gemini_live.rs:859+`.
- **Config:**
  - a `kind = "groq"` file migrates to `openai-compatible` with the explicit URL and model
  - the `config.toml.v6.bak` backup is byte-exact
  - a second load is a no-op
  - `[voice.provider]` migrates the same way
  - `live` defaults to false, round-trips when true, and is not written when false
- **Pipeline:**
  - OpenAI with `live = true` maps to the live adapter with `gpt-live-transcribe`
  - OpenAI with `live = false` still maps to `gpt-transcribe` batch
  - a hand-edited `model = "gpt-live-transcribe"` maps to live (this replaces today's
    `gpt_live_transcribe_never_reaches_a_batch_adapter_contract` test)
  - `live = true` on any other kind is ignored
  - primary local mode builds no live adapter
- **UI:** the Groq row is gone and OpenAI-compatible stays last. The toggle row appears only
  for OpenAI with the feature on.

## 7. Risks

| Risk | Mitigation |
|---|---|
| The wire contract differs from the docs (Gemini shipped with three such bugs) | CP0 probe against the live API before any adapter code is trusted. **Done 2026-10-29: GO** |
| Slow connects (CP0 saw up to 6.4 s) make a short dictation slower than batch | The 4 s connect bound already used by Gemini, after which the pump drops and batch replay runs. Re-measure in a release build during Ship. If p95 is still poor, a pre-warmed socket per dictation is an option, because CP0's usage figure points to audio-only billing |
| A hold under 100 ms after pre-roll and tail | The adapter pads the committed buffer with silence up to 100 ms (2,400 samples at 24 kHz), so commit never errors. The server then returns `""`, which injects nothing |
| Idle socket time is billed (we open on key-down) | Measure in CP0. If it is billed, open on key-down anyway: holds are 2–15 s, so the cost is bounded. Note it in the cost hint |
| Deltas only arrive after commit | Still a win, because upload overlaps the hold. CP0 measures the real gain |
| `delay` is rejected for this model (the reference says it is whisper-only) | Omit the field if CP0 shows rejection |
| About 3.8× the price of gpt-transcribe | The cost note in Settings. OpenAI (batch) stays the cheaper OpenAI option |
| The D2 key copy overwrites a key | Copy only into an empty slot. A test covers it |
| `form.rs` also changes under the approved intent `new-users-cannot-choose-a-transcription-setup` | Whichever lands second rebases. Do not run both in parallel on `form.rs` |

## 8. Lessons Learned / Gotchas (pre-seeded; extend after CP0 and route to LL-G via `/add-lesson`)

- **LL-G Rust HIGH `websocket-binary-frames-not-text`:**
  - accept JSON in both text and binary frames
  - read while writing
  - accept both the documented and the observed terminator
  - log unknown frames by their top-level keys
  - put frame counts in timeout errors
  - keep the probe committed
- **LL-G Rust HIGH `blocking-io-on-tokio`:** no blocking reqwest or fs on the adapter's
  runtime. It stays private and `current_thread`.
- **LL-G Rust HIGH `rubato-4-whole-clip-process-all`:** the replay upsample uses
  `process_all`, and the stream uses one stateful `StreamResampler` per utterance, never one
  per chunk.
- **LL-G Rust HIGH `reqwest-0132-static-roots-tls-certs-only`:** the WebSocket TLS must keep
  static webpki roots (`tokio-tungstenite` `rustls-tls-webpki-roots`), never the OS store.
- **BP `versioned-config-migration-backup`:** a retired value must still load, with a
  version-stamped backup written before the migration is saved.
- **Unknown `kind` strings fail the whole config parse today** (`ProviderKind` has no
  `#[serde(other)]`). Removing a variant without a migration silently resets the user to
  defaults behind a banner, and the next Save overwrites their file.
- **24 kHz is the only PCM rate `gpt-live-transcribe` accepts**, unlike Gemini Live's 16 kHz.
  The live input rate is per provider, not a pipeline constant. CP0 confirmed this: "Expected
  a value >= 24000".
- **CP0, OpenAI realtime: a bad key does not fail the WebSocket handshake.** It is accepted
  (101), and the refusal is an `error` event plus close code 3000. An adapter that checks only
  the handshake reports a bad key as a mid-session failure. The error text also quotes a
  masked `sk-...` key, which must be scrubbed.
- **CP0, OpenAI realtime: commit needs at least 100 ms of buffered audio**
  (`input_audio_buffer_commit_empty`).
- **CP0: OpenAI's realtime API reference said `delay` is whisper-only, and that was wrong.**
  The transcription guide was right. Probe before trusting either page.
- **Review: `tokio::time::timeout(Duration::ZERO, socket.next())` is not a free non-blocking
  read.** The zero deadline rounds up to the next 1 ms timer tick, and the runtime parks to
  fire it, which Windows stretches to about 15.6 ms. 100 empty drains took 1.5 s. Use
  `yield_now()` followed by `now_or_never()`. `gemini_live.rs` still has the old pattern,
  tracked as a follow-up. Candidate for LL-G.
- **Review: a live adapter's fallback must not share its transport.** Replaying over the same
  realtime socket loses the dictation to a WebSocket-blocking proxy, an outage, or one
  rejected keyword. Fall back over REST with the same key.
- **Groq migration: inherited settings must be written out.** A Groq install with no
  `[voice.provider]` inherited Groq cleanup. Converting only `[provider]` would have turned
  cleanup off silently, because `openai-compatible` does not inherit.
