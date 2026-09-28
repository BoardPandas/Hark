# hark-meeting rules

- **Pure: no I/O, no threads, no clocks,** with two fenced exceptions:
  `storage_fs.rs` (measure and delete meeting audio) and `probe_win.rs` with
  its `probe_watch_win.rs` worker (read who holds the mic and notify changes).
  Every decision lives in the pure modules and is tested
  on fixtures; time is a caller-supplied ms counter and offsets are 16 kHz
  sample counts, never wall-clock time.
- **`storage_fs::delete_audio` is the only deletion, and its guard is the
  point:** a plain id the database knows, a real (not symlinked) direct child
  of `meetings/`. Never loosen it to delete "stray" entries; they are reported,
  not removed.
- **Detection never counts Hark itself** (its pre-roll stream holds the mic
  forever) and never logs or stores window titles.
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
