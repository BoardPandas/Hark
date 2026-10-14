//! The silent-recording precheck run before any final-pass upload.
//!
//! Six meetings once uploaded nothing but silence and came back as "Deepgram
//! returned no transcript", which pointed at the provider and the network
//! when the capture was at fault. A recording whose two channels both stay
//! under the floor is reported as silent and never sent.
//!
//! The floor is a level, never exact zero: process loopback idles near
//! -96 dBFS and a muted mic can still toggle a bit (LL-G HIGH "per-process
//! loopback idles near -96 dBFS"). Loudness is the loudest 100 ms window, so
//! a mostly quiet call with some speech is never judged silent (hark-audio's
//! rule: never a whole-recording mean).

use std::path::Path;

/// Both channels under this, over every 100 ms window, is a silent recording.
pub(super) const FLOOR_DBFS: f64 = -85.0;
/// 100 ms at the spool rate.
const WINDOW: usize = hark_meeting::SAMPLE_RATE as usize / 10;

/// The loudest 100 ms window seen so far on one channel, as a mean square
/// of i16 samples. Windows are consecutive, not sliding: speech spans many
/// of them, so the coarser grid cannot hide it.
#[derive(Default)]
struct Loudest {
    sum_sq: f64,
    len: usize,
    loudest: f64,
}

impl Loudest {
    fn push(&mut self, samples: &[i16]) {
        for &s in samples {
            self.sum_sq += f64::from(s) * f64::from(s);
            self.len += 1;
            if self.len == WINDOW {
                self.close_window();
            }
        }
    }

    fn close_window(&mut self) {
        if self.len > 0 {
            self.loudest = self.loudest.max(self.sum_sq / self.len as f64);
        }
        self.sum_sq = 0.0;
        self.len = 0;
    }

    fn heard(&self) -> bool {
        self.loudest > floor_mean_square()
    }
}

/// [`FLOOR_DBFS`] as a mean square of i16 samples (full scale 32768).
fn floor_mean_square() -> f64 {
    let rms = 32_768.0 * 10f64.powf(FLOOR_DBFS / 20.0);
    rms * rms
}

/// Whether both channels of `chunks` (left = Me, right = Them) stay under
/// the floor. Stops reading as soon as both channels have been heard, so a
/// normal meeting costs a few seconds of audio, not the whole file.
fn chunks_are_silent<I, E>(chunks: I) -> Result<bool, E>
where
    I: IntoIterator<Item = Result<(Vec<i16>, Vec<i16>), E>>,
{
    let (mut me, mut them) = (Loudest::default(), Loudest::default());
    for chunk in chunks {
        let (left, right) = chunk?;
        me.push(&left);
        them.push(&right);
        if me.heard() && them.heard() {
            return Ok(false);
        }
    }
    me.close_window();
    them.close_window();
    Ok(!me.heard() && !them.heard())
}

/// True only when the meeting's audio was read to the end and both channels
/// stayed under the floor. Any read error is "not silent": the upload path
/// then reports the real problem, and a false "silent" would be the app
/// quietly doing nothing (the same bias as hark-audio's loudness gate).
pub(super) fn recording_is_silent(dir: &Path) -> bool {
    let Some(source) = hark_audio::meeting_audio(dir) else {
        return false;
    };
    let Ok(chunks) = hark_audio::mp3::stereo_chunks(&source) else {
        return false;
    };
    chunks_are_silent(chunks).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Chunk<E> = Result<(Vec<i16>, Vec<i16>), E>;

    fn chunks(left: Vec<i16>, right: Vec<i16>) -> Vec<Chunk<()>> {
        left.chunks(4096)
            .zip(right.chunks(4096))
            .map(|(l, r)| Ok((l.to_vec(), r.to_vec())))
            .collect()
    }

    /// A square wave at `amplitude` LSB, `len` samples.
    fn tone(amplitude: i16, len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    const TEN_SECONDS: usize = 160_000;

    #[test]
    fn exact_zero_on_both_channels_is_silent() {
        let silent = chunks(vec![0; TEN_SECONDS], vec![0; TEN_SECONDS]);
        assert_eq!(chunks_are_silent(silent), Ok(true));
    }

    #[test]
    fn an_idle_loopback_and_a_one_lsb_mic_are_still_silent() {
        // Measured on the failing machine: me.wav all zero, them.wav peaking
        // at 1 LSB, process loopback idle near -96 dBFS. Exact-zero checks
        // miss both; the floor catches them.
        let one_lsb = tone(1, TEN_SECONDS);
        let idle = tone(0, TEN_SECONDS);
        assert_eq!(chunks_are_silent(chunks(idle, one_lsb)), Ok(true));
    }

    #[test]
    fn speech_on_either_channel_is_not_silent() {
        // -40 dBFS is quiet speech; one side is enough.
        let quiet_speech = tone(328, TEN_SECONDS);
        assert_eq!(
            chunks_are_silent(chunks(vec![0; TEN_SECONDS], quiet_speech.clone())),
            Ok(false)
        );
        assert_eq!(
            chunks_are_silent(chunks(quiet_speech, vec![0; TEN_SECONDS])),
            Ok(false)
        );
    }

    #[test]
    fn a_single_short_burst_is_judged_by_its_window_not_the_mean() {
        // 100 ms of speech in ten seconds of nothing: the whole-file mean is
        // far under the floor, the loudest window is far above it.
        let mut left = vec![0i16; TEN_SECONDS];
        left[80_000..80_000 + WINDOW].copy_from_slice(&tone(1_000, WINDOW));
        assert_eq!(
            chunks_are_silent(chunks(left, vec![0; TEN_SECONDS])),
            Ok(false)
        );
    }

    #[test]
    fn the_floor_sits_between_idle_capture_and_the_quietest_speech() {
        // 2 LSB RMS is about -84 dBFS, just over the floor; 1 LSB is -90.
        assert!(floor_mean_square() > 1.0);
        assert!(floor_mean_square() < 4.0);
    }

    #[test]
    fn a_read_error_is_reported_not_called_silent() {
        let failing: Vec<Chunk<&str>> = vec![Err("decode")];
        assert_eq!(chunks_are_silent(failing), Err("decode"));
    }

    #[test]
    fn a_missing_recording_is_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!recording_is_silent(dir.path()));
    }

    #[test]
    fn a_silent_spool_pair_on_disk_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        for file in [hark_audio::spool::ME_FILE, hark_audio::spool::THEM_FILE] {
            let mut spool = hark_audio::spool::SpoolWriter::create(&dir.path().join(file)).unwrap();
            spool.append(&vec![0i16; 16_000]).unwrap();
            spool.close().unwrap();
        }
        assert!(recording_is_silent(dir.path()));
    }
}
