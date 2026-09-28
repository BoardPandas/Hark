# Meeting AEC bakeoff

This standalone experiment compares `aec3 = 0.4.0`,
`webrtc-audio-processing = 2.1.0` (bundled C++), and an unchanged-microphone
baseline. Its own `[workspace]` and lockfile keep the comparison independent of
Hark's package graph; running this tool never enables AEC in the app. Hark 0.57.0
separately selects Rust `aec3 = 0.4.0` for the optional **Reduce speaker echo**
meeting setting. The C++ candidate remains experimental. All processing here is
offline; nothing sends audio to a provider.

See [the recorded comparison](RESULTS.md) for the completed WSL run, raw
numeric evidence, and the limits of its quality measurements.

## Reproduce

Prerequisites for the **Linux/WSL x86_64 comparison**: Rust supporting edition
2024 dependencies, Python 3.10+, a C/C++ compiler, libclang, pkg-config, `cp`,
`nm`, and Rust's `rust-objcopy` (normally the `llvm-tools-preview` component).
The script reports missing prerequisites through the normal build failure; it
does not install system packages. Obtain approval before changing the machine's
toolchain or system packages.

Run from this directory in WSL:

```sh
python3 build-wsl.py test --release --locked --all-features --jobs 2
python3 build-wsl.py clippy --release --locked --all-features --all-targets --jobs 2 -- -D warnings
python3 build-wsl.py run --release --locked --all-features -- list
python3 build-wsl.py run --release --locked --all-features -- bench results/run-01 3
```

From PowerShell, prefix the equivalent commands with
`wsl.exe -d Debian -- python3 /mnt/c/repos/Hark/tools/meeting-aec-bakeoff/build-wsl.py`.
The script chooses this directory as Cargo's working directory.

`build-wsl.py` downloads fixed Meson 1.12.1 and Ninja 1.13.2 wheels from PyPI's
file host, verifies their embedded SHA256 hashes, and extracts them under
`target/build-tools/`. No pip, apt, elevated access, or persistent PATH change
is used. Cargo output goes to `target/cargo/` unless `CARGO_TARGET_DIR` is set.
Meson may download the upstream-pinned Abseil fallback while building
WebRTC; first builds therefore need internet access. Cargo dependencies and
their checksums are pinned in `Cargo.lock`.

The local Meson launcher explicitly requests `--buildtype=release` and
`-Db_ndebug=true`. **The bundled `meson.build` defaults to `debugoptimized`
(O2), independently of Cargo's `--release`.** Ninja is capped at two jobs. Wait
for compilation and other heavy checks to finish before timing. Record CPU,
toolchain, target, power mode, and background load when comparing runs.

To build only the pure Rust candidate or the baseline with regular Cargo:

```sh
cargo test --manifest-path Cargo.toml --release --locked --features rust-aec3
cargo run --manifest-path Cargo.toml --release --locked --features rust-aec3 -- bench results/rust-only-01 3
```

The default feature set has only the bypass baseline. Feature selection is
explicit, and `list` shows the actually compiled engines.

## Inputs and metrics

Every engine receives identical mono float32, 16 kHz, 160-sample (10 ms) frames.
Each iteration analyzes render before capture, with a fresh engine for each
fixture/repetition. AEC and high-pass filtering are enabled; separate noise
suppression, AGC, and the Rust pipeline's extra post-filter are disabled.
The default benchmark supplies **no oracle delay** to either engine.

Seven deterministic fixtures cover these cases:

| Fixture | Duration | Scored interval | Purpose |
| --- | ---: | ---: | --- |
| Far only, 40 ms echo | 30 s | 20–30 s | Steady-state echo attenuation |
| Far only, 160 ms echo | 30 s | 20–30 s | Longer delay acquisition |
| Near only | 30 s | 20–30 s | Preserve the local speaker |
| Double talk, 40 / 160 ms echo | 30 s each | 12–23 s | Preserve near audio during overlapping render |
| Far only, +31 / −31 ppm drift | 120 s each | 110–120 s | Gradual device-clock mismatch |

The synthetic source mixes independently seeded colored noise with modulated
harmonics and a changing envelope. The echo path is a fixed four-tap linear
room response. Double talk begins after eight seconds of far-only adaptation.
The ±31 ppm cases are inspired by a Hark development observation, not a limit
on real hardware drift. At 120 seconds they accumulate only 3.72 ms of drift;
they do not establish hour-long stability. This set does not model nonlinear speakers, moving
participants, clipping, packet loss, or all device changes.

