//! Pure measurement helpers for the spike harness: latency percentiles and
//! transcript-vs-reference edit distance. Model- and network-free so all of it
//! is unit-testable on any box.

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

/// Whether `term` appears in `text` after normalization (multi-word safe).
pub fn contains_term(text: &str, term: &str) -> bool {
    let hay = format!(" {} ", normalize_text(text));
    let needle = format!(" {} ", normalize_text(term));
    hay.contains(&needle)
}

/// Latency samples for one measurement arm (e.g. warm-client runs of one provider).
#[derive(Debug, Clone, Default)]
pub struct LatencyTally {
    samples_ms: Vec<u128>,
}

impl LatencyTally {
    pub fn record(&mut self, ms: u128) {
        self.samples_ms.push(ms);
    }

    pub fn n(&self) -> usize {
        self.samples_ms.len()
    }

    /// Nearest-rank percentile; `pct` in [0, 1]. None when no samples recorded.
    pub fn percentile(&self, pct: f64) -> Option<u128> {
        if self.samples_ms.is_empty() {
            return None;
        }
        let mut sorted = self.samples_ms.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * pct).round() as usize;
        Some(sorted[idx])
    }

    pub fn p50(&self) -> Option<u128> {
        self.percentile(0.50)
    }

    pub fn p95(&self) -> Option<u128> {
        self.percentile(0.95)
    }

    pub fn min(&self) -> Option<u128> {
        self.samples_ms.iter().min().copied()
    }

    pub fn max(&self) -> Option<u128> {
        self.samples_ms.iter().max().copied()
    }
}
