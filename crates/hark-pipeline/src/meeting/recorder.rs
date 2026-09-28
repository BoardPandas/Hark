//! One meeting's capture: the microphone and the system-audio loopback, each
//! drained every ~100 ms into its WAV spool and its live chunker.
//!
//! Both tracks sit on one timeline: 16 kHz samples since the meeting started.
//! A track that opens late is padded with leading silence, and samples the
//! ring lost to a stalled drain are replaced by silence of the same length,
//! so a line's offset is its real time in the call and the two spools stay
//! aligned for the stereo final pass. The mic is resampled from its device
//! rate here; the loopback already arrives at 16 kHz.

use super::echo::EchoReducer;
use hark_audio::resample::StreamResampler;
use hark_audio::ring::{Consumer, RangeError};
use hark_audio::spool::{SpoolWriter, ME_FILE, THEM_FILE};
use hark_audio::{CaptureHandle, LoopbackHandle, LoopbackTarget};
use hark_meeting::{Channel, Chunk, ChunkParams, Chunker};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// Ring length per track. The drain runs every 100 ms, so this only has to
/// cover a stalled coordinator; 10 s is generous and costs ~2 MB at 48 kHz.
const RING_SECONDS: u32 = 10;
const RATE: u64 = hark_meeting::SAMPLE_RATE as u64;

/// A chunk on its way to the live transcriber.
pub(super) type LiveJob = (Channel, Chunk);

enum Source {
    Mic(CaptureHandle),
    Loopback(LoopbackHandle),
    #[cfg(test)]
    Test {
        failed: bool,
    },
    #[cfg(test)]
    FailingDuringDrain(std::cell::Cell<u32>),
}

impl Source {
    fn errored(&self) -> bool {
        match self {
            Source::Mic(h) => h.stream_errored(),
            Source::Loopback(h) => h.stream_errored(),
            #[cfg(test)]
            Source::Test { failed } => *failed,
            #[cfg(test)]
            Source::FailingDuringDrain(checks) => {
                let previous = checks.get();
                checks.set(previous + 1);
                previous > 0
            }
        }
    }

    fn discontinuities(&self) -> u64 {
        match self {
            Source::Mic(h) => h.discontinuities().load(Ordering::Relaxed),
            Source::Loopback(h) => h.discontinuities().load(Ordering::Relaxed),
            #[cfg(test)]
            Source::Test { .. } => 0,
            #[cfg(test)]
            Source::FailingDuringDrain(_) => 0,
        }
    }
}

struct Track {
    channel: Channel,
    consumer: Consumer,
    /// Next absolute ring index to read.
    read: u64,
    rate: u32,
    resampler: Option<StreamResampler>,
    spool: SpoolWriter,
    chunker: Option<Chunker>,
    /// Leading silence placed yet (first delivery).
    aligned: bool,
    /// Samples lost to ring overruns, for the log.
    lost: u64,
    discontinuous: bool,
    capture_discontinuities: u64,
    /// Declared last: fields drop in order, so the spool and chunker are done
    /// with before the capture thread is joined.
    source: Source,
}

/// What stopping produced, for the finisher.
pub(super) struct Recorded {
    pub id: String,
    pub dir: PathBuf,
    /// 16 kHz samples per channel (0 when the channel was never captured).
    pub me_samples: u64,
    pub them_samples: u64,
}

pub(super) struct Recorder {
    pub id: String,
    pub dir: PathBuf,
    started: Instant,
    me: Option<Track>,
    them: Option<Track>,
    live: Option<Sender<LiveJob>>,
    echo: Option<EchoReducer>,
}

/// Why a track closed mid-meeting.
pub(super) struct TrackLost {
    pub channel: Channel,
    pub detail: String,
}

