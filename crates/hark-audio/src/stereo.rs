//! Streams a meeting's two mono spools (`me.wav`, `them.wav`) as ONE 16 kHz
//! stereo WAV (L = me, R = them) without ever materializing the merged file:
//! a meeting can run for hours, and an hour of 16-bit stereo at 16 kHz is
//! ~115 MB, far too much to buffer for a single upload.
//!
//! A missing or shorter channel is padded with silence out to the longer
//! spool's length, so the two channels stay time-aligned the way the D2
//! Deepgram multichannel pass needs.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::spool::{self, HEADER_LEN};

/// Frames (samples per channel, i.e. per-file byte offset / 2) read per
/// refill: bounds memory to a small multiple of this regardless of meeting
/// length. 16 KiB of interleaved i16 stereo per refill.
const CHUNK_FRAMES: usize = 4096;

/// Two bytes per sample, two channels.
const STEREO_BYTES_PER_FRAME: u64 = 4;

/// One chunk of time-aligned (me, them) i16 samples.
pub(crate) type SpoolChunk = (Vec<i16>, Vec<i16>);

/// Frames (samples per channel) the stereo stream for `me`/`them` will hold:
/// the longer of the two spools. A missing spool counts as zero frames, all
/// silence.
pub fn spool_pair_frames(me: &Path, them: &Path) -> io::Result<u64> {
    Ok(stat_frames(me)?.max(stat_frames(them)?))
}

/// A streamed 16 kHz stereo i16 WAV (canonical 44-byte header, L = me,
/// R = them) built from the two spools. Returns the reader and its exact
/// total byte length, so a caller building an HTTP body can set
/// `Content-Length` without buffering the body first.
pub fn stereo_wav_reader(me: &Path, them: &Path) -> io::Result<(Box<dyn Read + Send>, u64)> {
    let frames = spool_pair_frames(me, them)?;
    let data_bytes = frames.checked_mul(STEREO_BYTES_PER_FRAME).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "combined spool audio overflows a byte count",
        )
    })?;
    // The RIFF size field is a u32 that also counts 36 header bytes, same
    // limit spool::MAX_SAMPLES protects for a mono spool; stereo halves the
    // hours because each frame is twice the bytes.
    if data_bytes > u32::MAX as u64 - 36 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "combined spool audio is too long for a WAV header",
        ));
    }
    let header = stereo_header(frames);
    let body = StereoBody {
        pair: SpoolPairChunks::open(me, them, CHUNK_FRAMES)?,
        buf: Vec::new(),
        pos: 0,
    };
    let total_len = HEADER_LEN + data_bytes;
    let reader: Box<dyn Read + Send> = Box::new(Cursor::new(header).chain(body));
    Ok((reader, total_len))
}

/// The canonical 44-byte PCM header for `frames` of 16 kHz stereo i16
/// (mirrors `spool::header`, but 2 channels / 4-byte frames).
fn stereo_header(frames: u64) -> [u8; HEADER_LEN as usize] {
    let data = (frames * STEREO_BYTES_PER_FRAME) as u32;
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&2u16.to_le_bytes()); // stereo
    h[24..28].copy_from_slice(&spool::SPOOL_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(spool::SPOOL_RATE * STEREO_BYTES_PER_FRAME as u32).to_le_bytes());
    h[32..34].copy_from_slice(&(STEREO_BYTES_PER_FRAME as u16).to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// A spool's frame count from its file length alone (not the header's data
/// field), so a spool whose header has not been patched yet (see
/// `spool::recover`) still reports correctly. Missing entirely is 0 frames.
fn stat_frames(path: &Path) -> io::Result<u64> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.len().saturating_sub(HEADER_LEN) / 2),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
    }
}

