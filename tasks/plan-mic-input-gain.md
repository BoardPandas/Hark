# Plan — Microphone Sensitivity: The Silence Gate Drops Quiet Speech

**Created:** 2026-07-21 (rewritten same day against the real v0.14.3 tree; the first draft was written against a stale checkout with no code in it and was largely wrong)
**Status:** **All four phases implemented 2026-07-21** (v0.17.0). 314 workspace tests green, clippy clean on Linux and on `x86_64-pc-windows-msvc`. Runtime behaviour on real hardware — the WASAPI communications-role query, and whether the retuned gate actually fixes affected users — is still unconfirmed; see Verification below.
**Related:** [`crates/hark-audio/src/window.rs`](../crates/hark-audio/src/window.rs), [`crates/hark-audio/CLAUDE.md`](../crates/hark-audio/CLAUDE.md).

## Symptom

Users must speak close to the mic or Hark does not pick them up. Their mic works fine in every other app.

Note the shape of this complaint: it is not "transcription is inaccurate." It is "nothing happens." That points at a gate, not at signal quality — and there is one.

## Primary cause: `gate_clip` is an absolute threshold over a padded window

[`window.rs:110`](../crates/hark-audio/src/window.rs:110):

```rust
pub fn gate_clip(samples: &[f32], params: &WindowParams) -> GateVerdict {
    if rms(samples) < params.silence_rms { GateVerdict::TooQuiet } else { GateVerdict::Speech }
}
```

with `silence_rms: 0.01` (−40 dBFS). Two independent defects compound.

**1. The threshold is absolute, not relative to the room.** It encodes an assumption about how loud the user's mic is. A laptop array mic at arm's length with the Windows level at 50–70 sits around −30 to −40 dBFS RMS during normal speech. That straddles the threshold. Lean in, it passes; sit back, it drops. That is the reported symptom exactly.

**2. RMS is computed over the whole assembled window**, which by construction includes 300 ms of pre-roll and 150 ms of tail that are *not speech* — plus any pause between pressing the chord and starting to talk. RMS is a mean, so silence dilutes it. The gate passes iff `speech_rms × √(speech fraction) ≥ 0.01`, which means **the effective threshold rises as utterances get shorter**:

| Utterance | Window | Speech fraction | Speech RMS needed | dBFS |
|---|---:|---:|---:|---:|
| 3.0 s sentence, speaking throughout | 3450 ms | 0.87 | 0.0107 | −39.4 |
| 1.0 s hold, speaking throughout | 1450 ms | 0.69 | 0.0120 | −38.4 |
| 1.0 s hold, 300 ms lead-in pause | 1450 ms | 0.48 | 0.0144 | −36.8 |
| 0.4 s hold — "yes", "done" | 850 ms | 0.29 | 0.0184 | −34.7 |
| 0.7 s hold, pause then one word | 1150 ms | 0.26 | 0.0196 | −34.2 |

So the gate is strictest on **short commands and on users who press-then-think** — the highest-frequency, most latency-sensitive interactions in a push-to-talk dictation tool. A user whose long sentences work but whose one-word confirmations vanish will describe that as "it doesn't hear me unless I get close," because leaning in is the only variable they control.

