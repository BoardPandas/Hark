//! Pure text-comparison helpers used by the spike harness to quantify sherpa-onnx
//! issue #3267 (empty/hallucinated `modified_beam_search` output). Kept model- and
//! hardware-free so the classification logic is unit-testable on any box.

/// Lowercase, drop punctuation, collapse runs of whitespace. Makes transcript
/// comparison robust to casing/spacing noise that isn't a real divergence.
pub fn normalize_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true; // trims leading space
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Char-level Levenshtein edit distance.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Normalized edit-distance ratio in [0, 1]: distance / longer length (post-normalize).
pub fn divergence_ratio(output: &str, expected: &str) -> f64 {
    let o = normalize_text(output);
    let e = normalize_text(expected);
    let max_len = o.chars().count().max(e.chars().count());
    if max_len == 0 {
        return 0.0;
    }
    levenshtein(&o, &e) as f64 / max_len as f64
}

/// Classification of one decode output relative to a known-good transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing (or whitespace/punctuation only) came back.
    Empty,
    /// Output diverged from the reference beyond `threshold`.
    Hallucination,
    /// Output matched the reference within tolerance.
    Clean,
}

/// Classify one output. `threshold` is the max tolerated divergence ratio (e.g. 0.5).
pub fn classify(output: &str, expected: &str, threshold: f64) -> Outcome {
    if normalize_text(output).is_empty() {
        return Outcome::Empty;
    }
    if divergence_ratio(output, expected) > threshold {
        Outcome::Hallucination
    } else {
        Outcome::Clean
    }
}

/// Aggregate outcome + latency stats for one A/B arm.
#[derive(Debug, Clone, Default)]
pub struct AbTally {
    pub n: usize,
    pub empty: usize,
    pub halluc: usize,
    pub clean: usize,
    pub panics: usize,
    pub decode_ms: Vec<u128>,
}

impl AbTally {
    pub fn record_outcome(&mut self, outcome: Outcome) {
        self.n += 1;
        match outcome {
            Outcome::Empty => self.empty += 1,
            Outcome::Hallucination => self.halluc += 1,
            Outcome::Clean => self.clean += 1,
        }
    }

    pub fn record_panic(&mut self) {
        self.n += 1;
        self.panics += 1;
    }

    pub fn empty_rate(&self) -> f64 {
        self.rate(self.empty)
    }

    pub fn halluc_rate(&self) -> f64 {
        self.rate(self.halluc)
    }

    /// Empty + hallucinated + panicked, over N: the #3267 failure rate.
    pub fn failure_rate(&self) -> f64 {
        self.rate(self.empty + self.halluc + self.panics)
    }

    fn rate(&self, count: usize) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            count as f64 / self.n as f64
        }
    }

    pub fn latency_percentile(&self, pct: f64) -> Option<u128> {
        if self.decode_ms.is_empty() {
            return None;
        }
        let mut sorted = self.decode_ms.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * pct).round() as usize;
        Some(sorted[idx])
    }
}