impl Recorder {
    /// Open the tracks. The meeting records with whatever opened: mic only if
    /// the loopback failed (the reason is returned for a notice), loopback
    /// only if the mic failed. Neither is an error.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        id: String,
        dir: PathBuf,
        mic_device: Option<String>,
        loopback: Option<LoopbackTarget>,
        echo_cancellation: bool,
        live: Option<Sender<LiveJob>>,
        chunk: ChunkParams,
    ) -> Result<(Recorder, Vec<String>), String> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create the meeting folder: {e}"))?;
        let started = Instant::now();
        let chunker = || live.is_some().then(|| Chunker::new(chunk));
        let mut notices = Vec::new();

        let me = match hark_audio::start(RING_SECONDS, mic_device) {
            Ok((handle, consumer)) => {
                let rate = handle.sample_rate();
                let spool = SpoolWriter::create(&dir.join(ME_FILE))
                    .map_err(|e| format!("cannot create the microphone recording: {e}"))?;
                Some(Track::new(
                    Channel::Me,
                    Source::Mic(handle),
                    consumer,
                    rate,
                    spool,
                    chunker(),
                )?)
            }
            Err(e) => {
                notices.push(format!("Your microphone could not be recorded: {e}"));
                None
            }
        };
        let them =
            match loopback.map(|target| hark_audio::start_process_loopback(target, RING_SECONDS)) {
                Some(Ok((handle, consumer))) => {
                    let spool = SpoolWriter::create(&dir.join(THEM_FILE))
                        .map_err(|e| format!("cannot create the system-audio recording: {e}"))?;
                    Some(Track::new(
                        Channel::Them,
                        Source::Loopback(handle),
                        consumer,
                        hark_meeting::SAMPLE_RATE,
                        spool,
                        chunker(),
                    )?)
                }
                Some(Err(e)) => {
                    notices.push(format!(
                    "Other people's audio cannot be captured ({e}); recording your microphone only."
                ));
                    None
                }
                None => None,
            };
        if me.is_none() && them.is_none() {
            return Err(notices.join(" "));
        }
        let echo = if echo_cancellation && me.is_some() && them.is_some() {
            match EchoReducer::new() {
                Ok(echo) => Some(echo),
                Err(_) => {
                    notices.push("Speaker echo reduction could not start; recording the original microphone audio.".into());
                    None
                }
            }
        } else {
            None
        };
        Ok((
            Recorder {
                id,
                dir,
                started,
                me,
                them,
                live,
                echo,
            },
            notices,
        ))
    }

    pub fn has_system_audio(&self) -> bool {
        self.them.is_some()
    }

    /// Drain both tracks. A track whose stream died is closed (its audio so
    /// far is kept) and reported; the meeting carries on with the other.
    pub fn pump(&mut self) -> io::Result<Vec<TrackLost>> {
        self.drain(false)
    }

    fn drain(&mut self, finishing: bool) -> io::Result<Vec<TrackLost>> {
        let elapsed = self.started.elapsed().as_millis() as u64;
        let mut batches = [Vec::new(), Vec::new()];
        let mut discontinuity = false;
        let mut ending = finishing || self.me.is_none() || self.them.is_none();
        let mut closing = [false; 2];
        for (i, slot) in [&mut self.me, &mut self.them].into_iter().enumerate() {
            let Some(track) = slot else { continue };
            batches[i] = track.read(elapsed)?;
            discontinuity |= track.take_discontinuity();
            // Reuse this decision when closing. A device can fail while AEC
            // is running; closing on a second read would skip its audio tail.
            closing[i] = finishing || track.source.errored();
            if closing[i] {
                batches[i].extend(track.drain_tail()?);
                ending = true;
            }
        }
        if let Some(echo) = &mut self.echo {
            batches[0] = echo.push(&batches[0], &batches[1], discontinuity, ending);
        }
        for (i, slot) in [&mut self.me, &mut self.them].into_iter().enumerate() {
            if let Some(track) = slot {
                track.place(&batches[i], self.live.as_ref())?;
            }
        }
        let mut lost = Vec::new();
        for (i, slot) in [&mut self.me, &mut self.them].into_iter().enumerate() {
            if closing[i] {
                let track = slot.take().expect("checked above");
                let channel = track.channel;
                track.close(self.live.as_ref())?;
                if !finishing {
                    lost.push(TrackLost {
                        channel,
                        detail: match channel {
                            Channel::Me => {
                                "The microphone stopped; recording continues without it."
                            }
                            Channel::Them => {
                                "System audio stopped; recording continues with your microphone."
                            }
                        }
                        .to_string(),
                    });
                }
            }
        }
        Ok(lost)
    }

    /// No track is left recording.
    pub fn is_empty(&self) -> bool {
        self.me.is_none() && self.them.is_none()
    }

    /// Final drain, flush the chunkers' tails to the transcriber, close the
    /// spools, and stop capture. Dropping the live sender afterwards lets the
    /// transcriber finish its queue and exit.
    pub fn stop(mut self) -> io::Result<Recorded> {
        self.drain(true)?;
        let mut samples = [0u64; 2];
        // Tracks that closed early still have their spool on disk.
        for (i, name) in [ME_FILE, THEM_FILE].into_iter().enumerate() {
            if samples[i] == 0 {
                samples[i] = spool_samples(&self.dir.join(name));
            }
        }
        self.live = None;
        Ok(Recorded {
            id: self.id,
            dir: self.dir,
            me_samples: samples[0],
            them_samples: samples[1],
        })
    }
}

/// Samples in a closed spool, from its length (0 if absent).
pub(super) fn spool_samples(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .map(|m| m.len().saturating_sub(44) / 2)
        .unwrap_or(0)
}

