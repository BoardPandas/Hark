use super::*;
use std::f64::consts::PI;

/// A 1 s tone (short enough to keep the suite fast) at `amp` of full
/// scale.
fn tone(seconds: f64, freq_hz: f64, amp: f64) -> Vec<i16> {
    let n = (spool::SPOOL_RATE as f64 * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / spool::SPOOL_RATE as f64;
            (amp * i16::MAX as f64 * (2.0 * PI * freq_hz * t).sin()) as i16
        })
        .collect()
}

fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples
        .iter()
        .map(|&s| (s as f64) * (s as f64))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

/// RMS relative to full scale, in dBFS.
fn dbfs(x: f64) -> f64 {
    if x <= 1e-9 {
        -180.0
    } else {
        20.0 * (x / i16::MAX as f64).log10()
    }
}

fn write_spool(path: &Path, samples: &[i16]) {
    let mut w = spool::SpoolWriter::create(path).expect("create spool");
    w.append(samples).expect("append");
    w.close().expect("close");
}

fn meeting_dir_with(dir: &Path, me_samples: &[i16], them_samples: &[i16]) -> (PathBuf, PathBuf) {
    let me = dir.join(spool::ME_FILE);
    let them = dir.join(spool::THEM_FILE);
    write_spool(&me, me_samples);
    write_spool(&them, them_samples);
    (me, them)
}

#[test]
fn archive_keeps_channels_independent_not_joint_stereo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loud = tone(1.0, 440.0, 0.5);
    let quiet = vec![0i16; loud.len()];
    meeting_dir_with(dir.path(), &loud, &quiet);

    let outcome = compress_meeting_dir(dir.path()).expect("compress");
    assert!(matches!(outcome, CompressOutcome::Compressed { .. }));

    let archive = ArchiveDecoder::open(&dir.path().join(ARCHIVE_FILE)).expect("open archive");
    let (l, r) = archive.decode_all().expect("decode");
    assert!(
        dbfs(rms(&l)) > -20.0,
        "the loud (me) side must survive: {} dBFS",
        dbfs(rms(&l))
    );
    assert!(
        dbfs(rms(&r)) < -60.0,
        "STEREO mode must keep the silent (them) side silent, got {} dBFS \
         (JOINT_STEREO would leak the tone in at about -96 dBFS, but this \
         checks against real crosstalk, not the encoder's noise floor)",
        dbfs(rms(&r))
    );
}

#[test]
fn fresh_archives_and_mono_exports_preserve_exact_frame_counts() {
    for frames in [
        1, 159, 160, 575, 576, 577, 1152, 4095, 4096, 4097, 16_000, 32_137,
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let samples: Vec<_> = (0..frames)
            .map(|i| (10_000.0 * (2.0 * PI * 400.0 * i as f64 / 16_000.0).sin()) as i16)
            .collect();
        let (me, them) = meeting_dir_with(dir.path(), &samples, &samples[..frames / 2]);
        let mono = dir.path().join("mono.mp3");
        export_mono_mp3(&MeetingAudio::Spools { me, them }, &mono).expect("mono export");
        compress_meeting_dir(dir.path()).expect("compress");
        for path in [mono, dir.path().join(ARCHIVE_FILE)] {
            let (left, right) = ArchiveDecoder::open(&path)
                .expect("open")
                .decode_all()
                .expect("decode");
            assert_eq!(left.len(), frames, "frame count {frames}");
            assert_eq!(right.len(), frames, "right frame count {frames}");
        }
    }
}

#[test]
fn finalization_rejects_an_export_without_gapless_metadata() {
    let mut encoder = lame_builder(1, Mode::Mono, Bitrate::Kbps32).unwrap();
    let samples = tone(1.0, 440.0, 0.4);
    let mut bytes = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(samples.len()));
    encoder
        .encode_to_vec(MonoPcm(&samples), &mut bytes)
        .unwrap();
    assert!(matches!(
        finish_lame(&mut encoder, &mut bytes),
        Err(EncodeError::Lame(_))
    ));
}

