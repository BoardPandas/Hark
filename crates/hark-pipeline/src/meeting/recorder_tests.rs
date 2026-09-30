use super::*;
use hark_audio::ring::{ring, Producer};
use hark_audio::spool::f32_to_i16;
use std::sync::mpsc;

fn track(dir: &std::path::Path, channel: Channel, rate: u32) -> (Producer, Track) {
    let (producer, consumer) = ring(rate as usize * 10);
    let name = if channel == Channel::Me {
        ME_FILE
    } else {
        THEM_FILE
    };
    let mut track = Track::new(
        channel,
        Source::Test { failed: false },
        consumer,
        rate,
        SpoolWriter::create(&dir.join(name)).unwrap(),
        Some(Chunker::new(ChunkParams {
            silence_rms: 0.0,
            ..ChunkParams::default()
        })),
    )
    .unwrap();
    // Explicit sample-zero origin for deterministic fixture capture.
    track.aligned = true;
    (producer, track)
}

fn samples(path: &std::path::Path) -> Vec<i16> {
    std::fs::read(path).unwrap()[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

fn recorder(dir: &std::path::Path, me: Track, them: Track, enabled: bool) -> Recorder {
    Recorder {
        id: "fixture".into(),
        dir: dir.into(),
        started: Instant::now(),
        me: Some(me),
        them: Some(them),
        live: None,
        echo: enabled.then(|| EchoReducer::new().unwrap()),
    }
}

#[test]
fn disabled_aec_preserves_both_tracks_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, me) = track(dir.path(), Channel::Me, 16_000);
    let (render, them) = track(dir.path(), Channel::Them, 16_000);
    let mut recording = recorder(dir.path(), me, them, false);
    let input: Vec<_> = (0..1_917).map(|i| (i as f32 * 0.09).sin() * 0.4).collect();
    mic.push(&input[..991]);
    render.push(&input[..1_019]);
    recording.pump().unwrap();
    mic.push(&input[991..]);
    render.push(&input[1_019..]);
    let result = recording.stop().unwrap();
    assert_eq!((result.me_samples, result.them_samples), (1_917, 1_917));
    let expected: Vec<_> = input.iter().copied().map(f32_to_i16).collect();
    assert_eq!(samples(&dir.path().join(ME_FILE)), expected);
    assert_eq!(samples(&dir.path().join(THEM_FILE)), expected);
}

#[test]
fn stopping_flushes_loopback_before_the_final_ring_read() {
    let dir = tempfile::tempdir().unwrap();
    let (_mic, me) = track(dir.path(), Channel::Me, 16_000);
    let (render, mut them) = track(dir.path(), Channel::Them, 16_000);
    render.push(&[0.25; 160]);
    them.source = Source::FlushOnStop {
        flush: Some(Box::new(move || {
            render.push(&[0.75; 137]);
        })),
        captured_frames: None,
    };
    let mut recording = recorder(dir.path(), me, them, false);
    recording.pump().unwrap();
    let result = recording.stop().unwrap();
    assert_eq!(result.them_samples, 297);
    assert_eq!(
        samples(&dir.path().join(THEM_FILE)),
        [&[8_192; 160][..], &[24_576; 137]].concat()
    );
}

#[test]
fn mixer_retention_does_not_become_leading_silence_on_a_delayed_first_read() {
    let dir = tempfile::tempdir().unwrap();
    let (render, mut them) = track(dir.path(), Channel::Them, 16_000);
    them.aligned = false;
    render.push(&[0.25; 12_000]);
    them.source = Source::FlushOnStop {
        flush: Some(Box::new(move || render.push(&[0.5; 4_000]))),
        captured_frames: Some(16_000),
    };
    let first = them.read(1_000).unwrap();
    them.place(&first, None).unwrap();
    them.source.stop_loopback();
    let tail = them.read(1_000).unwrap();
    them.place(&tail, None).unwrap();
    assert_eq!(them.close(None).unwrap(), 16_000);
    let audio = samples(&dir.path().join(THEM_FILE));
    assert_eq!(audio, [&[8_192; 12_000][..], &[16_384; 4_000]].concat());
}

#[test]
fn microphone_spool_keeps_exact_resampled_count_and_the_ending_signal() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, me) = track(dir.path(), Channel::Me, 48_000);
    let (_render, them) = track(dir.path(), Channel::Them, 16_000);
    let mut input = vec![0.0; 24_000];
    input[23_880..].fill(0.5);
    mic.push(&input);
    let result = recorder(dir.path(), me, them, false).stop().unwrap();
    assert_eq!(result.me_samples, 8_000);
    let audio = samples(&dir.path().join(ME_FILE));
    assert_eq!(audio.len(), 8_000);
    assert!(audio[7_960..].iter().any(|sample| *sample > 8_000));
}

