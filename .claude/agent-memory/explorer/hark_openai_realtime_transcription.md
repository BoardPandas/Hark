---
name: hark-openai-realtime-transcription
description: Wire contract for OpenAI Realtime WebSocket transcription with gpt-live-transcribe (URL, session.update, append/commit, events, errors, pricing); lists what is UNCONFIRMED.
metadata:
  type: reference
---

Researched 2026-10-29 for the Hark OpenAI realtime adapter (push-to-talk, no server VAD). Related: [[hark-cloud-stt-providers]].

Sources:
- https://developers.openai.com/api/docs/guides/realtime-transcription
- https://developers.openai.com/api/docs/models/gpt-live-transcribe
- https://developers.openai.com/api/docs/pricing
- https://developers.openai.com/api/reference/resources/realtime/client-events
- https://developers.openai.com/api/reference/resources/realtime/server-events
- https://developers.openai.com/api/docs/guides/realtime (GA header note)
- https://developers.openai.com/api/docs/guides/realtime-websocket (search snippet only; direct fetch returned 404 repeatedly)

## 1. Connection
- URL: `wss://api.openai.com/v1/realtime?intent=transcription`. Do NOT put `model` in the URL: the model URL creates a conversation session that rejects the transcription update. Model goes in session.update.
- Header: `Authorization: Bearer <key>` (WebSocket guide says "pass an authentication header using your OpenAI API key"). Realtime guide: "Remove the `OpenAI-Beta: realtime=v1` header when calling the GA interface." So do not send it.
- Optional `OpenAI-Safety-Identifier` header for trusted server connections.
- Subprotocols: UNCONFIRMED (none documented for server-side).
- A different `wss://api.openai.com/v1/live/sessions` + `session.start`/`session.started` flow exists for the conversational GPT-Live; gpt-live-transcribe page lists only the Realtime transcription endpoint, so it is NOT this.

## 2. session.update
```json
{"type":"session.update","session":{"type":"transcription","audio":{"input":{
  "format":{"type":"audio/pcm","rate":24000},
  "transcription":{"model":"gpt-live-transcribe","prompt":"...","keywords":["..."],"languages":["en"],"delay":"low"},
  "turn_detection":null}}}}
```
- PCM rate: reference says "Only a 24kHz sample rate is supported." Resample 16k -> 24k client side. (`audio/pcmu`, `audio/pcma` have no rate.)
- `turn_detection`: omit or null; server_vad / semantic_vad unsupported by the model.
- `keywords`: each one line, no `<`, `>`, CR or LF, else update is rejected. Max count/length UNCONFIRMED.
- `prompt`: rejected if over the model limit; limit not stated (UNCONFIRMED). Reference schema says prompt unsupported with gpt-realtime-whisper only.
- `languages`: array (minItems 1); not singular `language`; do not send both. ISO 639-1, some 639-3 (`spa`), `zh-hk` style. Unsupported codes rejected.
- `noise_reduction`: `{type: "near_field"|"far_field"}` or null (reference schema); not mentioned on the transcription guide. Whether gpt-live-transcribe honors it: UNCONFIRMED.

## 3. Append
- `input_audio_buffer.append` with `audio` = base64 PCM16 LE mono. Max 15 MiB per event. No server confirmation sent. Recommended chunk duration and rate limits: UNCONFIRMED (model page RPM/TPM tiers only).

## 4. End of turn
- `input_audio_buffer.commit`: errors if buffer empty. Success -> `input_audio_buffer.committed` {event_id, item_id, previous_item_id|null}, then `conversation.item.created`. Numeric minimum: reference gives none; community reports cite a "at least 100 ms" error text (unofficial). Treat >=100 ms as required.
- Transcript events: `conversation.item.input_audio_transcription.delta` {item_id, content_index?, delta, logprobs?}; `.completed` {item_id, content_index, transcript, usage (tokens or duration), languages? (gpt-transcribe only), logprobs?}; `.failed` {item_id, content_index, error{code,message,param,type}} (example code `audio_unintelligible`). Failed is separate from `error` events so you can tie it to an item.
- Reliable final signal: `.completed` for the item_id from `committed`. Match by item_id; ordering across turns not guaranteed; transcription is async relative to other events.
- Doc caveat: "transcript ... should be treated as a rough guide."
- Whether deltas arrive during the hold (before commit) for gpt-live-transcribe: UNCONFIRMED (model is "streaming"; guide says commit gets "the final transcript").

