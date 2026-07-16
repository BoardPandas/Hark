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
- **Tests assert sample counts, never wall-clock timings.** Pure modules
  (`ring`, `resample`, `window`) must stay hardware-free; `capture_win.rs` is
  the only file allowed to touch cpal.
- **Debug impls must never dump samples** (`AudioClip` prints lengths only).
