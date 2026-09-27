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
  (`ring`, `resample`, `window`) must stay hardware-free, and `spool` is tested
  against a temp dir only; `capture_win.rs` is the only file allowed to touch
  cpal.
- **Debug impls must never dump samples** (`AudioClip` prints lengths only).
- **Process loopback (`loopback_win.rs`): the activation `PROPVARIANT` stays in
  `ManuallyDrop`.** Its `VT_BLOB` points at stack memory, and windows-rs 0.62's
  `Drop` frees it: the process dies with no message (LL-G
  `propvariant-drop-frees-blob`). Its thread owns its MTA apartment like
  `capture_win`'s, with the apartment guard declared before any interface so
  it is dropped last. `GetMixFormat` is unsupported on that virtual device:
  pass the 16 kHz mono i16 format and `AUTOCONVERTPCM`.
- **Meeting spools convert with `spool::f32_to_i16`, never `* i16::MAX`.** It is
  the exact inverse of the `/ 32768` the loopback applies going into the f32
  ring, so loopback audio reaches the WAV bit-identical.
- **The D9 archive (`mp3.rs`) encodes with LAME `Mode::Stereo`, never
  `Mode::JointStereo`.** Joint stereo shares bits between channels and leaks
  one side into the other at about -96 dBFS, which would corrupt a re-run
  through Deepgram multichannel; plain stereo keeps the sides independent
  (`archive_keeps_channels_independent_not_joint_stereo` pins this).
- **Every `encode_to_vec`/`flush_to_vec` call reserves its buffer first**
  (`max_required_buffer_size(n)` before an encode, `7200` before a flush).
  Those helpers hand LAME the `Vec`'s spare capacity directly, and LAME reads
  a spare capacity of 0 as "unbounded" rather than "none available" (LL-G
  HIGH `kb/rust/mp3lame-encode-to-vec-no-reserve.md`) -- skip the reserve and
  it is a heap overflow, not a clean error.
