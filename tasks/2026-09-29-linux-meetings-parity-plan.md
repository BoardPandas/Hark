# Linux meetings parity — implementation plan and handoff

Date: 2026-09-29. Intent: `intent/linux-meetings-parity/intent.md` (see
`intent/linux-meetings-parity/`); spec:
`intent/linux-meetings-parity/spec.md`. Status: implemented and validated as
far as a Linux dev machine allows; delivered in the working tree.

## Order (as executed)

1. **Spike first.** A scratch crate against the live system answered the
   load-bearing PipeWire questions before any production code: monitor
   capture negotiates 16 kHz mono F32 exactly; targetless `capture.sink`
   fails ("no target node available") so the default sink must be resolved
   from metadata; a plain capture with `target.object` = a stream node's
   serial taps exactly that stream (proven with a two-sink setup); registry
   globals filter `application.*` props, so node identity needs a bind+info.
2. **`hark-audio`: `loopback/` module** (`mod.rs` contract unchanged, `win.rs`
   moved, `linux.rs` new). One thread owns the pw stack; substreams by key;
   timer tick applies queued add/dead; two-phase `mainloop.run()` (start
   gate, then capture lifetime); start timeout 5 s; metadata-driven monitor
   rebuild counts a discontinuity; Include falls back to the sink.
3. **`hark-meeting`: `probe/` module** (`win.rs`/`watch_win.rs` moved,
   `linux.rs`/`watch_linux.rs` new): snapshot binds `Stream/Input/Audio`
   nodes for identity, `/proc` process list, X11 titles via x11rb;
   `DEFAULT_APPS`/`BROWSERS` learn Linux binary names (no `.exe` collisions).
4. **`hark-hotkey`: `Mode::Shortcuts`** in `hook_linux.rs` behind the existing
   `spawn_shared_listener` seam; edges are collected per poll pass before
   dispatch (the dispatch path needs the devices immutably for the engage
   verification the Windows hook performs).
5. **`meetings_supported()` → Linux**, `app_display_name` Linux ids,
   coordinator `probe::` paths, unconditional watcher drop.
6. **Share parity:** zenity→kdialog save dialogs, Word export everywhere,
   FileManager1/xdg-open folder reveal, settings folder opener.
7. **CI/packaging/docs/notices/changelog** (+ `_meta` sync records).

## Verification performed

- Live, on a private `PIPEWIRE_RUNTIME_DIR` stack with WirePlumber and null
  sinks: `loopback_smoke` in both modes (≈16,000 frames/s each second,
  silence included; Include captured a targeted sine at −15.3 dBFS through
  the stream node only), `detect_smoke` (fake `zoom` holder → `Prompt`
  after the 5 s debounce; watcher woke 19 times on graph churn; probe ≈3 ms).
- `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --workspace`, `npm run check:claude`, `npm run check:docs`:
  all green.
- Not verifiable here: a real mic call on production hardware, Windows/macOS
  compilation (CI's job), and zenity/kdialog dialogs (neither installed on
  the dev box; both are graceful-fallback paths).

## Lessons Learned / Gotchas

- **pipewire-rs 0.10's published examples lag its API.** `StreamBox::State`,
  `keys::TARGET_OBJECT` (behind the `v0_3_44` feature), `Pod::from_bytes`
  returning `Option<&Pod>` (borrow, not owned), `RegistryRc::downgrade`,
  metadata `property` returning `i32` — compile against the crate source in
  `~/.cargo/registry`, not the repo's `main` examples.
- **A registered pw-rs listener unregisters when dropped.** Storing only the
  stream and letting the listener binding die leaves the stream permanently
  `Connecting` from our side while WirePlumber happily links it. Store
  `(StreamRc, StreamListener)` pairs (or proxy+listener boxes) together.
- **Registry globals filter `application.*`.** App identity (binary, pid) is
  only in the bound node's info props, and it takes a *second* roundtrip
  after binding to arrive. Both the loopback's Include matching and the
  detection probe depend on this.
- **`stream.capture.sink=true` ignores a stream-node `target.object`** — it
  routes to the sink's monitor. Per-app capture must be a *plain* capture
  with the stream node as target. Verify such claims with a two-sink setup,
  or the monitor's audio masquerades as a successful tap.
- **A `for` loop over `mem::take(&mut state.borrow_mut().…)` holds the
  RefCell borrow for the whole loop** (scrutinee temporary lifetime); hoist
  the take. Panics inside pw process callbacks abort the process — there is
  no unwind through the C frames.
- **bindgen needs libclang and GCC's include dir** on a dev box without the
  distro package: PyPI's `libclang` wheel plus
  `BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/<target>/<ver>/include"`. CI
  already installs clang; it only needed `libpipewire-0.3-dev`.
- **A headless `pipewire` daemon has no nodes and no metadata** (WirePlumber
  creates them, and it needs `/dev/snd` access). For testing: run a private
  `PIPEWIRE_RUNTIME_DIR` stack and create `support.null-audio-sink` sinks
  via the `adapter` factory (`factory.name` prop; the direct factory name
  does not exist) with `node.driver=true` and `object.linger=1`.
- **Metadata arrives per bound Metadata object** (several exist), so the
  default-sink handler fires more than once per change; the "rebuild"
  path must drop the old substream only when one is actually live, or the
  same tick removes what it just opened (found as the "no audio output to
  capture" failure).
