# AEC comparison recorded 2026-09-28

Both engines built and processed every fixture successfully in WSL. This synthetic
run found more far-only attenuation from `aec3 0.4.0` and lower processing
time from `webrtc-audio-processing 2.1.0`. Hark 0.57.0 subsequently selects
**Rust `aec3 = 0.4.0`** for optional meeting microphone echo reduction. The user
explicitly accepted that recommendation without further speakerphone testing.
The measurements below are unchanged historical evidence; they do not establish
real speakerphone quality. The isolated Rust candidate also passed the native
Windows GNU check described below. Native C++ build validation remains open.

## Recorded evidence

- [Full JSON](evidence/2026-09-28-wsl-results.json) and
  [CSV](evidence/2026-09-28-wsl-results.csv): 7 fixtures × 3 backends × 3
  repetitions = **63 completed measurements**, including bypass controls.
- CPU: AMD Ryzen 9 9950X, 32 logical CPUs; Debian under WSL2,
  `x86_64-unknown-linux-gnu`; kernel `6.18.33.2-microsoft-standard-WSL2`.
- Rust `1.98.1 (48a229cea 2026-09-01)`, LLVM 22.1.8; GCC 14.2.0,
  Meson 1.12.1, Ninja 1.13.2. Cargo release; Meson build options verified as
  `buildtype=release`, `optimization=3`, `b_ndebug=true`.
- Hark checkout base: `691d3fd9b579f7a83520fa653b6ccbb4f99f5b99`.
  The standalone harness was uncommitted when measured. Other repository
  compilation was paused for the timed run; power mode, CPU affinity, and
  unrelated operating-system activity were not controlled.

The successful command, from this directory in WSL, was:

```sh
python3 build-wsl.py run --release --locked --all-features -- bench results/wsl-2026-09-28-buffered 3
```

WAV inputs and first-repetition outputs remain in that ignored local result
directory. Only the numeric reports are included in Git. An earlier attempt
was stopped during unbuffered fixture output before measured rows were
produced; it is not included in these results. The completed harness buffers
WAV writes, outside the timed engine calls.

Final-source verification passed: four all-features unit/streaming tests,
strict all-targets release clippy, direct rustfmt checks, and Python syntax
compilation. The `pair`/`process` CLI preserved a synthetic WAV byte for byte
through bypass; an attempted overwrite failed and left its SHA256 unchanged.

## Native Windows Rust check

On 2026-09-28, the existing Rust `1.98.1 (48a229cea 2026-09-01)` Windows
toolchain successfully built and ran the isolated Rust candidate for
`x86_64-pc-windows-gnu`. All **four tests passed**. A separate release build
produced the native `.exe`, which replayed all seven existing synthetic WAV
pairs with automatic delay and the same AEC/HPF configuration.

Independent WAV inspection confirmed **6,240,000 finite output samples** at
16 kHz mono: **39,000 complete 10 ms frames**, with every output length
matching its render/microphone inputs. The largest absolute sample was
0.245123. The validator checked the float32 WAVE_FORMAT_EXTENSIBLE subtype,
RIFF lengths, sample finiteness, and per-fixture counts. It ran separately
under WSL; all AEC processing and the four tests ran as native Windows executables.

Reproduce the native build/tests from this directory in PowerShell:

```powershell
cargo test --release --locked --no-default-features --features rust-aec3 --target x86_64-pc-windows-gnu --target-dir target/native-windows --jobs 1
cargo build --release --locked --no-default-features --features rust-aec3 --target x86_64-pc-windows-gnu --target-dir target/native-windows --jobs 1
```

The native replay used
`target/native-windows/x86_64-pc-windows-gnu/release/hark-meeting-aec-bakeoff.exe`
with the documented `process aec3-0.4.0 ... auto` command. Local outputs and
the numeric verification report are under the ignored
`results/native-windows-2026-09-28/`; build/test logs are under `target/`.
The executable SHA256 was
`0f85761e67593f8f691d11a7e7527ca879220a788b7f42204e552178fa05c141`.

This validates the standalone Rust candidate on this Windows GNU toolchain;
it does not validate MSVC, the Hark application, live capture/playback,
real-speaker quality, or production packaging. No App Control block occurred
in this isolated attempt. The existing WSL benchmark numbers are unchanged;
no native Windows timing comparison was collected.

