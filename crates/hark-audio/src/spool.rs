//! Append-only WAV spool for meeting audio: 16 kHz mono i16, one file per
//! channel at `<data_dir>/meetings/<id>/{me,them}.wav`.
//!
//! A meeting runs for hours and the process can die at any point in it, so the
//! file is always a 44-byte header followed by raw samples. The header's two
//! size fields read 0 until [`SpoolWriter::close`] patches them; after a crash,
//! [`recover`] (run over every spool at startup by [`recover_all`]) patches
//! them from the file length. Either way the result is a WAV any reader
//! accepts, and the only audio a crash can lose is what was still buffered.
//!
//! Pure `std` file I/O: builds and tests the same on every OS.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The spool's sample rate. Equal to [`crate::TARGET_RATE`]: the loopback
/// delivers it directly and the microphone is resampled to it.
pub const SPOOL_RATE: u32 = crate::TARGET_RATE;

/// The microphone channel's file name inside a meeting's directory.
pub const ME_FILE: &str = "me.wav";
/// The system-audio channel's file name inside a meeting's directory.
pub const THEM_FILE: &str = "them.wav";

const HEADER_LEN: u64 = 44;
const BYTES_PER_SAMPLE: u64 = 2;
/// The RIFF size field is a u32 that also counts 36 header bytes. At 16 kHz
/// mono i16 that is ~37 hours, far past any meeting, but a forgotten recording
/// must hit a clean error rather than wrap the header.
const MAX_SAMPLES: u64 = (u32::MAX as u64 - 36) / BYTES_PER_SAMPLE;
/// Samples converted per write: 8 KiB of bytes on the stack.
const WRITE_BATCH: usize = 4096;

/// The canonical 44-byte PCM header for `samples` of 16 kHz mono i16.
fn header(samples: u64) -> [u8; HEADER_LEN as usize] {
    let data = (samples * BYTES_PER_SAMPLE) as u32;
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&SPOOL_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(SPOOL_RATE * BYTES_PER_SAMPLE as u32).to_le_bytes());
    h[32..34].copy_from_slice(&(BYTES_PER_SAMPLE as u16).to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// Rewrite the two size fields in place and flush them to disk.
fn patch_sizes(file: &mut File, samples: u64) -> io::Result<()> {
    let h = header(samples);
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&h[4..8])?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&h[40..44])?;
    file.sync_all()
}

/// f32 in [-1, 1] to i16, the exact inverse of `i16 as f32 / 32768.0`, so a
/// loopback sample that went through the f32 ring comes back bit-identical.
/// Out-of-range input clips rather than wrapping.
pub fn f32_to_i16(s: f32) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// Writes one channel's spool. Create it when the meeting starts, append every
/// drained batch, and [`close`](Self::close) it when the meeting stops.
pub struct SpoolWriter {
    /// `None` once closed, so `Drop` knows the header is already patched.
    file: Option<BufWriter<File>>,
    samples: u64,
}

