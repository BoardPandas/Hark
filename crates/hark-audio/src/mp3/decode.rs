//! The decode side: a saved meeting MP3 as bounded (left, right) i16 chunks.

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use super::*;

/// Decodes an MP3 archive to (left, right) i16 chunks. Mono files (should
/// not occur for our own archives, but a defensive read) duplicate channel 0
/// into both sides.
pub(super) struct ArchiveDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Decoded-but-not-yet-yielded samples, drained by `next_chunk`.
    pending_l: Vec<i16>,
    pending_r: Vec<i16>,
    finished: bool,
}

impl ArchiveDecoder {
    pub(super) fn open(path: &Path) -> Result<Self, EncodeError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| EncodeError::Decode(e.to_string()))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| EncodeError::Decode("no default audio track".into()))?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| EncodeError::Decode("no audio codec parameters".into()))?;
        if params.sample_rate != Some(spool::SPOOL_RATE) {
            return Err(EncodeError::Decode(
                "meeting archives must use 16 kHz audio".into(),
            ));
        }
        if params
            .channels
            .as_ref()
            .is_none_or(|channels| !matches!(channels.count(), 1 | 2))
        {
            return Err(EncodeError::Decode(
                "meeting archives must have one or two channels".into(),
            ));
        }
        // Gapless trimming (the LAME/Info tag's encoder delay and padding) is
        // a decoder option since symphonia 0.6, on by default; the duration
        // check in verify depends on it.
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(true))
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
    pub(super) fn next_chunk(&mut self, want: usize) -> Result<Option<StereoChunk>, EncodeError> {
        while !self.finished && self.pending_l.len() < want {
            match self.format.next_packet() {
                Ok(Some(packet)) => {
                    if packet.track_id != self.track_id {
                        continue;
                    }
                    let decoded = self
                        .decoder
                        .decode(&packet)
                        .map_err(|e| EncodeError::Decode(e.to_string()))?;
                    let chans = decoded.spec().channels().count().max(1);
                    let mut samples = Vec::<f32>::new();
                    decoded.copy_to_vec_interleaved(&mut samples);
                    for frame in samples.chunks_exact(chans) {
                        self.pending_l.push(spool::f32_to_i16(frame[0]));
                        self.pending_r.push(spool::f32_to_i16(frame[chans - 1]));
                    }
                }
                Ok(None) => self.finished = true,
                // A file cut off mid-frame (crash during write) still yields
                // everything decoded before the cut, as it did on 0.5, where
                // this was the only end-of-stream signal.
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
    pub(super) fn decode_all(mut self) -> Result<(Vec<i16>, Vec<i16>), EncodeError> {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        while let Some((cl, cr)) = self.next_chunk(CHUNK_FRAMES)? {
            l.extend(cl);
            r.extend(cr);
        }
        Ok((l, r))
    }
}