/// Opens a spool for reading past its header, or `None` if it does not
/// exist -- a missing channel (mic-only or system-audio-only meeting) is
/// silence, not an error.
fn open_past_header(path: &Path) -> io::Result<Option<File>> {
    match File::open(path) {
        Ok(mut f) => {
            f.seek(SeekFrom::Start(HEADER_LEN))?;
            Ok(Some(f))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reads exactly `n` frames from `file`, silence-padding whatever `left`
/// (this channel's own remaining real frames) falls short of `n`.
fn read_channel_chunk(file: &mut Option<File>, left: &mut u64, n: u64) -> io::Result<Vec<i16>> {
    let mut samples = vec![0i16; n as usize];
    let real = n.min(*left) as usize;
    if real > 0 {
        let file = file
            .as_mut()
            .expect("left > 0 implies open_past_header found the file");
        let mut bytes = vec![0u8; real * 2];
        file.read_exact(&mut bytes)?;
        let (pairs, _remainder) = bytes.as_chunks::<2>();
        for (s, pair) in samples[..real].iter_mut().zip(pairs) {
            *s = i16::from_le_bytes(*pair);
        }
    }
    *left -= real as u64;
    Ok(samples)
}

/// Iterates a spool pair in fixed-size, frame-aligned chunks, padding a
/// missing or shorter channel with silence. Shared by the stereo WAV
/// streamer (this module) and the archive encoder / spool-source mixdown
/// (`mp3.rs`), so the padding/EOF logic exists exactly once.
pub(crate) struct SpoolPairChunks {
    me: Option<File>,
    them: Option<File>,
    me_left: u64,
    them_left: u64,
    remaining: u64,
    chunk_frames: usize,
}

impl SpoolPairChunks {
    pub(crate) fn open(me: &Path, them: &Path, chunk_frames: usize) -> io::Result<Self> {
        let me_frames = stat_frames(me)?;
        let them_frames = stat_frames(them)?;
        Ok(Self {
            me: open_past_header(me)?,
            them: open_past_header(them)?,
            me_left: me_frames,
            them_left: them_frames,
            remaining: me_frames.max(them_frames),
            chunk_frames: chunk_frames.max(1),
        })
    }

    /// The next chunk of up to `chunk_frames` (me_samples, them_samples), or
    /// `None` once every frame (real or padded) has been yielded.
    pub(crate) fn next_chunk(&mut self) -> io::Result<Option<SpoolChunk>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let n = (self.chunk_frames as u64).min(self.remaining);
        let me_samples = read_channel_chunk(&mut self.me, &mut self.me_left, n)?;
        let them_samples = read_channel_chunk(&mut self.them, &mut self.them_left, n)?;
        self.remaining -= n;
        Ok(Some((me_samples, them_samples)))
    }
}

/// The WAV body (everything after the 44-byte header): interleaved L/R i16
/// samples, produced one `SpoolPairChunks` chunk at a time.
struct StereoBody {
    pair: SpoolPairChunks,
    /// Interleaved bytes ready to hand out; refilled once drained.
    buf: Vec<u8>,
    pos: usize,
}

impl Read for StereoBody {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.buf.len() {
            self.refill()?;
            if self.buf.is_empty() {
                return Ok(0);
            }
        }
        let n = (self.buf.len() - self.pos).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl StereoBody {
    fn refill(&mut self) -> io::Result<()> {
        self.buf.clear();
        self.pos = 0;
        if let Some((me, them)) = self.pair.next_chunk()? {
            self.buf.reserve(me.len() * STEREO_BYTES_PER_FRAME as usize);
            for (l, r) in me.iter().zip(them.iter()) {
                self.buf.extend_from_slice(&l.to_le_bytes());
                self.buf.extend_from_slice(&r.to_le_bytes());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spool::SpoolWriter;

    fn ramp(n: usize, start: i16) -> Vec<i16> {
        (0..n).map(|i| start.wrapping_add(i as i16 * 3)).collect()
    }

    fn write_spool(path: &Path, samples: &[i16]) {
        let mut w = SpoolWriter::create(path).expect("create");
        w.append(samples).expect("append");
        w.close().expect("close");
    }

    /// Read a `stereo_wav_reader` output fully into memory and hand it to an
    /// independent WAV reader (hound), the same cross-check `spool.rs` uses.
    fn read_all(me: &Path, them: &Path) -> (hound::WavSpec, Vec<i16>, u64) {
        let (mut reader, len) = stereo_wav_reader(me, them).expect("reader");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).expect("read");
        assert_eq!(
            bytes.len() as u64,
            len,
            "declared length must match actual bytes"
        );
        let mut wav = hound::WavReader::new(Cursor::new(bytes)).expect("valid wav");
        let spec = wav.spec();
        let samples = wav.samples::<i16>().map(|s| s.expect("sample")).collect();
        (spec, samples, len)
    }

    #[test]
    fn equal_length_channels_interleave_correctly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let me = dir.path().join("me.wav");
        let them = dir.path().join("them.wav");
        write_spool(&me, &ramp(100, 0));
        write_spool(&them, &ramp(100, 1000));

        let (spec, samples, _) = read_all(&me, &them);
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (2, 16_000, 16)
        );
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        assert_eq!(samples.len(), 200);
        // Interleaved: even indices are L (me), odd are R (them).
        let me_ramp = ramp(100, 0);
        let them_ramp = ramp(100, 1000);
        for i in 0..100 {
            assert_eq!(samples[2 * i], me_ramp[i], "L sample {i}");
            assert_eq!(samples[2 * i + 1], them_ramp[i], "R sample {i}");
        }
    }

    #[test]
    fn shorter_channel_is_padded_with_silence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let me = dir.path().join("me.wav");
        let them = dir.path().join("them.wav");
        write_spool(&me, &ramp(100, 0));
        write_spool(&them, &ramp(40, 500));

        let (_, samples, _) = read_all(&me, &them);
        assert_eq!(samples.len(), 200);
        let them_ramp = ramp(40, 500);
        for i in 0..40 {
            assert_eq!(samples[2 * i + 1], them_ramp[i]);
        }
        for i in 40..100 {
            assert_eq!(samples[2 * i + 1], 0, "padded silence at frame {i}");
        }
    }

    #[test]
    fn missing_channel_is_entirely_silence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let me = dir.path().join("me.wav");
        let them = dir.path().join("them.wav"); // never created
        write_spool(&me, &ramp(50, 0));

        let (_, samples, _) = read_all(&me, &them);
        assert_eq!(samples.len(), 100);
        for i in 0..50 {
            assert_eq!(samples[2 * i + 1], 0);
        }
    }

    #[test]
    fn spool_pair_frames_matches_the_longer_spool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let me = dir.path().join("me.wav");
        let them = dir.path().join("them.wav");
        write_spool(&me, &ramp(300, 0));
        write_spool(&them, &ramp(120, 0));
        assert_eq!(spool_pair_frames(&me, &them).expect("frames"), 300);
    }

    #[test]
    fn reader_works_across_a_chunk_boundary() {
        // Longer than CHUNK_FRAMES so refill() runs more than once.
        let dir = tempfile::tempdir().expect("tempdir");
        let me = dir.path().join("me.wav");
        let them = dir.path().join("them.wav");
        let n = CHUNK_FRAMES + 100;
        write_spool(&me, &ramp(n, 0));
        write_spool(&them, &ramp(n, 0));
        let (_, samples, len) = read_all(&me, &them);
        assert_eq!(samples.len(), 2 * n);
        assert_eq!(len, HEADER_LEN + (n as u64) * STEREO_BYTES_PER_FRAME);
    }
}