#[test]
fn archive_preserves_audio_in_the_final_fifty_milliseconds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut samples = vec![0; 32_137 - 800];
    samples.extend(tone(0.05, 440.0, 0.4));
    meeting_dir_with(dir.path(), &samples, &vec![0; samples.len()]);
    compress_meeting_dir(dir.path()).expect("compress");
    let (left, right) = ArchiveDecoder::open(&dir.path().join(ARCHIVE_FILE))
        .expect("open archive")
        .decode_all()
        .expect("decode");
    assert_eq!(left.len(), samples.len());
    assert!(
        rms(&left[left.len() - 400..]) > 1000.0,
        "final audio must survive"
    );
    assert!(rms(&right) < 1.0, "silent channel must remain independent");
}

#[test]
fn wavs_are_deleted_after_successful_compression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (me, them) = meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.4), &tone(1.0, 550.0, 0.4));

    let outcome = compress_meeting_dir(dir.path()).expect("compress");
    let CompressOutcome::Compressed {
        archive_bytes,
        wav_bytes_freed,
    } = outcome
    else {
        panic!("expected Compressed, got {outcome:?}");
    };
    assert!(archive_bytes > 0);
    assert!(wav_bytes_freed > 0);
    assert!(!me.exists(), "me.wav must be deleted");
    assert!(!them.exists(), "them.wav must be deleted");
    assert!(dir.path().join(ARCHIVE_FILE).exists());
}

#[test]
fn compress_with_no_spools_is_a_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = compress_meeting_dir(dir.path()).expect("compress");
    assert_eq!(outcome, CompressOutcome::NothingToDo);
}

#[test]
fn truncated_mp3_fails_verify() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 3 s so cutting it down to a handful of bytes leaves a decoded
    // duration far outside the 1 s tolerance, not just a little short.
    let me_samples = tone(3.0, 440.0, 0.4);
    let them_samples = tone(3.0, 550.0, 0.4);
    let me = dir.path().join(spool::ME_FILE);
    let them = dir.path().join(spool::THEM_FILE);
    write_spool(&me, &me_samples);
    write_spool(&them, &them_samples);
    let expected = stereo::spool_pair_frames(&me, &them).expect("frames");

    let archive = dir.path().join(ARCHIVE_FILE);
    encode_stereo_archive(&me, &them, &archive).expect("encode");
    let full = std::fs::read(&archive).expect("read");
    std::fs::write(&archive, &full[..300]).expect("truncate");

    assert!(verify_mp3(&archive, expected).is_err());
}

#[test]
fn recover_removes_an_interrupted_tmp_and_keeps_the_wavs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (me, them) = meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.3), &tone(1.0, 440.0, 0.3));
    std::fs::write(dir.path().join(format!("{ARCHIVE_FILE}.tmp")), b"partial").expect("write");

    let action = recover_meeting_dir(dir.path()).expect("recover");
    assert_eq!(action, RecoverAction::RemovedPartial);
    assert!(!dir.path().join(format!("{ARCHIVE_FILE}.tmp")).exists());
    assert!(
        me.exists() && them.exists(),
        "wavs must survive a partial-tmp recovery"
    );
}

#[test]
fn recover_finishes_a_good_interrupted_compression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let me_samples = tone(1.0, 440.0, 0.3);
    let them_samples = tone(1.0, 550.0, 0.3);
    let me = dir.path().join(spool::ME_FILE);
    let them = dir.path().join(spool::THEM_FILE);
    write_spool(&me, &me_samples);
    write_spool(&them, &them_samples);
    // Simulate a crash between "rename to audio.mp3" and "delete the
    // WAVs": encode straight to the final name, leaving the WAVs in
    // place too.
    let archive = dir.path().join(ARCHIVE_FILE);
    encode_stereo_archive(&me, &them, &archive).expect("encode");

    let action = recover_meeting_dir(dir.path()).expect("recover");
    assert_eq!(action, RecoverAction::FinishedCompression);
    assert!(archive.exists());
    assert!(!me.exists() && !them.exists());
}

