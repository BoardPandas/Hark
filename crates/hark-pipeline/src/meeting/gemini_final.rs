//! Bounded mono windows for the explicitly selected Gemini Files final pass.
use super::LiveSegment;
use hark_config::Settings;
use hark_stt::meeting::FinalSegment;
use hark_stt::meeting_gemini::{GeminiFiles, WINDOW_MS};
use std::path::Path;

const WINDOW_FRAMES: usize = 16_000 * 300;

pub(super) fn run(dir: &Path, settings: &Settings) -> Result<Vec<LiveSegment>, String> {
    let key = hark_keychain::resolve_key_for("HARK_GEMINI_KEY", "gemini").map_err(|_| {
        "Add a Gemini key in Settings > Meetings to use the Gemini final pass.".to_string()
    })?;
    let api = GeminiFiles::new(key, settings.meeting.gemini_model.clone())?;
    let source = hark_audio::meeting_audio(dir).ok_or("The meeting recording is unavailable.")?;
    let chunks = hark_audio::mp3::stereo_chunks(&source)
        .map_err(|_| "Cannot read meeting audio.".to_string())?;
    let corrector = hark_spellbook::Corrector::new(&settings.spellbook.corrector_entries());
    let keyterms = settings.spellbook.terms();
    let mut segments = Vec::new();
    visit_windows(chunks, |channel, offset_ms, samples| {
        let duration_ms = (samples.len() as u64 * 1000).div_ceil(16_000);
        let floats: Vec<f32> = samples.iter().map(|s| *s as f32 / 32768.0).collect();
        let wav = hark_stt::wav::encode_wav_16k_mono(&floats);
        let window = api.transcribe(wav, channel, offset_ms, duration_ms, &keyterms)?;
        check_coverage(samples, offset_ms, &window)?;
        segments.extend(window.into_iter().map(|s| LiveSegment {
            channel: s.channel,
            speaker: s.speaker,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: corrector.correct(&s.text).0,
        }));
        Ok(())
    })?;
    if segments.is_empty() {
        return Err("Gemini returned no transcript; the previous transcript was kept.".into());
    }
    segments.sort_by_key(|s| (s.start_ms, s.channel));
    Ok(segments)
}

/// Separately visit each channel of each five-minute window, including the
/// final partial window. Never buffer more than two windows of PCM.
fn visit_windows<I, F>(chunks: I, mut visit: F) -> Result<(), String>
where
    I: IntoIterator<Item = Result<(Vec<i16>, Vec<i16>), hark_audio::EncodeError>>,
    F: FnMut(u8, u64, &[i16]) -> Result<(), String>,
{
    let mut buffers = [
        Vec::with_capacity(WINDOW_FRAMES),
        Vec::with_capacity(WINDOW_FRAMES),
    ];
    let mut offset = 0;
    for chunk in chunks {
        let (left, right) = chunk.map_err(|_| "Cannot decode meeting audio.".to_string())?;
        if left.len() != right.len() {
            return Err("Meeting channels are not aligned.".into());
        }
        let mut consumed = 0;
        while consumed < left.len() {
            let count = (WINDOW_FRAMES - buffers[0].len()).min(left.len() - consumed);
            buffers[0].extend_from_slice(&left[consumed..consumed + count]);
            buffers[1].extend_from_slice(&right[consumed..consumed + count]);
            consumed += count;
            if buffers[0].len() == WINDOW_FRAMES {
                visit(0, offset, &buffers[0])?;
                visit(1, offset, &buffers[1])?;
                buffers.iter_mut().for_each(Vec::clear);
                offset += WINDOW_MS;
            }
        }
    }
    if !buffers[0].is_empty() {
        visit(0, offset, &buffers[0])?;
        visit(1, offset, &buffers[1])?;
    }
    Ok(())
}