impl Track {
    fn new(
        channel: Channel,
        source: Source,
        consumer: Consumer,
        rate: u32,
        spool: SpoolWriter,
        chunker: Option<Chunker>,
    ) -> Result<Track, String> {
        let resampler = if rate == hark_meeting::SAMPLE_RATE {
            None
        } else {
            Some(
                StreamResampler::new(rate)
                    .map_err(|e| format!("cannot resample the {rate} Hz microphone: {e}"))?,
            )
        };
        Ok(Track {
            channel,
            consumer,
            read: 0,
            rate,
            resampler,
            spool,
            chunker,
            aligned: false,
            lost: 0,
            discontinuous: false,
            capture_discontinuities: 0,
            source,
        })
    }

    fn read(&mut self, elapsed_ms: u64) -> io::Result<Vec<f32>> {
        let total = self.consumer.total_written();
        if total <= self.read {
            return Ok(Vec::new());
        }
        // A stalled drain can let the ring lap us. Keep the timeline honest:
        // what was lost becomes silence of the same length.
        let mut gap = 0;
        let oldest = self.consumer.oldest_available();
        if self.read < oldest {
            gap = oldest - self.read;
            self.read = oldest;
        }
        let device = match self.consumer.read_range(self.read, total) {
            Ok(s) => s,
            Err(RangeError::Overwritten { oldest, .. }) => {
                gap += oldest - self.read;
                self.read = oldest;
                self.consumer
                    .read_range(oldest, total)
                    .map_err(|e| io::Error::other(e.to_string()))?
            }
            Err(e) => return Err(io::Error::other(e.to_string())),
        };
        self.read = total;

        let mut out = Vec::new();
        if gap > 0 {
            self.lost += gap;
            self.discontinuous = true;
            log::warn!(
                "meeting {:?} track: {gap} samples lost to a ring overrun; padded with silence",
                self.channel
            );
            out.resize((gap * RATE / self.rate as u64) as usize, 0.0);
        }
        match &mut self.resampler {
            Some(r) => out.extend(
                r.push(&device)
                    .map_err(|e| io::Error::other(e.to_string()))?,
            ),
            None => out.extend_from_slice(&device),
        }
        if !self.aligned {
            // The first delivery: everything before it on the session clock
            // is leading silence (the stream took this long to open).
            let due = elapsed_ms * RATE / 1000;
            let lead = due.saturating_sub(out.len() as u64) as usize;
            if lead > 0 {
                let mut padded = vec![0.0; lead];
                padded.extend_from_slice(&out);
                out = padded;
            }
            self.aligned = true;
        }
        Ok(out)
    }

    fn take_discontinuity(&mut self) -> bool {
        let count = self.source.discontinuities();
        let changed = count != self.capture_discontinuities;
        self.capture_discontinuities = count;
        std::mem::take(&mut self.discontinuous) || changed
    }

    fn drain_tail(&mut self) -> io::Result<Vec<f32>> {
        match &mut self.resampler {
            Some(r) => r.drain().map_err(|e| io::Error::other(e.to_string())),
            None => Ok(Vec::new()),
        }
    }

    fn place(&mut self, samples: &[f32], live: Option<&Sender<LiveJob>>) -> io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        self.spool.append_f32(samples)?;
        if let Some(chunker) = &mut self.chunker {
            for chunk in chunker.push(samples) {
                send(live, self.channel, chunk);
            }
        }
        Ok(())
    }

    /// Flush the chunker, close the spool, stop capture. The resampler tail
    /// must already have passed through the shared drain and optional AEC.
    /// Returns the samples on the timeline.
    fn close(mut self, live: Option<&Sender<LiveJob>>) -> io::Result<u64> {
        if let Some(chunk) = self.chunker.as_mut().and_then(Chunker::finish) {
            send(live, self.channel, chunk);
        }
        if let Some(chunker) = &self.chunker {
            log::info!(
                "meeting {:?} track: {} empty chunks skipped",
                self.channel,
                chunker.skipped()
            );
        }
        if self.lost > 0 {
            log::warn!(
                "meeting {:?} track: {} samples lost in total",
                self.channel,
                self.lost
            );
        }
        self.spool.close()
    }
}

fn send(live: Option<&Sender<LiveJob>>, channel: Channel, chunk: Chunk) {
    if let Some(tx) = live {
        // The transcriber only goes away at shutdown; a lost chunk then is
        // a lost live line, never lost audio (the spool has it).
        let _ = tx.send((channel, chunk));
    }
}

#[cfg(test)]
#[path = "recorder_tests.rs"]
mod tests;
