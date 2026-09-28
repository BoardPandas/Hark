//! Bounded pairing on the recorder's existing 16 kHz session timeline.
//!
//! Capture clocks are independent: these indices are not hardware timestamps.
//! AEC3 estimates acoustic delay; this layer only pairs available samples and
//! preserves microphone audio whenever a usable reference is unavailable.

use hark_audio::meeting_aec::{MeetingAec, MeetingAecError, FRAME_SAMPLES};
use std::collections::VecDeque;

const MAX_PENDING: usize = 4_000; // 250 ms of microphone audio
const MAX_REFERENCE: usize = 32_000; // two seconds, independent of call length

pub(super) trait FrameProcessor {
    fn process(&mut self, render: &[f32], mic: &[f32], out: &mut [f32]) -> bool;
    fn reset(&mut self) -> bool;
    fn delay(&self) -> usize {
        0
    }
}

impl FrameProcessor for MeetingAec {
    fn process(&mut self, render: &[f32], mic: &[f32], out: &mut [f32]) -> bool {
        self.process_frame(render, mic, out).is_ok()
    }

    fn reset(&mut self) -> bool {
        MeetingAec::reset(self).is_ok()
    }

    fn delay(&self) -> usize {
        hark_audio::meeting_aec::OUTPUT_DELAY_SAMPLES
    }
}

pub(super) struct EchoReducer<P = MeetingAec> {
    processor: Option<P>,
    mic: VecDeque<f32>,
    render: VecDeque<f32>,
    emitted: u64,
    render_start: u64,
    render_end: u64,
    needs_reset: bool,
    /// Original samples whose delayed processed output has not arrived yet.
    retained: VecDeque<f32>,
    warmup: usize,
}

impl EchoReducer {
    pub fn new() -> Result<Self, MeetingAecError> {
        Ok(Self::with_processor(MeetingAec::new()?))
    }
}

impl<P: FrameProcessor> EchoReducer<P> {
    fn with_processor(processor: P) -> Self {
        let warmup = processor.delay();
        Self {
            processor: Some(processor),
            mic: VecDeque::with_capacity(MAX_PENDING + FRAME_SAMPLES),
            render: VecDeque::with_capacity(MAX_REFERENCE),
            emitted: 0,
            render_start: 0,
            render_end: 0,
            needs_reset: false,
            retained: VecDeque::with_capacity(warmup + FRAME_SAMPLES),
            warmup,
        }
    }

    /// Each input is the next consecutive batch from its track. No callback
    /// calls this. `finish` flushes even an incomplete final microphone frame;
    /// use it also after either capture track has permanently stopped.
    pub fn push(
        &mut self,
        mic: &[f32],
        render: &[f32],
        discontinuity: bool,
        finish: bool,
    ) -> Vec<f32> {
        let mut output = Vec::new();
        if discontinuity {
            self.bypass_pending(&mut output);
            self.render.clear();
            self.render_start = self.render_end;
            self.needs_reset = true;
        }
        self.render_end += render.len() as u64;
        // Never retain an arbitrarily large batch, even after a stalled drain.
        if render.len() >= MAX_REFERENCE {
            self.render.clear();
            self.render.extend(&render[render.len() - MAX_REFERENCE..]);
            self.render_start = self.render_end - MAX_REFERENCE as u64;
        } else {
            self.discard_reference_before(self.render_end.saturating_sub(MAX_REFERENCE as u64));
            self.render.extend(render);
        }
        // Feed bounded slices so a long stalled drain cannot permanently grow
        // the microphone FIFO allocation beyond its waiting budget + one frame.
        for chunk in mic.chunks(FRAME_SAMPLES) {
            self.mic.extend(chunk);
            self.process_pending(&mut output, false);
        }
        self.process_pending(&mut output, finish);
        self.discard_reference_before(self.emitted);
        output
    }

    fn process_pending(&mut self, output: &mut Vec<f32>, finish: bool) {
        if self.processor.is_none() {
            self.bypass_pending(output);
            return;
        }

        while self.mic.len() >= FRAME_SAMPLES {
            let end = self.emitted + FRAME_SAMPLES as u64;
            let available = self.emitted >= self.render_start && end <= self.render_end;
            if !available
                && self.emitted >= self.render_start
                && !finish
                && self.mic.len() <= MAX_PENDING
            {
                break; // reference may arrive in the next coordinator drain
            }
            let mut capture = [0.0; FRAME_SAMPLES];
            for sample in &mut capture {
                *sample = self.mic.pop_front().expect("complete frame checked");
            }
            let mut cleaned = [0.0; FRAME_SAMPLES];
            let mut succeeded = false;
            if available {
                let mut reference = [0.0; FRAME_SAMPLES];
                let offset = (self.emitted - self.render_start) as usize;
                for (i, sample) in reference.iter_mut().enumerate() {
                    *sample = self.render[offset + i];
                }
                let processor = self.processor.as_mut().expect("checked above");
                let reset_ok = !self.needs_reset || processor.reset();
                if self.needs_reset && reset_ok {
                    self.warmup = processor.delay();
                }
                if reset_ok
                    && processor.process(&reference, &capture, &mut cleaned)
                    && cleaned.iter().all(|sample| sample.is_finite())
                {
                    succeeded = true;
                    self.needs_reset = false;
                } else {
                    // A processing failure disables AEC for this meeting. A
                    // broken engine must not repeatedly consume capture time.
                    log::warn!("meeting echo reduction failed; keeping original microphone audio");
                    self.processor = None;
                }
            } else {
                self.needs_reset = true;
            }
            if succeeded {
                self.retained.extend(capture);
                // AEC3 buffers 128 samples internally. Remove its leading
                // latency and keep the corresponding original microphone tail
                // until processed output arrives on the following frame.
                let skip = self.warmup.min(cleaned.len());
                self.warmup -= skip;
                output.extend_from_slice(&cleaned[skip..]);
                self.retained.drain(..cleaned.len() - skip);
            } else {
                output.extend(self.retained.drain(..));
                output.extend_from_slice(&capture);
            }
            self.emitted = end;
            if self.processor.is_none() {
                self.bypass_pending(output);
                break;
            }
        }
        if finish {
            // Do not pad an incomplete frame through the filter: keep the
            // actual ending samples, with no invented duration or lost tail.
            self.bypass_pending(output);
        }
    }

    fn bypass_pending(&mut self, output: &mut Vec<f32>) {
        // Flushed originals must never also emerge from the old filter on a
        // later drain (a lost reference can still have buffered audio ahead).
        if !self.retained.is_empty() || !self.mic.is_empty() {
            self.needs_reset = true;
        }
        output.extend(self.retained.drain(..));
        self.emitted += self.mic.len() as u64;
        output.extend(self.mic.drain(..));
    }

    fn discard_reference_before(&mut self, before: u64) {
        let count = before
            .saturating_sub(self.render_start)
            .min(self.render.len() as u64);
        self.render.drain(..count as usize);
        self.render_start += count;
    }
}

#[cfg(test)]
#[path = "echo_tests.rs"]
mod tests;
