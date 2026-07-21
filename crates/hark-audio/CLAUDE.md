# hark-audio rules

- **Never block, allocate, lock, or syscall in the cpal input callback.** The
  callback may only call `Producer::push*` (relaxed atomic stores plus one
  release store). cpal #970: pushing into channel/ring types that take locks
  or allocate can silently stop the stream with no error. Validate any change
  to the callback path under real capture on target hardware.
- **The capture thread owns its COM apartment.** The cpal stream is built and
  kept alive on the dedicated `hark-audio-capture` thread. Never build or own
  the stream on the UI thread, the hook thread, or the pipeline worker:
  WASAPI COM init modes conflict (`RPC_E_CHANGED_MODE`).
- **WASAPI does not resample for you and rarely offers 16 kHz.** Capture at
  the device default rate (usually 48 kHz f32) and resample per clip with
  `resample::resample_to_16k`. Whole-clip resampling must go through rubato's
  `process_all()` (trims FFT startup delay, exact `ceil(len * ratio)` output);
  a single oversized `process()` call leaves leading silence and truncates
  the tail.
- **`SampleFormat::F32` is required explicitly.** Phase 1 has no integer
  conversion path; a device with no f32 config is a clear startup error.
- **Loudness is judged by the loudest 100 ms window, never a whole-clip mean.**
  An assembled clip is always padded with pre-roll and tail, so a mean falls as
  the *proportion* of silence rises: short utterances score lower than long ones
  spoken at the same level, and the gate ends up strictest on exactly the short
  commands push-to-talk exists for. This was a real shipped bug (users reported
  having to lean into the mic). Any new statistic over a clip must be
  length-independent — `window::peak_window_rms`, not `window::rms`.
- **The loudness gate is biased toward passing, deliberately.** A false pass
  costs one transcription request; a false drop is the app silently doing
  nothing, which no user can diagnose. The absolute threshold and the
  above-the-room test are OR'd, never AND'd. Do not "tighten" this to save
  spend without weighing that asymmetry.
- **Multi-channel input takes channel 0, never an average.** Array mics
  commonly ship a near-silent reference channel, and averaging speech with
  silence costs 6 dB.
- **Normalization is boost-only.** Audio that already works must come out
  byte-identical; `gain` only lifts quiet clips, and never past the clipping
  or noise ceilings.
- **Tests assert sample counts, never wall-clock timings.** Pure modules
  (`ring`, `resample`, `window`) must stay hardware-free; `capture_win.rs` is
  the only file allowed to touch cpal.
- **Debug impls must never dump samples** (`AudioClip` prints lengths only).