#[test]
fn recover_discards_a_bad_interrupted_archive_and_keeps_the_wavs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (me, them) = meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.3), &tone(1.0, 550.0, 0.3));
    std::fs::write(dir.path().join(ARCHIVE_FILE), b"not an mp3").expect("write");

    let action = recover_meeting_dir(dir.path()).expect("recover");
    assert_eq!(action, RecoverAction::DiscardedBadArchive);
    assert!(!dir.path().join(ARCHIVE_FILE).exists());
    assert!(
        me.exists() && them.exists(),
        "wavs must survive a discarded bad archive"
    );
}

#[test]
fn recover_does_nothing_when_there_is_nothing_to_recover() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        recover_meeting_dir(dir.path()).expect("recover"),
        RecoverAction::None
    );

    // Also nothing to do with only WAVs (nothing interrupted) or only an
    // intact archive (nothing to recover from).
    let wavs_only = tempfile::tempdir().expect("tempdir");
    meeting_dir_with(
        wavs_only.path(),
        &tone(0.2, 440.0, 0.3),
        &tone(0.2, 440.0, 0.3),
    );
    assert_eq!(
        recover_meeting_dir(wavs_only.path()).expect("recover"),
        RecoverAction::None
    );
}

#[test]
fn mix_mono_of_two_full_scale_channels_never_clips_or_wraps() {
    // The arithmetic itself, isolated from lossy MP3 round-tripping
    // (which is free to introduce its own ringing on a signal this
    // extreme, and is not what "never clips" is about here).
    let n = 8;
    assert_eq!(
        mix_mono(&vec![i16::MAX; n], &vec![i16::MAX; n]),
        vec![i16::MAX; n]
    );
    assert_eq!(
        mix_mono(&vec![i16::MIN; n], &vec![i16::MIN; n]),
        vec![i16::MIN; n]
    );
    // Opposite extremes: (32767 + -32768) / 2 truncates to 0, not a
    // wrapped negative number.
    assert_eq!(
        mix_mono(&vec![i16::MAX; n], &vec![i16::MIN; n]),
        vec![0i16; n]
    );
}

#[test]
fn mono_export_of_full_scale_audio_has_the_right_duration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let n = spool::SPOOL_RATE as usize; // 1 s
    let me = dir.path().join(spool::ME_FILE);
    let them = dir.path().join(spool::THEM_FILE);
    write_spool(&me, &vec![i16::MAX; n]);
    write_spool(&them, &vec![i16::MAX; n]);

    let out = dir.path().join("share.mp3");
    export_mono_mp3(
        &MeetingAudio::Spools {
            me: me.clone(),
            them: them.clone(),
        },
        &out,
    )
    .expect("export");
    assert!(out.metadata().expect("meta").len() > 0);

    let (l, _r) = ArchiveDecoder::open(&out)
        .expect("open export")
        .decode_all()
        .expect("decode");
    assert_eq!(l.len(), n);
}

#[test]
fn mono_wav_export_matches_the_mix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let me_samples = tone(0.5, 440.0, 0.5);
    let them_samples = vec![0i16; me_samples.len()];
    let me = dir.path().join(spool::ME_FILE);
    let them = dir.path().join(spool::THEM_FILE);
    write_spool(&me, &me_samples);
    write_spool(&them, &them_samples);

    let out = dir.path().join("share.wav");
    export_mono_wav(
        &MeetingAudio::Spools {
            me: me.clone(),
            them: them.clone(),
        },
        &out,
    )
    .expect("export");

    let mut reader = hound::WavReader::open(&out).expect("open wav");
    let spec = reader.spec();
    assert_eq!(
        (spec.channels, spec.sample_rate, spec.bits_per_sample),
        (1, 16_000, 16)
    );
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("sample"))
        .collect();
    assert_eq!(samples.len(), me_samples.len());
    // them is silence, so the mix is me / 2.
    let expected: Vec<i16> = me_samples.iter().map(|&s| s / 2).collect();
    assert_eq!(samples, expected);
}