impl SpoolWriter {
    /// Create a new spool at `path`, making its directory if needed. Never
    /// overwrites: an existing file is an `AlreadyExists` error, because a
    /// reused meeting id must not clobber another meeting's audio.
    ///
    /// The header is on disk when this returns, so the file is a valid (empty)
    /// WAV from the first moment and a crash leaves something [`recover`] can
    /// fix.
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&header(0))?;
        Ok(SpoolWriter {
            file: Some(BufWriter::with_capacity(64 * 1024, file)),
            samples: 0,
        })
    }

    /// Append samples. Fails with `FileTooLarge`, writing nothing, rather than
    /// write past what a WAV header can describe.
    pub fn append(&mut self, samples: &[i16]) -> io::Result<()> {
        self.check_room(samples.len())?;
        let mut bytes = [[0u8; 2]; WRITE_BATCH];
        for batch in samples.chunks(WRITE_BATCH) {
            for (pair, s) in bytes.iter_mut().zip(batch) {
                *pair = s.to_le_bytes();
            }
            self.writer()?
                .write_all(bytes[..batch.len()].as_flattened())?;
            // Counted per batch, so after an I/O error the header still
            // covers exactly what was handed to the OS.
            self.samples += batch.len() as u64;
        }
        Ok(())
    }

    /// Append f32 samples in [-1, 1], converted with [`f32_to_i16`].
    pub fn append_f32(&mut self, samples: &[f32]) -> io::Result<()> {
        self.check_room(samples.len())?;
        let mut converted = [0i16; WRITE_BATCH];
        for batch in samples.chunks(WRITE_BATCH) {
            for (out, &s) in converted.iter_mut().zip(batch) {
                *out = f32_to_i16(s);
            }
            self.append(&converted[..batch.len()])?;
        }
        Ok(())
    }

    /// Push buffered samples to the OS, bounding what a crash can lose. The
    /// header is not patched: that is `close`'s job, or recovery's.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer()?.flush()
    }

    /// Samples appended so far.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Flush, patch the header, and sync. Returns the samples written.
    pub fn close(mut self) -> io::Result<u64> {
        self.finalize()?;
        Ok(self.samples)
    }

    fn finalize(&mut self) -> io::Result<()> {
        let Some(writer) = self.file.take() else {
            return Ok(());
        };
        let mut file = writer.into_inner().map_err(|e| e.into_error())?;
        patch_sizes(&mut file, self.samples)
    }

    fn writer(&mut self) -> io::Result<&mut BufWriter<File>> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("spool already closed"))
    }

    fn check_room(&self, more: usize) -> io::Result<()> {
        if self.samples + more as u64 > MAX_SAMPLES {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "spool reached the WAV size limit",
            ));
        }
        Ok(())
    }
}

impl Drop for SpoolWriter {
    /// A spool dropped without `close` (an unwinding panic, an early return)
    /// still gets its header patched. Recovery would fix it on the next start,
    /// but a readable file now is better; a failure here is logged because
    /// `Drop` has no one to return it to.
    fn drop(&mut self) {
        if let Err(e) = self.finalize() {
            log::warn!(
                "spool dropped without close; header not patched ({e}), recovery will retry"
            );
        }
    }
}

/// What [`recover`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// The header already matched the samples on disk.
    Intact { samples: u64 },
    /// The header was patched to the samples on disk (a torn final sample, if
    /// any, was cut off).
    Repaired { samples: u64 },
    /// Not a spool this module wrote (wrong format, or too short to hold a
    /// header). Left exactly as found.
    Unrecognized,
}

/// Make a spool's header agree with its length. Idempotent: an intact spool
/// is only read, never written.
///
/// Refuses anything that is not byte-for-byte this module's header layout, so
/// pointing it at an unrelated WAV can never rewrite that file.
pub fn recover(path: &Path) -> io::Result<Recovery> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    if len < HEADER_LEN {
        return Ok(Recovery::Unrecognized);
    }
    let mut found = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut found)?;
    let expected = header(0);
    // Everything but the two size fields must match exactly.
    let same_layout = found[0..4] == expected[0..4] && found[8..40] == expected[8..40];
    let on_disk = (len - HEADER_LEN) / BYTES_PER_SAMPLE;
    if !same_layout || on_disk > MAX_SAMPLES {
        return Ok(Recovery::Unrecognized);
    }
    if found == header(on_disk) && (len - HEADER_LEN).is_multiple_of(BYTES_PER_SAMPLE) {
        return Ok(Recovery::Intact { samples: on_disk });
    }
    // A crash mid-write can leave half a sample: drop it.
    file.set_len(HEADER_LEN + on_disk * BYTES_PER_SAMPLE)?;
    patch_sizes(&mut file, on_disk)?;
    Ok(Recovery::Repaired { samples: on_disk })
}