`results.csv` and `results.json` contain repeated measurements and input
fingerprints. The input and first output WAVs are retained beside them for
exact replay. The FNV1a64 input fingerprint checks accidental differences; it
is not a security hash. Floating-point synthesis may differ across targets,
so compare the saved WAVs/fingerprints when reproducing on another platform.

- **ERLE (dB)**: microphone energy / processed energy during a far-only
  interval. Higher means less residual echo; total muting can also score well.
- **Near SNR (dB) and projected near gain (dB)**: compare output with the known
  clean synthetic near source. Reported output alignment searches 0–20 ms;
  this diagnostic compensates algorithmic lag but never shifts the input
  frames fed into AEC. Gain near 0 dB and improved SNR support preservation.
- **Frame time (p50/p95/p99/max, microseconds) and real-time factor**: wall time
  inside the engine's render-plus-capture calls, including wrapper overhead.
  Excludes WAV IO, metric calculation, fixture creation, and engine startup.
- **Peak and alignment samples**: help expose unstable output and unexpected
  lag. All frames must be finite, and a backend producing no output fails
  the run instead of substituting fabricated silence.

These are diagnostic signal metrics, **not intelligibility or subjective
speech-quality scores**. The bypass control makes a broken metric easier to
spot. Neither high ERLE nor low CPU alone identifies the production choice.
Power ratios floor zero energy at `1e-30` so reports stay finite; the very high
SNR of a bit-identical near-only bypass is a numerical limit, not a claimed
physical recording dynamic range.

## Replay a real speaker recording

This remains an optional follow-up for evaluating the intended Windows
microphone and speakers. The production Rust choice was explicitly accepted
without further speakerphone testing; that decision does not establish real
speakerphone quality. Keep recordings local and use a short, non-sensitive test;
do not add real voices or transcripts to Git.

1. Preserve uncompressed Hark `them.wav` and `me.wav` spools from the same test
   meeting. Use 16 kHz mono PCM16 or float32 originals, not normalized exports
   or the lossy MP3 archive. In Settings > Meetings, uncheck **Compress kept
   recordings (about 29 MB per hour)** for the test and retain a nonzero audio
   cap. Leave **Reduce speaker echo** off to retain an unprocessed mic control.
   Copy the completed meeting's WAVs from its recordings folder before
   restoring that preference. Record device, speaker volume, room, and capture mode.
2. At the normal speaker volume, record far-only audio for 20 seconds, local
   speech alone for 10 seconds, both for 20 seconds, and far-only again for
   10 seconds. Repeat once with headphones as the low-echo control. Do not
   change gain, move devices, or reuse separately recorded channels between
   engines.
3. Preserve existing leading timeline padding. The optional `pair` command
   only pads the shorter file's **tail** to equal duration; it neither infers
   timestamps nor fixes alignment. Then process the exact pair with both
   engines using automatic delay first:

   ```sh
   ./target/cargo/release/hark-meeting-aec-bakeoff pair them.wav me.wav results/real-01
   ./target/cargo/release/hark-meeting-aec-bakeoff process aec3-0.4.0 results/real-01/render.wav results/real-01/mic.wav results/real-01/rust.wav auto
   ./target/cargo/release/hark-meeting-aec-bakeoff process webrtc-2.1.0 results/real-01/render.wav results/real-01/mic.wav results/real-01/cpp.wav auto
   ```

4. Listen through headphones to the microphone original and both outputs,
   with playback volume fixed. Score residual remote voice, lost local words,
   robotic/chopped sound during overlap, and recovery after overlap. Label
   outputs A/B when possible. A separate test may pass a known delay in
   milliseconds as the last argument, but apply the same hint to both.
5. Use the listening evidence to review the chosen engine's local-word
   preservation and remaining alignment limits. Synthetic scores alone do not
   establish the quality of a real speakerphone recording.

`process` refuses unequal input lengths, unsupported WAV formats, non-finite
samples, unknown backends, and existing output files. `pair` and `bench` refuse
existing output directories. Logs contain only engine/fixture labels and
counts. WAVs are deliberately created locally; the `results/` directory is
ignored by Git.

## Hark integration constraints

The production processing seam is on the meeting worker **after** continuous
resampling/alignment and **before** the microphone spool/chunker. Maintain a
10 ms render queue and feed the corresponding render frame before microphone
capture. Never run this in a cpal callback or add locks/allocations there.

The production recorder feeds the render reference before processing paired
microphone frames. It bounds microphone waiting for a reference to 250 ms and
render history to two seconds. A separate 128-sample (8 ms) original-microphone
guard compensates the pinned Rust engine's output latency: startup and reset
discard leading delayed output, while stop or fallback writes the retained
original guard before pending mic audio, including any incomplete final frame.
Matching input/output lengths alone would not prove that the real ending survived.