**The cost asymmetry is backwards.** The gate exists to avoid wasted spend ([`window.rs:87`](../crates/hark-audio/src/window.rs:87) cites Groq's 10 s billing minimum). But a false *pass* costs one cheap API call; a false *drop* is the product silently not working. It should be tuned to almost never drop real speech.

## Contributing causes

**Channel averaging costs up to 6 dB.** [`ring.rs:78`](../crates/hark-audio/src/ring.rs:78) averages all channels into mono. Many laptop array mics present a second channel that is a near-silent reference; averaging halves the amplitude. This pushes borderline signals under the gate. Take channel 0, or the loudest channel, rather than the mean.

**No gain stage exists anywhere.** Confirmed by grep across `crates/`. Other apps (Teams, Zoom, Discord) run AGC by default and are normalizing quiet signals up — which is why users believe their mic is fine everywhere else. It is; those apps are compensating.

**Device role.** `select_device` falls back to `host.default_input_device()`, which is cpal's `eConsole`. Windows keeps a separate Default *Communications* Device, and every headset setup guide tells users to set that one. A user with their headset as the communications default gets it in Teams and the built-in array mic in Hark. The device picker (v0.14.0) lets them fix this manually, but nothing tells them there is anything to fix.

**Diagnostics are too coarse to confirm any of this from a user report.** [`worker.rs:129`](../crates/hark-pipeline/src/worker.rs:129) logs `"dictation gated (too short or silent)"` — it does not log the measured RMS, the threshold, or which of the two gates fired, and `FailStage::Gated` collapses both causes into one variant the UI cannot differentiate.

## Phase 1 — Make the gate reportable — **DONE**

- `assemble_window` logs the measured loudness, the clip's noise floor, the threshold, and the sample count on every too-quiet drop. (Loudness + floor turned out more useful than the planned "speech fraction", which is an artefact of the mean-based gate that no longer exists.)
- `FailStage::Gated` split into `GatedTooShort` / `GatedTooQuiet`; `assemble_window` returns `Assembled::Gated(verdict)` rather than a bare `None`, so the reason survives the crate boundary.
- `capture_win` logs device name, rate, channel count, and format at stream open, plus a line naming the channel-0 downmix when the device is multi-channel.

## Phase 2 — Fix the gate — **DONE**

`peak_window_rms` + a rewritten `gate_clip` in `window.rs`. The peak-window
approach subsumed the planned "measure the hold region only" option: it is
pause- and length-independent without threading region boundaries through the
gate at all.

Noise-floor-relative thresholding landed too, but **OR'd with the absolute
threshold rather than replacing it** — so the change can only ever admit clips
the old rule dropped, never the reverse. The floor is the clip's quietest
window, not the pre-roll's mean: pre-roll exists precisely to catch words the
user started early, so it is the one region that cannot be assumed silent.
`DEAD_MIC_RMS` (≈ −55 dBFS) is the backstop the relative path cannot argue
under.

## Phase 3 — Level and device — **DONE**

- `push_interleaved` takes channel 0 instead of averaging; channel count logged at open.
- New `gain` module: boost-only per-utterance normalization after resampling, limited by a clipping ceiling, a noise ceiling, and a hard cap. Applied gain is carried on `AudioClip` for logging.
- `communications_default_device()` queries `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture, eCommunications)` on its own COM apartment; the picker labels that device "used by Teams/Zoom". No-ops off Windows.

## Phase 4 — Tell the user — **DONE**

- Settings → Microphone shows a live meter driven by the existing `LevelMeter`, with silent / too-quiet / good / hot bands and per-band guidance. Repaints at 20 fps only while the page is open.
- A too-quiet drop now produces `PipelineStatus::Hint` — footer line plus a Settings jump, tray stays idle. A too-short hold stays silent, as it should.

## Verification

- 314 workspace tests pass; clippy clean with `-D warnings` on Linux and `x86_64-pc-windows-msvc`.
- `hark-audio` is type-checked for the Windows target, which covers the WASAPI/COM code paths that cannot run here.
- **Not verified:** `hark-app`'s own tests compile but cannot link on Linux (`tray-icon`/`muda` want `libxdo`), so the `next_status` and level-band tests are type-checked only. And nothing here has been run against a real microphone — the thresholds are reasoned, not measured. The first real user log is what should confirm or retune them.

## Lessons Learned / Gotchas

Checked against LL-G on 2026-07-21.

- **A mean-based gate over a padded window is length-dependent.** Padding that exists for good reasons (pre-roll catches early words) silently makes the gate stricter for short utterances. Any threshold applied to a mean must be applied over the region of interest, not the assembled buffer. This is the actual bug and it is not obvious from reading `gate_clip` alone — it only appears when you look at what `assemble_window` hands it.
- **Cost-protection gates need inverted tuning.** A gate protecting against wasted spend should bias hard toward passing, because the failure it causes (product does nothing) is far more expensive than the failure it prevents (one API call).
- **`rubato-4-whole-clip-process-all` (HIGH) — already handled.** [`crates/hark-audio/CLAUDE.md`](../crates/hark-audio/CLAUDE.md) documents it and `resample.rs` uses `process_all`. Recorded here only so a future reader does not re-litigate it.
- **`mmdevice-apartment-bound` (HIGH) — already handled.** Capture, enumeration, and the level meter each own their COM apartment on dedicated threads; the meter passes `f32` over an atomic rather than sharing the device. `capture_win.rs` and `level.rs` are worth reading as the reference pattern before adding any new WASAPI touchpoint.
- **A "cannot build here" conclusion deserves one more push before it goes in a plan.** An earlier draft of this document recorded that `hark-audio` could not compile on Linux for want of ALSA headers. That was wrong: the headers were installed and the first `cargo test` had simply run sandboxed without access to `pkg-config`. The retry succeeded. A tooling failure that blocks verification is worth re-testing outside the first environment you hit it in, because the cost of believing it is every later claim going unverified.
- **`cargo check -p <crate>` does not check that crate's tests.** Clippy with `--all-targets` caught a stale `FailStage::Gated` in a `#[cfg(test)]` block that a plain `cargo check` had reported clean. For crates that cannot link (see Verification), `--all-targets` is the only thing standing between you and a broken test file.
- **`cargo check --target x86_64-pc-windows-msvc` type-checks Windows-only code from Linux** as long as no dependency in the graph needs a C compiler — `check` never links, so no MSVC toolchain is required. It caught nothing here, but it is the difference between writing `#[cfg(windows)]` COM code blind and writing it type-checked. It fails for the workspace as a whole only because `hark-store` pulls `libsqlite3-sys`, which does build C.
- **A uniform clip has no noise floor to find.** The first cut of `normalization_gain` capped the gain using the quietest window as "the room" — but in a steady clip the quietest window *is* the signal, so the clips that most needed lifting got none. The floor is only usable as noise when it sits clearly below the peak; otherwise there is nothing to protect and the gate has already ruled the clip speech.
