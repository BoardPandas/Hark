//! Meeting audio sharing (D8) and archiving (D9): a mono share export
//! (40 kbps MP3 or 16 kHz WAV mixdown) and the one-file stereo 64 kbps MP3
//! archive that replaces the two WAV spools once a meeting is processed.
//!
//! LAME mode matters: the archive uses `Mode::Stereo`, never
//! `Mode::JointStereo` -- joint stereo shares bits between channels and
//! leaks one side into the other at about -96 dBFS, which would corrupt a
//! re-run through Deepgram multichannel. `Mode::Stereo` keeps the sides
//! bit-independent (confirmed in the CP0 spike).
//!
//! Every `encode_to_vec`/`flush_to_vec` call reserves its buffer first
//! (LL-G HIGH `mp3lame-encode-to-vec-no-reserve.md`): those helpers pass the
//! `Vec`'s spare capacity straight to LAME, and LAME reads a spare capacity
//! of 0 as "unbounded" rather than "none available".

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use mp3lame_encoder::{Bitrate, DualPcm, Encoder as LameEncoder, FlushGap, Mode, MonoPcm, Quality};

use crate::spool;
use crate::stereo::{self, SpoolPairChunks};

mod decode;
mod excerpt;
mod pending;
use decode::ArchiveDecoder;
pub use excerpt::{export_excerpt_mp3, export_excerpt_wav};
use pending::PendingFile;

/// The archive file name inside a meeting's directory.
pub const ARCHIVE_FILE: &str = "audio.mp3";

/// Frames (samples per channel) handled per encode/decode step. Bounds
/// memory to a small multiple of this regardless of meeting length.
const CHUNK_FRAMES: usize = 4096;

// At 16 kHz, a 32 kbps mono frame is too small for LAME's gapless tag,
// so LAME silently omits it. 40 kbps is the smallest CBR rate that fits.
const MONO_EXPORT_BITRATE: Bitrate = Bitrate::Kbps40;

/// One chunk of time-aligned (left, right) i16 samples.
type StereoChunk = (Vec<i16>, Vec<i16>);

/// A bounded decoder for saved meeting audio, always 16 kHz PCM. Each item
/// owns at most 4096 frames per channel, with shorter final chunks.
pub struct MeetingAudioChunks {
    source: MixSource,
    finished: bool,
}

pub fn stereo_chunks(src: &MeetingAudio) -> Result<MeetingAudioChunks, EncodeError> {
    Ok(MeetingAudioChunks {
        source: MixSource::open(src, CHUNK_FRAMES)?,
        finished: false,
    })
}

impl Iterator for MeetingAudioChunks {
    type Item = Result<(Vec<i16>, Vec<i16>), EncodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.source.next_chunk() {
            Ok(Some(chunk)) => Some(Ok(chunk)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("LAME encoder error: {0}")]
    Lame(String),
    #[error("MP3 decode error: {0}")]
    Decode(String),
    #[error("archive verification failed: {0}")]
    Verify(String),
}

/// Where a meeting's audio currently lives.
pub enum MeetingAudio {
    /// The two lossless WAV spools, not yet compressed.
    Spools { me: PathBuf, them: PathBuf },
    /// The compressed stereo archive (§4.11); the WAVs are gone.
    Archive(PathBuf),
}

/// Spools if either WAV exists, else the archive if it exists, else `None`
/// (nothing recorded, or the audio was evicted under the storage cap).
pub fn meeting_audio(dir: &Path) -> Option<MeetingAudio> {
    let me = dir.join(spool::ME_FILE);
    let them = dir.join(spool::THEM_FILE);
    if me.exists() || them.exists() {
        return Some(MeetingAudio::Spools { me, them });
    }
    let archive = dir.join(ARCHIVE_FILE);
    if archive.exists() {
        return Some(MeetingAudio::Archive(archive));
    }
    None
}

/// Shareable mono mixdown ((L + R) / 2, never clips: an i16 average of two
/// i16s always fits an i16) at 40 kbps, with gapless timing metadata.
pub fn export_mono_mp3(src: &MeetingAudio, out: &Path) -> Result<(), EncodeError> {
    let mut encoder = lame_builder(1, Mode::Mono, MONO_EXPORT_BITRATE)?;
    let mut mix = MixSource::open(src, CHUNK_FRAMES)?;
    let mut buf = Vec::new();
    while let Some((l, r)) = mix.next_chunk()? {
        let mono = mix_mono(&l, &r);
        buf.reserve(mp3lame_encoder::max_required_buffer_size(mono.len()));
        encoder
            .encode_to_vec(MonoPcm(&mono), &mut buf)
            .map_err(|e| EncodeError::Lame(e.to_string()))?;
    }
    finish_lame(&mut encoder, &mut buf)?;
    let (pending, mut file) = PendingFile::new(out)?;
    file.write_all(&buf)?;
    drop(file);
    pending.commit(out)?;
    Ok(())
}

/// Shareable mono 16 kHz i16 WAV mixdown, same mix as [`export_mono_mp3`].
pub fn export_mono_wav(src: &MeetingAudio, out: &Path) -> Result<(), EncodeError> {
    let (pending, file) = PendingFile::new(out)?;
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: spool::SPOOL_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(file, spec).map_err(io::Error::other)?;
        let mut mix = MixSource::open(src, CHUNK_FRAMES)?;
        while let Some((l, r)) = mix.next_chunk()? {
            for s in mix_mono(&l, &r) {
                writer.write_sample(s).map_err(io::Error::other)?;
            }
        }
        writer.finalize().map_err(io::Error::other)?;
    }
    pending.commit(out)?;
    Ok(())
}

