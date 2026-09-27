# hark-meeting rules

- **Pure: no I/O, no threads, no clocks.** Capture, spooling, STT and storage
  are the coordinator's job; this crate decides. Every offset is a count of
  16 kHz samples since the session start, and tests assert exact sample
  counts on synthetic PCM, never wall-clock time.
- **One `SessionState` machine per meeting.** A finished machine never
  restarts; back-to-back meetings get a new machine while the old one is
  still summarizing. `advance` is total: stray or duplicate events are inert,
  never a panic.
- **A failed or skipped final pass or summary is not a `Failure`.** The
  meeting keeps the live transcript (and loses only notes). `Failed` is only
  for "nothing to keep".
- **Chunks are judged by the loudest-window gate** (`hark_audio::window::gate_clip`),
  never a whole-chunk mean: one short answer in 30 s of listening is speech.
  Skipped chunks still advance the timeline.
- **Meeting content never reaches a log.** `Segment` and `Chunk` have
  hand-written `Debug` impls that print offsets and lengths only; do not
  derive `Debug` on anything holding text or samples.
