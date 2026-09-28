use super::*;

#[derive(Default)]
struct Subtract {
    calls: usize,
    resets: usize,
    fail: bool,
    nonfinite: bool,
}

impl FrameProcessor for Subtract {
    fn process(&mut self, render: &[f32], mic: &[f32], out: &mut [f32]) -> bool {
        self.calls += 1;
        for i in 0..out.len() {
            out[i] = if self.nonfinite {
                f32::NAN
            } else {
                mic[i] - render[i]
            };
        }
        !self.fail
    }

    fn reset(&mut self) -> bool {
        self.resets += 1;
        true
    }
}

#[test]
fn independent_batch_sizes_pair_by_position_and_preserve_partial_tail() {
    let mut echo = EchoReducer::with_processor(Subtract::default());
    let mic: Vec<_> = (0..1_937).map(|i| (i as f32 * 0.05).sin() * 0.4).collect();
    let render: Vec<_> = mic.iter().map(|x| x * 0.25).collect();
    let unchanged = render.clone();
    let mut out = Vec::new();
    out.extend(echo.push(&mic[..73], &render[..951], false, false));
    out.extend(echo.push(&mic[73..1047], &render[951..], false, false));
    out.extend(echo.push(&mic[1047..], &[], false, false));
    out.extend(echo.push(&[], &[], false, true));
    assert_eq!(out.len(), mic.len());
    for i in 0..1_920 {
        assert_eq!(out[i], mic[i] - render[i]);
    }
    assert_eq!(&out[1920..], &mic[1920..]);
    assert_eq!(render, unchanged);
}

#[test]
fn delayed_reference_waits_then_recovers_without_dropping_or_shifting_mic() {
    let mut echo = EchoReducer::with_processor(Subtract::default());
    let mic = vec![0.5; 4_160];
    let first = echo.push(&mic, &[], false, false);
    assert_eq!(first, vec![0.5; 160]);
    assert_eq!(echo.mic.len(), MAX_PENDING);
    let second = echo.push(&[], &vec![0.2; 4_160], false, false);
    assert_eq!(second, vec![0.3; 4_000]);
    assert_eq!(echo.processor.as_ref().unwrap().resets, 1);
    assert!(echo.mic.is_empty());
    assert!(echo.render.is_empty());
}

#[test]
fn failure_or_nonfinite_output_preserves_original_and_disables_engine() {
    for nonfinite in [false, true] {
        let mut echo = EchoReducer::with_processor(Subtract {
            fail: !nonfinite,
            nonfinite,
            ..Subtract::default()
        });
        let mic = vec![0.4; 999];
        assert_eq!(echo.push(&mic, &vec![0.2; 999], false, false), mic);
        assert!(echo.processor.is_none());
        assert_eq!(echo.push(&[0.25; 3], &[], false, false), vec![0.25; 3]);
    }
}

#[test]
fn discontinuity_flushes_old_mic_and_resets_before_new_reference() {
    let mut echo = EchoReducer::with_processor(Subtract::default());
    assert!(echo.push(&[0.5; 160], &[], false, false).is_empty());
    let out = echo.push(&[0.25; 160], &[0.1; 320], true, false);
    assert_eq!(&out[..160], &[0.5; 160]);
    assert_eq!(&out[160..], &[0.15; 160]);
    assert_eq!(echo.processor.as_ref().unwrap().resets, 1);
}

#[test]
fn track_loss_flushes_pending_and_never_invents_silence() {
    let mut echo = EchoReducer::with_processor(Subtract::default());
    assert!(echo.push(&[0.5; 317], &[], false, false).is_empty());
    assert_eq!(
        echo.push(&[0.25; 5], &[], false, true),
        [&[0.5; 317][..], &[0.25; 5]].concat()
    );
    assert_eq!(echo.push(&[0.125; 41], &[], false, true), vec![0.125; 41]);
}

#[test]
fn long_clock_skew_and_large_batches_have_bounded_retention_and_exact_length() {
    let mut echo = EchoReducer::with_processor(Subtract::default());
    let mut received = 0;
    let mut emitted = 0;
    for i in 0..20_000 {
        let mic_len = 160 + usize::from(i % 7 == 0);
        received += mic_len;
        emitted += echo
            .push(&vec![0.3; mic_len], &[0.2; 160], false, false)
            .len();
        assert!(echo.mic.len() <= MAX_PENDING);
        assert!(echo.render.len() <= MAX_REFERENCE);
        assert!(echo.mic.capacity() <= MAX_PENDING + FRAME_SAMPLES);
        assert!(echo.render.capacity() <= MAX_REFERENCE);
    }
    received += 80_037;
    emitted += echo
        .push(&vec![0.4; 80_037], &vec![0.2; 80_037], false, false)
        .len();
    assert!(echo.mic.len() <= MAX_PENDING);
    assert!(echo.render.len() <= MAX_REFERENCE);
    assert!(echo.mic.capacity() <= MAX_PENDING + FRAME_SAMPLES);
    assert!(echo.render.capacity() <= MAX_REFERENCE);
    emitted += echo.push(&[], &[], false, true).len();
    assert_eq!(emitted, received);
}

struct Delayed {
    samples: VecDeque<f32>,
    fail: bool,
}

impl Delayed {
    fn new() -> Self {
        Self {
            samples: VecDeque::from(vec![0.0; 128]),
            fail: false,
        }
    }
}

impl FrameProcessor for Delayed {
    fn process(&mut self, _render: &[f32], mic: &[f32], out: &mut [f32]) -> bool {
        self.samples.extend(mic);
        for sample in out {
            *sample = self.samples.pop_front().unwrap();
        }
        !self.fail
    }
    fn reset(&mut self) -> bool {
        *self = Self::new();
        true
    }
    fn delay(&self) -> usize {
        128
    }
}

#[test]
fn filter_latency_is_removed_without_losing_audio_at_full_or_partial_frame_endings() {
    for len in [159, 160, 161, 319, 320, 321, 16_000, 16_037] {
        let input: Vec<_> = (0..len).map(|i| i as f32 / len as f32).collect();
        let mut echo = EchoReducer::with_processor(Delayed::new());
        let mut output = Vec::new();
        for part in input.chunks(173) {
            output.extend(echo.push(part, &vec![0.0; part.len()], false, false));
        }
        output.extend(echo.push(&[], &[], false, true));
        assert_eq!(output, input, "sample content at length {len}");
    }
}

#[test]
fn delayed_audio_is_preserved_across_failure_reference_gap_and_reset() {
    for scenario in 0..3 {
        let mut echo = EchoReducer::with_processor(Delayed::new());
        let mut output = echo.push(&[0.1; 160], &[0.0; 160], false, false);
        if scenario == 0 {
            echo.processor.as_mut().unwrap().fail = true;
        }
        output.extend(echo.push(
            &[0.2; 160],
            if scenario == 1 { &[] } else { &[0.0; 160] },
            scenario == 2,
            true,
        ));
        assert_eq!(output, [&[0.1; 160][..], &[0.2; 160]].concat());
    }
}

#[test]
fn flushing_after_track_loss_cannot_replay_audio_from_old_filter_history() {
    let mut echo = EchoReducer::with_processor(Delayed::new());
    let mut output = echo.push(&[0.1; 160], &[0.0; 480], false, true);
    output.extend(echo.push(&[0.2; 160], &[], false, true));
    assert_eq!(output, [&[0.1; 160][..], &[0.2; 160]].concat());
}