Missing references bypass processing; an engine failure disables processing for
the meeting, and input discontinuities reset adaptation. Track-close decisions
are captured once per drain so an error arriving during processing cannot close
a track before its resampler/AEC tail is flushed. The setting is off by default
and fixed for the recording when the meeting starts.

Initial placement still uses elapsed worker time minus delivered samples; it
is not measured hardware timestamp pairing. The Windows loopback exposes a first
QPC timestamp, but the recorder does not use it, and the microphone callback
discards its capture timestamp. Continuous resampling is persistent across
drains. Automatic engine delay estimation does not add explicit device-clock
drift correction. Common-origin timestamps and drift handling remain potential
improvements; the first-delivery approximation is not evidence of precise alignment.

Per-process loopback can omit other audible applications. An AEC fed only a
meeting's render reference cannot cancel an unrelated app's speaker audio.
The all-except-Hark mode has different coverage and must be tested separately.

## Versions, licenses, and platform evidence

| Candidate | Reviewed source | License and build evidence |
| --- | --- | --- |
| `aec3 0.4.0` | Tag `v0.4.0`, commit `f999860f91998daeb6d051e6076eaff629356469` | Rust implementation; package `MIT OR BSD-3-Clause`; WebRTC-derived portions carry BSD notices and the repo includes a patent grant. No native C++ build. Upstream SIMD CI covers Linux x86_64/ARM, not Windows. |
| `webrtc-audio-processing 2.1.0` | Tag `v2.1.0`, commit `c14d7af1760baff83e8210fee336a0cae0faaa7d` | BSD-3-Clause wrapper, bundled WebRTC/PulseAudio C++ and Abseil notices also apply. Requires a native compiler and build tools. Upstream CI covers Linux/macOS, not Windows. |

The C++ bundled build contains Unix command/archive assumptions (`cp -a`,
`nm --defined-only`, `.a` archives, a `rust-objcopy` path without `.exe`) and
Linux/macOS library search paths. A successful WSL build does not prove native
Windows MSVC support. Pure Rust's simpler build is an integration advantage,
not evidence of superior echo quality. Hark retains the selected Rust engine's
MIT/WebRTC BSD license text and accompanying patent grant in
[THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md). The isolated Rust candidate passed a native Windows
GNU build, four tests, and replay of all seven saved synthetic fixtures on this
host; see [the exact scope and commands](RESULTS.md#native-windows-rust-check).
Native C++ builds, MSVC, Hark integration, and actual speaker quality remain
unverified by this experiment.

Primary sources, inspected 2026-09-28:

- [AEC3 manifest](https://github.com/RubyBit/aec3-rs/blob/v0.4.0/Cargo.toml),
  [linear pipeline API](https://github.com/RubyBit/aec3-rs/blob/v0.4.0/src/pipelines/linear.rs),
  [file example](https://github.com/RubyBit/aec3-rs/blob/v0.4.0/examples/file_to_file.rs),
  [license](https://github.com/RubyBit/aec3-rs/blob/v0.4.0/LICENSE),
  [CI](https://github.com/RubyBit/aec3-rs/blob/v0.4.0/.github/workflows/simd-arch-tests.yml).
- [WebRTC wrapper API](https://github.com/tonarino/webrtc-audio-processing/blob/v2.1.0/src/lib.rs),
  [configuration](https://github.com/tonarino/webrtc-audio-processing/blob/v2.1.0/webrtc-audio-processing-config/src/lib.rs),
  [build script](https://github.com/tonarino/webrtc-audio-processing/blob/v2.1.0/webrtc-audio-processing-sys/build.rs),
  [license](https://github.com/tonarino/webrtc-audio-processing/blob/v2.1.0/COPYING),
  [CI](https://github.com/tonarino/webrtc-audio-processing/blob/v2.1.0/.github/workflows/rust.yml).
- [Meson 1.12.1 metadata](https://pypi.org/pypi/meson/1.12.1/json) and
  [Ninja 1.13.2 metadata](https://pypi.org/pypi/ninja/1.13.2/json), supplying
  the wheel URLs and verified download hashes in the bootstrap.

Local integration evidence: `crates/hark-pipeline/src/meeting/recorder.rs`,
`crates/hark-pipeline/src/meeting/echo.rs`,
`crates/hark-audio/src/meeting_aec.rs`,
`crates/hark-audio/src/capture_win.rs`,
`crates/hark-audio/src/loopback_win.rs`, and
`tasks/2026-09-26-plan-meeting-transcription.md` (Polish item 6 / decision D6).