## 5. Connect events
- `session.created` {event_id, session, type} on connect; `session.updated` after a successful session.update. Docs do not say audio must wait for session.updated; safe approach: wait for `session.updated` (UNCONFIRMED requirement).

## 6. Errors
- `{"type":"error","event_id":"..","error":{"message","type","code","event_id","param"}}`; documented example code `invalid_event`. Specific codes for bad key / quota / bad rate / buffer too small: UNCONFIRMED. HTTP 401/403/429 on upgrade: UNCONFIRMED (expect an HTTP failure on handshake).

## 7. Frames
- JSON text frames: UNCONFIRMED in docs (events are JSON; assume text, and tolerate binary by parsing both).

## 8. Limits and billing
- Max session duration, idle timeout, concurrent sessions: UNCONFIRMED (not stated; community suggests idle drops 1005/1006). Billing is stated per audio minute ("Realtime audio"); whether idle open socket bills: UNCONFIRMED. `completed.usage` may be tokens or duration seconds.
- Rate limits (model page): Build 2,000 RPM / 210,000 TPM; Launch 10,000 / 600,000; Grow 10,000 / 780,000.

## 9. Pricing (official pricing page)
- gpt-live-transcribe: $0.017 / minute. gpt-transcribe: $0.0045 / minute.

## 10. delay
- Values `minimal`, `low`, `medium`, `high`, `xhigh`; higher = more accuracy, more latency; benchmark. Default UNCONFIRMED. The client-events reference says delay is "Only supported with gpt-realtime-whisper" which conflicts with the transcription guide listing it for gpt-live-transcribe; likely stale schema text, verify with a live probe.

## 11. Ephemeral tokens
- Ephemeral keys (`POST /v1/realtime/client_secrets`) are for browser/WebRTC clients; server-side use a standard API key. A desktop app holding the user's own key can connect directly with Bearer. Transcription guide: for browser audio use WebRTC.

## Live-probe checklist (given Gemini Live history)
Confirm: frame type, delta timing during hold, `.completed` arrival after commit, min commit length, idle behavior, 24 kHz enforcement error text, `delay` acceptance, writer must keep reading (session.created/updated/ack events) so the socket never stalls.

## Confirmed by live probe (CP0, 2026-10-29)

Probe: `crates/hark-stt/examples/openai_live_probe.rs`, run on Windows against the real API. Full numbers: `tasks/2026-10-29-plan-openai-live.md` §5 Foundation.

- Every server frame was a TEXT frame. None were binary.
- `session.created` arrives about 10 ms after connect, unprompted. `session.updated` arrives about 420 ms after the update.
- Deltas stream during the hold, before commit.
- After commit, events arrive in this order: `input_audio_buffer.committed` {item_id}, `conversation.item.added`, `conversation.item.done`, then `...transcription.completed`.
- Commit to `completed` took 419–731 ms, p50 about 500 ms. Warm gpt-transcribe batch took 667 ms on the same 10.3 s clip.
- `delay` is ACCEPTED for gpt-live-transcribe; the reference's "whisper-only" note is stale. `medium`, `high` and `xhigh` gave 4% WER; omitted, `minimal` and `low` gave 9%.
- The minimum commit is 100 ms. Below that the error code is `input_audio_buffer_commit_empty`.
- `rate: 16000` fails with `integer_below_min_value` ("Expected a value >= 24000").
- A bad key passes the handshake (101). The refusal is an `error` event with `invalid_api_key`, then close code 3000. The message quotes a masked `sk-...` key.
- `usage` was `{"type":"duration","seconds":11}` for 10.3 s of audio over a ~14 s session. That points to per-audio billing; idle billing was not measured directly.