/// Run [`recover`] on every spool under `meetings_dir` (`<id>/me.wav` and
/// `<id>/them.wav`), for the startup pass. One result per spool found; a spool
/// that cannot be opened (another process holding it) does not stop the rest.
///
/// Symlinked directories and files are skipped, never followed, so nothing
/// outside `meetings_dir` can be touched. A missing `meetings_dir` is simply
/// no meetings yet.
pub fn recover_all(meetings_dir: &Path) -> io::Result<Vec<(PathBuf, io::Result<Recovery>)>> {
    let entries = match std::fs::read_dir(meetings_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut results = Vec::new();
    for entry in entries {
        let entry = entry?;
        // DirEntry::file_type does not follow symlinks.
        if !entry.file_type()?.is_dir() {
            continue;
        }
        for name in [ME_FILE, THEM_FILE] {
            let path = entry.path().join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_file() => {
                    let outcome = recover(&path);
                    results.push((path, outcome));
                }
                // Absent (a mic-only meeting has no them.wav) or not a plain file.
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => results.push((path, Err(e))),
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<i16> {
        (0..n)
            .map(|i| (i as i32 * 37 % 65_536 - 32_768) as i16)
            .collect()
    }

    /// Read a spool back with an independent WAV reader.
    fn read_back(path: &Path) -> (hound::WavSpec, Vec<i16>) {
        let mut reader = hound::WavReader::open(path).expect("valid wav");
        let spec = reader.spec();
        let samples = reader
            .samples::<i16>()
            .map(|s| s.expect("sample"))
            .collect();
        (spec, samples)
    }

    /// A spool as a crash leaves it: the header `create` wrote (sizes 0),
    /// then whatever samples had been flushed.
    fn crashed_spool(path: &Path, samples: &[i16]) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let mut bytes = header(0).to_vec();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        std::fs::write(path, bytes).expect("write");
    }

    #[test]
    fn flush_puts_samples_on_disk_before_close() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        let mut writer = SpoolWriter::create(&path).expect("create");
        writer.append(&ramp(1_000)).expect("append");
        writer.flush().expect("flush");
        let on_disk = std::fs::read(&path).expect("read");
        assert_eq!(on_disk.len() as u64, HEADER_LEN + 2_000);
        assert_eq!(on_disk[..44], header(0), "sizes stay 0 until close");
        writer.close().expect("close");
    }

    #[test]
    fn a_closed_spool_is_a_standard_16k_mono_wav() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("m1").join(ME_FILE);
        let audio = ramp(50_000);
        let mut writer = SpoolWriter::create(&path).expect("create");
        // Uneven batches, one larger than the internal conversion batch.
        writer.append(&audio[..123]).expect("append");
        writer.append(&audio[123..10_000]).expect("append");
        writer.append(&audio[10_000..]).expect("append");
        assert_eq!(writer.close().expect("close"), 50_000);

        let (spec, samples) = read_back(&path);
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16_000, 16)
        );
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        assert_eq!(samples, audio);
        assert_eq!(
            std::fs::metadata(&path).expect("meta").len(),
            HEADER_LEN + 100_000
        );
    }

    #[test]
    fn an_empty_spool_is_a_valid_empty_wav() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(THEM_FILE);
        SpoolWriter::create(&path)
            .expect("create")
            .close()
            .expect("close");
        assert!(read_back(&path).1.is_empty());
    }

    #[test]
    fn f32_round_trips_every_i16_exactly() {
        for v in i16::MIN..=i16::MAX {
            assert_eq!(f32_to_i16(v as f32 / 32768.0), v);
        }
        assert_eq!(f32_to_i16(1.5), i16::MAX);
        assert_eq!(f32_to_i16(-1.5), i16::MIN);
    }

    #[test]
    fn append_f32_matches_append_of_the_converted_samples() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        let audio = ramp(9_000);
        let as_f32: Vec<f32> = audio.iter().map(|&v| v as f32 / 32768.0).collect();
        let mut writer = SpoolWriter::create(&path).expect("create");
        writer.append_f32(&as_f32).expect("append");
        assert_eq!(writer.samples(), 9_000);
        writer.close().expect("close");
        assert_eq!(read_back(&path).1, audio);
    }

    #[test]
    fn create_never_overwrites_an_existing_spool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        let mut first = SpoolWriter::create(&path).expect("create");
        first.append(&ramp(100)).expect("append");
        first.close().expect("close");
        let err = SpoolWriter::create(&path)
            .err()
            .expect("second create must fail");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(read_back(&path).1.len(), 100, "first spool untouched");
    }

    #[test]
    fn dropping_without_close_still_patches_the_header() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        {
            let mut writer = SpoolWriter::create(&path).expect("create");
            writer.append(&ramp(700)).expect("append");
        }
        assert_eq!(read_back(&path).1, ramp(700));
    }

    #[test]
    fn the_size_limit_is_a_clean_error_not_a_wrapped_header() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut writer = SpoolWriter::create(&dir.path().join(ME_FILE)).expect("create");
        writer.samples = MAX_SAMPLES - 1;
        let err = writer.append(&[0, 0]).expect_err("past the limit");
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert_eq!(writer.samples, MAX_SAMPLES - 1, "nothing counted");
        writer.append(&[0]).expect("exactly at the limit is fine");
    }

    #[test]
    fn recovery_patches_a_crashed_spool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        crashed_spool(&path, &ramp(12_345));
        assert!(
            hound::WavReader::open(&path)
                .map(|r| r.len() == 0)
                .unwrap_or(true),
            "precondition: the crashed header claims no samples"
        );
        assert_eq!(
            recover(&path).expect("recover"),
            Recovery::Repaired { samples: 12_345 }
        );
        assert_eq!(read_back(&path).1, ramp(12_345));
        // A second pass finds nothing to do.
        assert_eq!(
            recover(&path).expect("recover"),
            Recovery::Intact { samples: 12_345 }
        );
    }

    #[test]
    fn recovery_cuts_off_a_torn_final_sample() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        crashed_spool(&path, &ramp(10));
        OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(&[0xAB]))
            .expect("tear");
        assert_eq!(
            recover(&path).expect("recover"),
            Recovery::Repaired { samples: 10 }
        );
        assert_eq!(
            std::fs::metadata(&path).expect("meta").len(),
            HEADER_LEN + 20
        );
        assert_eq!(read_back(&path).1, ramp(10));
    }

    #[test]
    fn recovery_leaves_a_closed_spool_byte_identical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(ME_FILE);
        let mut writer = SpoolWriter::create(&path).expect("create");
        writer.append(&ramp(500)).expect("append");
        writer.close().expect("close");
        let before = std::fs::read(&path).expect("read");
        assert_eq!(
            recover(&path).expect("recover"),
            Recovery::Intact { samples: 500 }
        );
        assert_eq!(std::fs::read(&path).expect("read"), before);
    }

    #[test]
    fn recovery_refuses_files_it_did_not_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A real WAV in a different format.
        let stereo = dir.path().join("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&stereo, spec).expect("create");
        for s in ramp(200) {
            w.write_sample(s).expect("write");
        }
        w.finalize().expect("finalize");
        // Not a WAV at all, and a file too short for a header.
        let text = dir.path().join("notes.wav");
        std::fs::write(&text, vec![b'x'; 200]).expect("write");
        let short = dir.path().join("short.wav");
        std::fs::write(&short, b"RIFF").expect("write");

        for path in [&stereo, &text, &short] {
            let before = std::fs::read(path).expect("read");
            assert_eq!(recover(path).expect("recover"), Recovery::Unrecognized);
            assert_eq!(std::fs::read(path).expect("read"), before, "{path:?}");
        }
    }

    #[test]
    fn recover_all_fixes_every_crashed_spool_under_meetings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let meetings = dir.path().join("meetings");
        // Meeting a crashed with both channels open; b closed cleanly and
        // recorded only the mic.
        crashed_spool(&meetings.join("a").join(ME_FILE), &ramp(300));
        crashed_spool(&meetings.join("a").join(THEM_FILE), &ramp(200));
        let mut b = SpoolWriter::create(&meetings.join("b").join(ME_FILE)).expect("create");
        b.append(&ramp(100)).expect("append");
        b.close().expect("close");
        // Strays that are not spools of a meeting directory.
        std::fs::write(meetings.join("stray.wav"), b"loose").expect("write");
        std::fs::write(meetings.join("b").join("notes.txt"), b"x").expect("write");

        let mut results: Vec<(String, Recovery)> = recover_all(&meetings)
            .expect("scan")
            .into_iter()
            .map(|(p, r)| {
                let rel = p.strip_prefix(&meetings).expect("inside").to_path_buf();
                (
                    rel.to_string_lossy().replace('\\', "/"),
                    r.expect("recover"),
                )
            })
            .collect();
        results.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(
            results,
            vec![
                ("a/me.wav".into(), Recovery::Repaired { samples: 300 }),
                ("a/them.wav".into(), Recovery::Repaired { samples: 200 }),
                ("b/me.wav".into(), Recovery::Intact { samples: 100 }),
            ]
        );
    }

    #[test]
    fn recover_all_without_a_meetings_dir_finds_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = recover_all(&dir.path().join("meetings")).expect("scan");
        assert!(found.is_empty());
    }
}