#[test]
fn enabled_aec_changes_only_mic_and_live_audio_matches_saved_audio() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, me) = track(dir.path(), Channel::Me, 16_000);
    let (render, them) = track(dir.path(), Channel::Them, 16_000);
    let mut recording = recorder(dir.path(), me, them, true);
    let (tx, rx) = mpsc::channel();
    recording.live = Some(tx);
    let input: Vec<_> = (0..8_037).map(|i| (i as f32 * 0.08).sin() * 0.4).collect();
    mic.push(&input);
    render.push(&input);
    let result = recording.stop().unwrap();
    assert_eq!((result.me_samples, result.them_samples), (8_037, 8_037));
    let saved = samples(&dir.path().join(ME_FILE));
    let original: Vec<_> = input.iter().copied().map(f32_to_i16).collect();
    assert_ne!(saved[..8_000], original[..8_000]);
    assert_eq!(saved[8_000..], original[8_000..]);
    assert_eq!(samples(&dir.path().join(THEM_FILE)), original);
    let live: Vec<_> = rx
        .into_iter()
        .filter(|(c, _)| *c == Channel::Me)
        .flat_map(|(_, chunk)| chunk.samples)
        .map(f32_to_i16)
        .collect();
    assert_eq!(live, saved);
}

#[test]
fn system_track_loss_flushes_waiting_mic_and_keeps_later_mic_audio() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, me) = track(dir.path(), Channel::Me, 16_000);
    let (_render, them) = track(dir.path(), Channel::Them, 16_000);
    let mut recording = recorder(dir.path(), me, them, true);
    mic.push(&[0.25; 317]);
    assert!(recording.pump().unwrap().is_empty());
    recording.them.as_mut().unwrap().source = Source::Test { failed: true };
    let lost = recording.pump().unwrap();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost[0].channel, Channel::Them);
    mic.push(&[0.5; 109]);
    let result = recording.stop().unwrap();
    assert_eq!(result.me_samples, 426);
    assert_eq!(
        samples(&dir.path().join(ME_FILE)),
        [&[8_192; 317][..], &[16_384; 109]].concat()
    );
}

#[test]
fn resampler_tail_reaches_aec_and_spool_when_stopping() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, me) = track(dir.path(), Channel::Me, 48_000);
    let (_render, them) = track(dir.path(), Channel::Them, 16_000);
    let mut recording = recorder(dir.path(), me, them, true);
    let input: Vec<_> = (0..24_317).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    let mut reference = StreamResampler::new(48_000).unwrap();
    let mut expected = reference.push(&input).unwrap();
    expected.extend(reference.drain().unwrap());
    mic.push(&input);
    recording.pump().unwrap();
    let result = recording.stop().unwrap();
    assert_eq!(result.me_samples as usize, expected.len());
    assert_eq!(
        samples(&dir.path().join(ME_FILE)),
        expected.into_iter().map(f32_to_i16).collect::<Vec<_>>()
    );
}

#[test]
fn an_error_arriving_after_the_drain_snapshot_cannot_close_an_unflushed_track() {
    let dir = tempfile::tempdir().unwrap();
    let (mic, mut me) = track(dir.path(), Channel::Me, 48_000);
    let (_render, them) = track(dir.path(), Channel::Them, 16_000);
    me.source = Source::FailingDuringDrain(std::cell::Cell::new(0));
    let mut recording = recorder(dir.path(), me, them, true);
    let input: Vec<_> = (0..24_317).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    let mut reference = StreamResampler::new(48_000).unwrap();
    let mut expected = reference.push(&input).unwrap();
    expected.extend(reference.drain().unwrap());
    mic.push(&input);
    assert!(recording.pump().unwrap().is_empty());
    assert!(recording.me.is_some());
    let lost = recording.pump().unwrap();
    assert_eq!(lost.len(), 1);
    assert!(recording.me.is_none());
    let result = recording.stop().unwrap();
    assert_eq!(result.me_samples as usize, expected.len());
    assert_eq!(
        samples(&dir.path().join(ME_FILE)),
        expected.into_iter().map(f32_to_i16).collect::<Vec<_>>()
    );
}

#[test]
fn initial_alignment_does_not_treat_resampler_buffering_as_startup_silence() {
    let dir = tempfile::tempdir().unwrap();
    for (channel, rate) in [(Channel::Me, 44_100), (Channel::Them, 48_000)] {
        let (producer, mut track) = track(dir.path(), channel, rate);
        track.aligned = false;
        let input = vec![0.25; rate as usize / 10];
        let mut reference = StreamResampler::new(rate).unwrap();
        let expected = reference.push(&input).unwrap();
        assert!(expected.len() < 1600, "fixture must leave audio buffered");
        producer.push(&input);
        // 100 ms captured, read 150 ms after session start: exactly 50 ms
        // of startup silence, regardless of either resampler's buffering.
        let actual = track.read(150).unwrap();
        assert_eq!(actual.len(), 800 + expected.len());
        assert!(actual[..800].iter().all(|&sample| sample == 0.0));
        assert_eq!(&actual[800..], expected);
    }
}