/// The outcome of [`compress_meeting_dir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressOutcome {
    Compressed {
        archive_bytes: u64,
        wav_bytes_freed: u64,
    },
    /// No spools to compress: either nothing was recorded, or this meeting
    /// is already archived.
    NothingToDo,
}

/// `me.wav` + `them.wav` -> `audio.mp3.tmp` (stereo 64 kbps, LAME `STEREO`
/// mode), fsync, verify against the WAVs' combined length, rename to
/// `audio.mp3`, then delete the WAVs. Any failure leaves the WAVs untouched
/// and removes the `.tmp`.
pub fn compress_meeting_dir(dir: &Path) -> Result<CompressOutcome, EncodeError> {
    let me = dir.join(spool::ME_FILE);
    let them = dir.join(spool::THEM_FILE);
    let (me_exists, them_exists) = (me.exists(), them.exists());
    if !me_exists && !them_exists {
        return Ok(CompressOutcome::NothingToDo);
    }

    let expected_frames = stereo::spool_pair_frames(&me, &them)?;
    let wav_bytes_freed = file_len(&me)? + file_len(&them)?;

    let tmp = dir.join(format!("{ARCHIVE_FILE}.tmp"));
    let outcome =
        encode_stereo_archive(&me, &them, &tmp).and_then(|()| verify_mp3(&tmp, expected_frames));
    if let Err(e) = outcome {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    let archive = dir.join(ARCHIVE_FILE);
    std::fs::rename(&tmp, &archive)?;
    if me_exists {
        std::fs::remove_file(&me)?;
    }
    if them_exists {
        std::fs::remove_file(&them)?;
    }
    let archive_bytes = std::fs::metadata(&archive)?.len();
    Ok(CompressOutcome::Compressed {
        archive_bytes,
        wav_bytes_freed,
    })
}

/// What [`recover_meeting_dir`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverAction {
    /// Nothing needed doing.
    None,
    /// An interrupted `audio.mp3.tmp` was removed; the WAVs are intact and
    /// compression can re-run later.
    RemovedPartial,
    /// The archive from an interrupted run was already good: the WAVs it
    /// was compressed from are now deleted.
    FinishedCompression,
    /// The archive from an interrupted run was corrupt: it was deleted, and
    /// the WAVs are kept so compression can re-run.
    DiscardedBadArchive,
}

/// Startup recovery for one meeting directory. Must run before
/// [`meeting_audio`] or [`compress_meeting_dir`] are trusted on a directory
/// that might have crashed mid-compression.
pub fn recover_meeting_dir(dir: &Path) -> Result<RecoverAction, EncodeError> {
    let tmp = dir.join(format!("{ARCHIVE_FILE}.tmp"));
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
        return Ok(RecoverAction::RemovedPartial);
    }

    let archive = dir.join(ARCHIVE_FILE);
    let me = dir.join(spool::ME_FILE);
    let them = dir.join(spool::THEM_FILE);
    let (me_exists, them_exists) = (me.exists(), them.exists());
    if !archive.exists() || !(me_exists || them_exists) {
        return Ok(RecoverAction::None);
    }

    let expected_frames = stereo::spool_pair_frames(&me, &them)?;
    match verify_mp3(&archive, expected_frames) {
        Ok(()) => {
            if me_exists {
                std::fs::remove_file(&me)?;
            }
            if them_exists {
                std::fs::remove_file(&them)?;
            }
            Ok(RecoverAction::FinishedCompression)
        }
        Err(_) => {
            std::fs::remove_file(&archive)?;
            Ok(RecoverAction::DiscardedBadArchive)
        }
    }
}

