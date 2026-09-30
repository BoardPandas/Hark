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
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::spool;
use crate::stereo::{self, SpoolPairChunks};

mod excerpt;
mod pending;
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

/// Decodes an MP3 archive to (left, right) i16 chunks. Mono files (should
/// not occur for our own archives, but a defensive read) duplicate channel 0
/// into both sides.
struct ArchiveDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    /// Decoded-but-not-yet-yielded samples, drained by `next_chunk`.
    pending_l: Vec<i16>,
    pending_r: Vec<i16>,
    finished: bool,
}

impl ArchiveDecoder {
    fn open(path: &Path) -> Result<Self, EncodeError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let fmt_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &MetadataOptions::default())
            .map_err(|e| EncodeError::Decode(e.to_string()))?;
        let format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| EncodeError::Decode("no default audio track".into()))?;
        let track_id = track.id;
        if track.codec_params.sample_rate != Some(spool::SPOOL_RATE) {
            return Err(EncodeError::Decode(
                "meeting archives must use 16 kHz audio".into(),
            ));
        }
        if track
            .codec_params
            .channels
            .is_none_or(|channels| !matches!(channels.count(), 1 | 2))
        {
            return Err(EncodeError::Decode(
                "meeting archives must have one or two channels".into(),
            ));
        }
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| EncodeError::Decode(e.to_string()))?;
        Ok(Self {
            format,
            decoder,
            track_id,
            pending_l: Vec::new(),
            pending_r: Vec::new(),
            finished: false,
        })
    }

    /// Up to `want` (left, right) samples, or `None` once every decoded
    /// frame has been yielded. Draining `pending_*` on every call (rather
    /// than tracking a read cursor) keeps memory bounded to about one
    /// packet's worth regardless of file length.
    fn next_chunk(&mut self, want: usize) -> Result<Option<StereoChunk>, EncodeError> {
        while !self.finished && self.pending_l.len() < want {
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != self.track_id {
                        continue;
                    }
                    let decoded = self
                        .decoder
                        .decode(&packet)
                        .map_err(|e| EncodeError::Decode(e.to_string()))?;
                    let spec = *decoded.spec();
                    let chans = spec.channels.count().max(1);
                    let mut sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                    sb.copy_interleaved_ref(decoded);
                    for frame in sb.samples().chunks_exact(chans) {
                        self.pending_l.push(spool::f32_to_i16(frame[0]));
                        self.pending_r.push(spool::f32_to_i16(frame[chans - 1]));
                    }
                }
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    self.finished = true;
                }
                Err(e) => return Err(EncodeError::Decode(e.to_string())),
            }
        }
        if self.pending_l.is_empty() {
            return Ok(None);
        }
        let take = self.pending_l.len().min(want.max(1));
        let l: Vec<i16> = self.pending_l.drain(..take).collect();
        let r: Vec<i16> = self.pending_r.drain(..take).collect();
        Ok(Some((l, r)))
    }

    /// Decodes everything to completion, returning the (left, right) channels
    /// in full. Test-only: production code stays chunked.
    #[cfg(test)]
    fn decode_all(mut self) -> Result<(Vec<i16>, Vec<i16>), EncodeError> {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        while let Some((cl, cr)) = self.next_chunk(CHUNK_FRAMES)? {
            l.extend(cl);
            r.extend(cr);
        }
        Ok((l, r))
    }
}

#[cfg(test)]
mod tests {
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

    fn meeting_dir_with(
        dir: &Path,
        me_samples: &[i16],
        them_samples: &[i16],
    ) -> (PathBuf, PathBuf) {
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
        let mut bytes =
            Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(samples.len()));
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
        let (me, them) =
            meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.4), &tone(1.0, 550.0, 0.4));

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
        let (me, them) =
            meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.3), &tone(1.0, 440.0, 0.3));
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
        let (me, them) =
            meeting_dir_with(dir.path(), &tone(1.0, 440.0, 0.3), &tone(1.0, 550.0, 0.3));
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

        let (me, them) =
            meeting_dir_with(dir.path(), &tone(0.2, 440.0, 0.3), &tone(0.2, 440.0, 0.3));
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
}
