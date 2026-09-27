//! Orders transcribed chunks from both channels on the shared session timeline.
//!
//! The two channels transcribe independently, so results arrive out of order:
//! a chunk of one channel often comes back before an earlier chunk of the
//! other. Each segment is placed by its chunk's start offset as it arrives, so
//! the transcript is always in timeline order and never needs a re-sort.
//! Offsets on both channels count from the same session start, so device
//! clock drift (~31 ppm measured in CP0) only matters for echo cancellation,
//! never for ordering.

use crate::{Channel, SAMPLE_RATE};

/// One transcribed chunk.
pub struct Segment {
    pub channel: Channel,
    /// Session-timeline samples the text came from, `[start, end)`.
    pub start: u64,
    pub end: u64,
    pub text: String,
}

impl Segment {
    pub fn start_ms(&self) -> u64 {
        samples_to_ms(self.start)
    }

    pub fn end_ms(&self) -> u64 {
        samples_to_ms(self.end)
    }
}

fn samples_to_ms(samples: u64) -> u64 {
    samples * 1000 / SAMPLE_RATE as u64
}

// No derived Debug: `text` is what was said in the meeting and must never
// reach a log line. Lengths and offsets only.
impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segment")
            .field("channel", &self.channel)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("text_len", &self.text.len())
            .finish()
    }
}

/// The live transcript: segments from both channels in timeline order.
#[derive(Default)]
pub struct Transcript {
    segments: Vec<Segment>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Place a segment by its start offset and return its index. Blank text
    /// (a chunk that passed the loudness gate but held no words, like a
    /// cough) is dropped and returns `None`.
    ///
    /// Two segments starting at the same sample are ordered `Me` before
    /// `Them`, whichever arrived first, so the order never depends on which
    /// request happened to return sooner.
    pub fn insert(&mut self, segment: Segment) -> Option<usize> {
        if segment.text.trim().is_empty() {
            return None;
        }
        let key = (segment.start, segment.channel);
        let at = self
            .segments
            .partition_point(|s| (s.start, s.channel) <= key);
        self.segments.insert(at, segment);
        Some(at)
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Channel::{Me, Them};

    fn seg(channel: Channel, start: u64, text: &str) -> Segment {
        Segment {
            channel,
            start,
            end: start + 480_000,
            text: text.to_string(),
        }
    }

    fn order(t: &Transcript) -> Vec<(Channel, u64, &str)> {
        t.segments()
            .iter()
            .map(|s| (s.channel, s.start, s.text.as_str()))
            .collect()
    }

    #[test]
    fn out_of_order_arrivals_land_in_timeline_order() {
        let mut t = Transcript::new();
        // Them's second chunk returns first; Me's first chunk returns last.
        assert_eq!(t.insert(seg(Them, 800_000, "c")), Some(0));
        assert_eq!(t.insert(seg(Them, 320_000, "b")), Some(0));
        assert_eq!(t.insert(seg(Me, 1_100_000, "d")), Some(2));
        assert_eq!(t.insert(seg(Me, 0, "a")), Some(0));
        assert_eq!(
            order(&t),
            vec![
                (Me, 0, "a"),
                (Them, 320_000, "b"),
                (Them, 800_000, "c"),
                (Me, 1_100_000, "d"),
            ]
        );
    }

    #[test]
    fn a_tie_on_start_puts_me_first_whatever_the_arrival_order() {
        for them_first in [true, false] {
            let mut t = Transcript::new();
            let (me, them) = (seg(Me, 477_600, "me"), seg(Them, 477_600, "them"));
            if them_first {
                t.insert(them);
                t.insert(me);
            } else {
                t.insert(me);
                t.insert(them);
            }
            assert_eq!(
                order(&t),
                vec![(Me, 477_600, "me"), (Them, 477_600, "them")],
                "them_first = {them_first}"
            );
        }
    }

    #[test]
    fn blank_text_is_not_a_line() {
        let mut t = Transcript::new();
        assert_eq!(t.insert(seg(Me, 0, "")), None);
        assert_eq!(t.insert(seg(Them, 0, "  \n\t")), None);
        assert!(t.is_empty());
        assert_eq!(t.insert(seg(Them, 0, " ok ")), Some(0));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn offsets_convert_to_milliseconds() {
        let s = seg(Them, 477_600, "x");
        assert_eq!((s.start_ms(), s.end_ms()), (29_850, 59_850));
    }

    #[test]
    fn debug_never_prints_the_text() {
        let printed = format!("{:?}", seg(Me, 0, "confidential roadmap"));
        assert!(!printed.contains("confidential"), "{printed}");
        assert!(printed.contains("text_len: 20"), "{printed}");
    }
}