#[test]
fn full_exports_preserve_unrelated_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let (me, them) = meeting_dir_with(dir.path(), &[1000; 1600], &[0; 1600]);
    let src = MeetingAudio::Spools { me, them };
    for extension in ["wav", "mp3"] {
        let out = dir.path().join(format!("share.{extension}"));
        let unrelated = tmp_path_for(&out);
        std::fs::write(&unrelated, b"unrelated work").unwrap();
        if extension == "wav" {
            export_mono_wav(&src, &out).unwrap();
            assert_eq!(hound::WavReader::open(&out).unwrap().duration(), 1600);
        } else {
            export_mono_mp3(&src, &out).unwrap();
            assert_eq!(
                ArchiveDecoder::open(&out)
                    .unwrap()
                    .decode_all()
                    .unwrap()
                    .0
                    .len(),
                1600
            );
        }
        assert_eq!(std::fs::read(unrelated).unwrap(), b"unrelated work");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 6);
}

#[test]
fn failed_full_exports_preserve_destination_and_remove_owned_partials() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.mp3");
    std::fs::write(&broken, b"not an MP3").unwrap();
    let src = MeetingAudio::Archive(broken);
    for extension in ["wav", "mp3"] {
        let out = dir.path().join(format!("share.{extension}"));
        let unrelated = tmp_path_for(&out);
        std::fs::write(&out, b"previous export").unwrap();
        std::fs::write(&unrelated, b"unrelated work").unwrap();
        let result = if extension == "wav" {
            export_mono_wav(&src, &out)
        } else {
            export_mono_mp3(&src, &out)
        };
        assert!(result.is_err());
        assert_eq!(std::fs::read(&out).unwrap(), b"previous export");
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"unrelated work");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 5);
}

#[test]
fn failed_full_export_renames_remove_owned_partials() {
    let dir = tempfile::tempdir().unwrap();
    let (me, them) = meeting_dir_with(dir.path(), &[1000; 1600], &[0; 1600]);
    let src = MeetingAudio::Spools { me, them };
    for extension in ["wav", "mp3"] {
        let out = dir.path().join(format!("share.{extension}"));
        std::fs::create_dir(&out).unwrap();
        std::fs::write(out.join("keep"), b"existing directory").unwrap();
        let result = if extension == "wav" {
            export_mono_wav(&src, &out)
        } else {
            export_mono_mp3(&src, &out)
        };
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(out.join("keep")).unwrap(),
            b"existing directory"
        );
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn export_from_an_archive_source_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.4), &tone(1.0, 550.0, 0.4));
    compress_meeting_dir(dir.path()).expect("compress");

    let archive_path = dir.path().join(ARCHIVE_FILE);
    let out = dir.path().join("share.mp3");
    export_mono_mp3(&MeetingAudio::Archive(archive_path), &out).expect("export from archive");
    assert!(out.metadata().expect("meta").len() > 0);
}

#[test]
fn meeting_audio_prefers_spools_then_archive_then_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(meeting_audio(dir.path()).is_none());

    let (me, them) = meeting_dir_with(dir.path(), &tone(0.2, 440.0, 0.3), &tone(0.2, 440.0, 0.3));
    assert!(matches!(
        meeting_audio(dir.path()),
        Some(MeetingAudio::Spools { .. })
    ));

    // A stray archive alongside intact spools does not win: the spools
    // are the source of truth until they are actually deleted.
    std::fs::write(dir.path().join(ARCHIVE_FILE), b"not an mp3").expect("write");
    assert!(matches!(
        meeting_audio(dir.path()),
        Some(MeetingAudio::Spools { .. })
    ));

    std::fs::remove_file(&me).expect("remove");
    std::fs::remove_file(&them).expect("remove");
    assert!(matches!(
        meeting_audio(dir.path()),
        Some(MeetingAudio::Archive(_))
    ));

    std::fs::remove_file(dir.path().join(ARCHIVE_FILE)).expect("remove");
    assert!(meeting_audio(dir.path()).is_none());
}