The C++ candidate was not built natively: its upstream bundled build retains
the Unix tools, archive, and `rust-objcopy` path assumptions documented in
[README.md](README.md#versions-licenses-and-platform-evidence). That native
build qualification remains open.

## Signal measurements

Values below are medians of the three repetitions. ERLE is scored only when
the simulated local speaker is silent. The bypass control has 0 dB ERLE.

| Far-only case | Rust AEC3 ERLE | C++ WebRTC ERLE |
| --- | ---: | ---: |
| 40 ms delay | 42.42 dB | 18.17 dB |
| 160 ms delay | 43.56 dB | 18.22 dB |
| +31 ppm drift, 40 ms delay | 27.75 dB | 10.97 dB |
| −31 ppm drift, 40 ms delay | 29.30 dB | 10.78 dB |

| Near-audio case | Rust raw-reference SNR / gain | C++ raw-reference SNR / gain |
| --- | ---: | ---: |
| Near only | 5.08 dB / −1.67 dB | 5.08 dB / −1.67 dB |
| Double talk, 40 ms delay | 5.91 dB / −2.01 dB | 5.27 dB / −1.99 dB |
| Double talk, 160 ms delay | 5.43 dB / −2.14 dB | 4.92 dB / −2.26 dB |

Both engines' near-only results are almost identical, with a best-fit lag of
123 samples (7.69 ms). Their high-pass filtering and other shared signal-path
effects are included when comparing output against the **unfiltered** clean
source. Consequently, these raw-reference SNR/gain numbers must not be read
as a count of lost words, an intelligibility score, or isolated echo damage.
The bypass double-talk SNR is 6.28 / 6.31 dB, illustrating that this metric
alone can favor unfiltered echo-contaminated audio. A future quality study
should add a matched-filter control and perceptual/listening assessment.

The synthetic room is linear and fixed. Both drift cases last 120 seconds,
so 31 ppm accumulates 3.72 ms; this is not an hour-long stability result.

## Processing measurements

| Engine | Median p95 frame time across fixtures | Real-time factor range across all repetitions | Worst observed frame |
| --- | ---: | ---: | ---: |
| Rust AEC3 | 34.30–49.45 µs | 0.00261–0.00399 | 2.54 ms |
| C++ WebRTC | 21.08–40.59 µs | 0.00179–0.00301 | 0.462 ms |

Each frame represents 10 ms of audio. Times include render analysis and
capture processing, including wrapper overhead; they exclude startup,
file IO, fixture synthesis, and metric calculation. These wall-time samples
on one fast WSL host are not native Windows latency guarantees. All outputs
were finite; observed peak amplitudes stayed below 0.246 for both engines.

## Production decision and remaining limits

Rust AEC3 was selected for the optional production path after the user accepted
the recommendation and waived additional speakerphone testing. The synthetic
far-only attenuation and the successful standalone native Windows GNU build
support that choice; avoiding the C++ candidate's unqualified Windows build
also reduces integration work. This is not a measured real-speaker quality win.

Production **Reduce speaker echo** defaults off and applies from the next
meeting. It processes only the microphone before its recording/live transcript,
using 16 kHz/160-sample frames with the evaluated filter configuration. Microphone
waiting for its reference is bounded to 250 ms and render history to two seconds.
A separate 128-sample (8 ms) original-microphone guard compensates fixed engine
latency: startup and reset discard leading delayed output; stop or fallback writes
the original guard first, followed by pending mic audio and any incomplete frame.
This preserves the real ending, not just the output sample count. Missing
reference or errors preserve microphone audio; engine failures disable further
processing for that meeting. Input discontinuities reset adaptation. The recorder
captures track-close decisions before flushing resampler/AEC tails, so a later
device error cannot cause closure to skip the flush.

The production latency fixture compares AEC output with an identically
high-pass-filtered bypass and pins the delay at 128 samples. The historical
123-sample best-fit alignment above compared output against the unfiltered
synthetic source; it is not this fixed-latency measurement. None of the historical
signal or timing values were recomputed for the production compensation.

The recorder retains first-delivery timeline estimation and automatic engine
delay estimation. It has no hardware timestamp pairing or explicit device-clock
drift correction. Local-word preservation on actual speakers, nonlinear echo,
and long-call stability remain unverified. The [real recording procedure](README.md#replay-a-real-speaker-recording)
remains available for later qualification. No new benchmark timings or quality
measurements are claimed by the production decision.
