# Gemini final-pass specification

1. Add `FinalPass::Gemini` and nonblank `meeting.gemini_model` (default
   `gemini-3.8-flash`). Schema 5 adds defaults without switching an existing provider,
   shortcut, or intentional auto-stop value. The key is `HARK_GEMINI_KEY` or keychain
   account `gemini`, independent of the dictation provider/model choice.
2. On the existing finisher worker, decode the recording in bounded chunks and send
   each microphone/playback track separately in five-minute windows, including the
   partial tail. Apply spelling correction, order segments, keep microphone as Me,
   and display playback speaker labels as Window N · Speaker M throughout exports.
3. Use resumable Files uploads followed by structured Interactions with `store:false`.
   Preallocate a valid unique filename before audio upload, reject unsafe upload URLs
   and mismatched names, bound replies/timeouts, and attempt remote deletion on every
   post-upload result. Report cleanup failure; allow a single best-effort Drop retry.
   Do not log response bodies, audio, transcript text, keys, or upload locations.
4. Require valid completed responses, clamp segment timestamps, and conservatively
   reject conspicuous energy coverage gaps. Any failed window or wholly empty result
   keeps the live transcript. Checks are not a guarantee of complete or accurate
   transcription. Do not automatically fall back to another cloud provider.
5. Keep saved-recording re-runs explicitly Deepgram and retain their existing upload
   confirmation. Validate migrations, channel/window tails, omission rejection,
   window speaker identity, HTTP success/failure/lost-finalization cleanup, visible
   deletion failure, formatting, clippy, and workspace tests. Mock requests do not
   establish current live service behavior or native UI operation.

No AEC implementation, benchmark, generic all-six intent artifact, or provider
account/key mutation belongs to this snapshot. Source: [adapter](../../crates/hark-stt/src/meeting_gemini.rs),
[worker](../../crates/hark-pipeline/src/meeting/gemini_final.rs), and
[settings](../../crates/hark-config/src/meeting.rs).