/// Catch conspicuously missing energetic portions without trusting the model's
/// own `complete` claim. Energy is not proof of speech: rejection preserves the
/// previous transcript and asks for another provider instead of inventing text.
fn check_coverage(samples: &[i16], offset: u64, segments: &[FinalSegment]) -> Result<(), String> {
    let active: Vec<usize> = samples
        .chunks(1600)
        .enumerate()
        .filter_map(|(i, frame)| {
            let power =
                frame.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / frame.len() as f64;
            (power.sqrt() > 184.0).then_some(i) // about -45 dBFS in 100 ms bins
        })
        .collect();
    if active.len() < 10 {
        return Ok(());
    }
    let first = offset + active[0] as u64 * 100;
    let last = offset + (*active.last().unwrap() as u64 + 1) * 100;
    let covered_start = segments.iter().map(|s| s.start_ms).min();
    let covered_end = segments.iter().map(|s| s.end_ms).max();
    if covered_start.is_none_or(|s| s > first.saturating_add(30_000))
        || covered_end.is_none_or(|e| e.saturating_add(30_000) < last)
    {
        return Err(
            "Gemini may have omitted part of a window. The previous transcript was kept.".into(),
        );
    }
    let mut uncovered_run = 0;
    let mut previous = None;
    let mut uncovered_total = 0;
    for bin in active {
        let time = offset + bin as u64 * 100;
        let covered = segments.iter().any(|s| {
            time.saturating_add(3_000) >= s.start_ms && time <= s.end_ms.saturating_add(3_000)
        });
        if covered {
            uncovered_run = 0;
        } else {
            if previous != Some(bin.saturating_sub(1)) {
                uncovered_run = 0;
            }
            uncovered_run += 1;
            uncovered_total += 1;
            if uncovered_run >= 100 || uncovered_total >= 300 {
                return Err("Gemini may have omitted speech within a window. The previous transcript was kept.".into());
            }
        }
        previous = Some(bin);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_keep_channels_separate_and_include_the_partial_tail() {
        let chunks = vec![Ok((
            vec![1; WINDOW_FRAMES + 8000],
            vec![2; WINDOW_FRAMES + 8000],
        ))];
        let mut seen = Vec::new();
        visit_windows(chunks, |channel, offset, samples| {
            assert!(samples.iter().all(|s| *s == i16::from(channel) + 1));
            seen.push((channel, offset, samples.len()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                (0, 0, WINDOW_FRAMES),
                (1, 0, WINDOW_FRAMES),
                (0, WINDOW_MS, 8000),
                (1, WINDOW_MS, 8000)
            ]
        );
    }
    #[test]
    fn a_failed_window_aborts_the_whole_pass() {
        let chunks = vec![Ok((vec![1; WINDOW_FRAMES * 2], vec![2; WINDOW_FRAMES * 2]))];
        let mut calls = 0;
        assert!(visit_windows(chunks, |_, _, _| {
            calls += 1;
            Err("fixture failure".into())
        })
        .is_err());
        assert_eq!(calls, 1);
    }
    #[test]
    fn empty_or_early_ending_response_does_not_replace_energetic_audio() {
        let audio = vec![1000; 16_000 * 120];
        assert!(check_coverage(&audio, 0, &[]).is_err());
        let early = vec![FinalSegment {
            channel: 0,
            speaker: None,
            start_ms: 0,
            end_ms: 1000,
            text: "fixture".into(),
        }];
        assert!(check_coverage(&audio, 0, &early).is_err());
        assert!(check_coverage(&vec![0; 16000], 0, &[]).is_ok());
        let bookends = vec![
            FinalSegment {
                channel: 0,
                speaker: None,
                start_ms: 0,
                end_ms: 5000,
                text: "first".into(),
            },
            FinalSegment {
                channel: 0,
                speaker: None,
                start_ms: 115_000,
                end_ms: 120_000,
                text: "last".into(),
            },
        ];
        assert!(check_coverage(&audio, 0, &bookends).is_err());
    }
}