/// Decode fully; `Ok` if it decodes and its duration is within ±1 s of
/// `expected_frames` at 16 kHz.
pub fn verify_mp3(path: &Path, expected_frames: u64) -> Result<(), EncodeError> {
    let mut decoder = ArchiveDecoder::open(path)?;
    let mut frames = 0u64;
    while let Some((l, _r)) = decoder.next_chunk(CHUNK_FRAMES)? {
        frames += l.len() as u64;
    }
    let diff = frames.abs_diff(expected_frames);
    if diff > spool::SPOOL_RATE as u64 {
        return Err(EncodeError::Verify(format!(
            "decoded {frames} frames, expected {expected_frames} (diff {diff} exceeds the \
             {}-frame / 1 s tolerance)",
            spool::SPOOL_RATE
        )));
    }
    Ok(())
}

// ---- internals ----

fn file_len(path: &Path) -> io::Result<u64> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.len()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
fn tmp_path_for(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    out.with_file_name(name)
}

/// (L + R) / 2. The sum of two `i16`s always fits an `i32`, and halving it
/// always fits back in an `i16` (the extremes are ±32768/±32767 exactly), so
/// this never needs to clamp.
fn mix_mono(l: &[i16], r: &[i16]) -> Vec<i16> {
    l.iter()
        .zip(r)
        .map(|(&a, &b)| ((a as i32 + b as i32) / 2) as i16)
        .collect()
}

fn lame_builder(channels: u8, mode: Mode, brate: Bitrate) -> Result<LameEncoder, EncodeError> {
    let mut b = mp3lame_encoder::Builder::new()
        .ok_or_else(|| EncodeError::Lame("failed to allocate a LAME encoder".into()))?;
    b.set_num_channels(channels)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    b.set_sample_rate(spool::SPOOL_RATE)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    b.set_brate(brate)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    b.set_mode(mode)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    b.set_quality(Quality::Good)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    b.build().map_err(|e| EncodeError::Lame(e.to_string()))
}

/// Flushes the encoder and patches in the LAME/Info tag so decoders can
/// trim the encoder delay/padding (gapless, exact duration on decode).
fn finish_lame(encoder: &mut LameEncoder, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    out.reserve(7200);
    // This is the end of a standalone file. FlushNoGap only drains the
    // bitstream for a continuing encode and leaves final PCM unencoded.
    // FlushGap encodes that tail and records padding for gapless trimming.
    encoder
        .flush_to_vec::<FlushGap>(out)
        .map_err(|e| EncodeError::Lame(e.to_string()))?;
    let mut tag = Vec::with_capacity(encoder.lame_tag_size().max(1));
    encoder.lame_tag_encode_to_vec(&mut tag).ok_or_else(|| {
        EncodeError::Lame("missing gapless timing tag after finalizing MP3".into())
    })?;
    let at = encoder.id3v2_tag_size();
    let header = out.get_mut(at..at + tag.len()).ok_or_else(|| {
        EncodeError::Lame("gapless timing tag does not fit the MP3 header".into())
    })?;
    header.copy_from_slice(&tag);
    Ok(())
}

/// `me.wav` + `them.wav` -> stereo 64 kbps MP3 bytes at `tmp`, fsynced.
fn encode_stereo_archive(me: &Path, them: &Path, tmp: &Path) -> Result<(), EncodeError> {
    let mut encoder = lame_builder(2, Mode::Stereo, Bitrate::Kbps64)?;
    let mut pair = SpoolPairChunks::open(me, them, CHUNK_FRAMES)?;
    let mut buf = Vec::new();
    while let Some((left, right)) = pair.next_chunk()? {
        buf.reserve(mp3lame_encoder::max_required_buffer_size(left.len()));
        encoder
            .encode_to_vec(
                DualPcm {
                    left: &left,
                    right: &right,
                },
                &mut buf,
            )
            .map_err(|e| EncodeError::Lame(e.to_string()))?;
    }
    finish_lame(&mut encoder, &mut buf)?;

    let file = File::create(tmp)?;
    {
        use std::io::Write;
        let mut writer = std::io::BufWriter::new(&file);
        writer.write_all(&buf)?;
        writer.flush()?;
    }
    file.sync_all()?;
    Ok(())
}

/// Either a spool pair or an MP3 archive, exposed as the same
/// chunk-at-a-time (left, right) i16 stream so the mono exports do not care
/// which source they are mixing down.
enum MixSource {
    Spools(SpoolPairChunks),
    Archive(ArchiveDecoder),
}

impl MixSource {
    fn open(src: &MeetingAudio, chunk_frames: usize) -> Result<Self, EncodeError> {
        match src {
            MeetingAudio::Spools { me, them } => {
                Ok(Self::Spools(SpoolPairChunks::open(me, them, chunk_frames)?))
            }
            MeetingAudio::Archive(path) => Ok(Self::Archive(ArchiveDecoder::open(path)?)),
        }
    }

    fn next_chunk(&mut self) -> Result<Option<StereoChunk>, EncodeError> {
        match self {
            Self::Spools(s) => Ok(s.next_chunk()?),
            Self::Archive(a) => a.next_chunk(CHUNK_FRAMES),
        }
    }
}

#[cfg(test)]
mod tests;
