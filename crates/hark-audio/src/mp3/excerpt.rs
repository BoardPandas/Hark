//! Sample-accurate selection after decoding; never cuts compressed MP3 bytes.

use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Export the mono excerpt `[start_ms, end_ms)` as WAV. Returns the frames
/// actually written (the archive can end slightly before its recorded duration
/// because of MP3 gapless trimming). Memory is bounded to one codec chunk.
pub fn export_excerpt_wav(
    src: &MeetingAudio,
    out: &Path,
    start_ms: u64,
    end_ms: u64,
) -> Result<u64, EncodeError> {
    let mut selected = Selection::new(src, start_ms, end_ms)?;
    let (pending, file) = PendingFile::new(out)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: spool::SPOOL_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(file, spec).map_err(io::Error::other)?;
    let mut frames = 0;
    while let Some(mono) = selected.next()? {
        frames += mono.len() as u64;
        for sample in mono {
            writer.write_sample(sample).map_err(io::Error::other)?;
        }
    }
    require_audio(frames)?;
    writer.finalize().map_err(io::Error::other)?;
    pending.commit(out)?;
    Ok(frames)
}

/// Decode, select and re-encode a mono MP3 excerpt. Returns selected PCM
/// frames, allowing the matching transcript to stop at the actual audio end.
pub fn export_excerpt_mp3(
    src: &MeetingAudio,
    out: &Path,
    start_ms: u64,
    end_ms: u64,
) -> Result<u64, EncodeError> {
    let mut selected = Selection::new(src, start_ms, end_ms)?;
    let mut encoder = lame_builder(1, Mode::Mono, Bitrate::Kbps32)?;
    let mut buf = Vec::new();
    let mut frames = 0;
    while let Some(mono) = selected.next()? {
        frames += mono.len() as u64;
        buf.reserve(mp3lame_encoder::max_required_buffer_size(mono.len()));
        encoder
            .encode_to_vec(MonoPcm(&mono), &mut buf)
            .map_err(|e| EncodeError::Lame(e.to_string()))?;
    }
    require_audio(frames)?;
    finish_lame(&mut encoder, &mut buf)?;
    let (pending, mut file) = PendingFile::new(out)?;
    file.write_all(&buf)?;
    file.sync_all()?;
    drop(file);
    pending.commit(out)?;
    Ok(frames)
}

fn require_audio(frames: u64) -> Result<(), EncodeError> {
    if frames == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the selected range contains no saved audio",
        )
        .into());
    }
    Ok(())
}

struct Selection {
    source: MixSource,
    position: u64,
    start: u64,
    end: u64,
}

impl Selection {
    fn new(src: &MeetingAudio, start_ms: u64, end_ms: u64) -> Result<Self, EncodeError> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "choose a non-empty audio range",
            )
        };
        if start_ms >= end_ms {
            return Err(invalid().into());
        }
        let samples = |ms: u64| {
            ms.checked_mul(u64::from(spool::SPOOL_RATE))
                .map(|n| n / 1000)
                .ok_or_else(invalid)
        };
        Ok(Self {
            source: MixSource::open(src, CHUNK_FRAMES)?,
            position: 0,
            start: samples(start_ms)?,
            end: samples(end_ms)?,
        })
    }

    fn next(&mut self) -> Result<Option<Vec<i16>>, EncodeError> {
        while self.position < self.end {
            let Some((left, right)) = self.source.next_chunk()? else {
                return Ok(None);
            };
            let chunk_start = self.position;
            self.position += left.len() as u64;
            let from = self
                .start
                .saturating_sub(chunk_start)
                .min(left.len() as u64) as usize;
            let until = self.end.saturating_sub(chunk_start).min(left.len() as u64) as usize;
            if from < until {
                return Ok(Some(mix_mono(&left[from..until], &right[from..until])));
            }
        }
        Ok(None)
    }
}

/// Own only the unique file we created; a failed export never touches an
/// existing destination or a similarly named user's temporary file.
struct PendingFile(PathBuf);
impl PendingFile {
    fn new(out: &Path) -> io::Result<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut name = out.file_name().unwrap_or_default().to_os_string();
        name.push(format!(
            ".hark-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = out.with_file_name(name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok((Self(path), file))
    }

    fn commit(self, out: &Path) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&self.0)?
            .sync_all()?;
        std::fs::rename(&self.0, out)
    }
}
impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spools(dir: &Path) -> MeetingAudio {
        let me = dir.join("me.wav");
        let them = dir.join("them.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: spool::SPOOL_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&me, spec).unwrap();
        for i in 0..32_000 {
            writer.write_sample((i % 10_000) as i16).unwrap();
        }
        writer.finalize().unwrap();
        MeetingAudio::Spools { me, them }
    }

    #[test]
    fn wav_range_cuts_on_exact_pcm_samples_across_codec_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let src = spools(dir.path());
        let out = dir.path().join("excerpt.wav");
        assert_eq!(export_excerpt_wav(&src, &out, 257, 1103).unwrap(), 846 * 16);
        let samples: Vec<i16> = hound::WavReader::open(out)
            .unwrap()
            .samples()
            .map(Result::unwrap)
            .collect();
        assert_eq!(samples.len(), 846 * 16);
        assert_eq!(samples[0], 4112 / 2);
        assert_eq!(*samples.last().unwrap(), (17647 % 10_000) / 2);
    }

    #[test]
    fn archive_ranges_decode_before_cutting_and_clip_at_eof() {
        let dir = tempfile::tempdir().unwrap();
        let _ = spools(dir.path());
        compress_meeting_dir(dir.path()).unwrap();
        let src = meeting_audio(dir.path()).unwrap();
        let wav = dir.path().join("excerpt.wav");
        assert_eq!(export_excerpt_wav(&src, &wav, 300, 900).unwrap(), 9600);
        let frames = export_excerpt_wav(&src, &wav, 1500, 5000).unwrap();
        assert!(frames > 0 && frames <= 8000);
        let mp3 = dir.path().join("excerpt.mp3");
        assert_eq!(export_excerpt_mp3(&src, &mp3, 200, 1200).unwrap(), 16_000);
        verify_mp3(&mp3, 16_000).unwrap();
    }

    #[test]
    fn invalid_or_out_of_audio_ranges_leave_destination_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let src = spools(dir.path());
        let out = dir.path().join("saved.wav");
        std::fs::write(&out, b"keep").unwrap();
        for (start, end) in [(100, 100), (500, 200), (4000, 5000), (0, u64::MAX)] {
            assert!(export_excerpt_wav(&src, &out, start, end).is_err());
            assert_eq!(std::fs::read(&out).unwrap(), b"keep");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
