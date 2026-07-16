//! Window-assembly integration against a synthetic ring buffer fed with the
//! committed spike fixture (real 16 kHz speech): the full pre-STT path
//! (ring -> window -> gate -> resample passthrough -> WAV encode) asserted
//! on sample counts, never wall-clock (per .claude/rules/tests.md).

use hark_audio::ring::ring;
use hark_audio::{assemble_window, window, WindowParams};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../hark-stt/fixtures/spike_clip.wav"
);

fn fixture_samples() -> Vec<f32> {
    let bytes = std::fs::read(FIXTURE).expect("spike fixture exists (committed in hark-stt)");
    let info = hark_stt::wav::parse_wav_16k_mono(&bytes).expect("fixture parses");
    assert_eq!(info.sample_rate, 16_000, "fixture contract: 16 kHz");
    assert_eq!(info.channels, 1, "fixture contract: mono");
    info.samples
}

fn params() -> WindowParams {
    WindowParams::default()
}

#[test]
fn fixture_speech_survives_the_full_pre_stt_path() {
    let speech = fixture_samples();
    let rate = 16_000_u32;
    let p = params();

    // Build a synthetic capture: 1 s of silence, the fixture speech (as if
    // spoken during the hold), then enough silence for the tail.
    let (producer, consumer) = ring(window::ring_capacity(rate, &p));
    let lead_in = vec![0.0_f32; rate as usize];
    let tail_pad = vec![0.0_f32; rate as usize];
    producer.push(&lead_in);
    let down_abs = consumer.total_written();
    producer.push(&speech);
    let up_abs = consumer.total_written();
    producer.push(&tail_pad);

    let clip = assemble_window(&consumer, rate, down_abs, up_abs, &p)
        .expect("assembly succeeds")
        .expect("real speech must pass the silence gate");

    // At the 16 kHz source rate resampling is a passthrough, so the clip is
    // exactly pre-roll + hold + tail samples.
    let expected = window::ms_to_samples(p.preroll_ms, rate)
        + speech.len() as u64
        + window::ms_to_samples(p.tail_ms, rate);
    assert_eq!(clip.samples_16k.len() as u64, expected);

    // And the encoded WAV round-trips through the frozen hark-stt contract
    // with the identical sample count.
    let wav = hark_stt::wav::encode_wav_16k_mono(&clip.samples_16k);
    let parsed = hark_stt::wav::parse_wav_16k_mono(&wav).expect("encoded WAV parses");
    assert_eq!(parsed.samples.len(), clip.samples_16k.len());
    assert_eq!(parsed.sample_rate, 16_000);
    assert_eq!(parsed.channels, 1);
}

#[test]
fn silence_around_the_fixture_is_gated_but_speech_is_not() {
    let speech = fixture_samples();
    let rate = 16_000_u32;
    let p = params();

    let (producer, consumer) = ring(window::ring_capacity(rate, &p));
    // Pure silence hold: gated, no request.
    producer.push(&vec![0.0_f32; 3 * rate as usize]);
    let gated = assemble_window(&consumer, rate, rate as u64, 2 * rate as u64, &p)
        .expect("assembly succeeds");
    assert!(gated.is_none(), "silence must be gated");

    // Speech hold in the same ring: passes.
    let down_abs = consumer.total_written();
    producer.push(&speech);
    let up_abs = consumer.total_written();
    producer.push(&vec![0.0_f32; rate as usize]);
    let clip = assemble_window(&consumer, rate, down_abs, up_abs, &p).expect("assembly succeeds");
    assert!(clip.is_some(), "fixture speech must pass the gate");
}
