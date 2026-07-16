# Phase 2: Dictionary (phonetic post-correction + provider biasing)

**Date:** 2026-07-16. **Status:** CP0-CP5 implemented and committed (2026-07-16, `0d40ee7`..`a58180f`, 170 tests green); CP6 interactive gate pending. **Prereq:** Phase 1 complete (main @ `4f19ba2`, 118 tests green).
**Master plan:** `tasks/plan-repo.md` §Phase 2 (lines 138-141) and crate layout (line 92).

## 1. Goal

The user maintains a list of canonical terms (names, jargon, product words: "Modero", "Vossburg", "hark-stt", "nova-3"). After the STT provider returns a transcript, Hark finds spans that *sound like* a dictionary term but are spelled wrong and replaces them with the canonical spelling, before injection. This is the **primary** mechanism. Provider biasing (Deepgram `keyterm`, OpenAI/Groq `prompt`) is **secondary**: already plumbed, measured weak in the spike (Deepgram keyterm gave no lift on clean audio; Groq prompt failed to enforce spelling), kept because it is nearly free and occasionally helps.

Out of scope for Phase 2: dictionary UI editor (Phase 4), per-term weights, regex/glob terms, multi-language phonetics.

## 2. Design

### 2.1 New crate: `hark-dictionary`

Pure text processing, no I/O, no async. Runs on the pipeline worker thread inside the release-to-inject latency budget; target well under 10 ms for a 100-word utterance against a 200-term dictionary (expected: microseconds to low single-digit ms; the budget is dominated by the HTTPS round trip).

Dependencies:

| Crate | Version | Role |
|---|---|---|
| `rphonetic` | `3.0.6` | Double Metaphone primary + alternate codes (Apache-2.0, active as of 2026-01) |
| `strsim` | `0.11.1` | Jaro-Winkler confirmation score (MIT, stable API, repo active) |

Rejected: `rapidfuzz` Rust port (stale since 2023), `triple_accel` (SIMD batch overkill at this scale), rphonetic's Beider-Morse (needs external data files).

Public API sketch:

```rust
pub struct Corrector { /* precomputed term entries */ }

impl Corrector {
    /// Precomputes phonetic codes per term word at construction; call once at startup.
    pub fn new(terms: &[String]) -> Corrector;
    /// Returns the corrected text and the number of replacements made.
    pub fn correct(&self, text: &str) -> (String, usize);
}
```

`correct` never fails: any internal anomaly (unencodable token, empty input) degrades to returning the input span unchanged. A "no match" outcome means "left as transcribed", not "verified correct".

### 2.2 Matching algorithm

1. **Precompute (at `new`)**: for each term, split on whitespace and hyphens into words. Per word, store: lowercase form, and (if purely alphabetic and length >= 4) Double Metaphone primary + alternate codes. Words containing digits, or of length <= 3, are flagged **exact-only** (Double Metaphone codes degenerate at short lengths and cannot encode digits).
2. **Tokenize transcript**: split into word tokens with byte spans, capturing leading/trailing punctuation separately so it survives replacement. Lowercase copies for comparison; originals kept for output.
3. **Match**: for each term (sorted longest word-count first, then longest char length, so multi-word terms win overlaps), slide a window of the term's word count across the tokens. A window matches if every word pair passes:
   - exact-only words: case-insensitive equality;
   - phonetic words: Double Metaphone code equality (primary or alternate on either side) **and** `jaro_winkler(lowercase) >= 0.85` as the false-positive guard.
4. **Replace**: matched windows are replaced with the canonical term verbatim (canonical spelling includes its own casing; that is the point of the dictionary). Adjacent punctuation from the original tokens is preserved. Consumed token indices are marked so overlapping shorter terms skip them. A window that already equals the canonical term exactly is a no-op (not counted as a replacement).

Threshold `0.85` is a crate-level `const` for Phase 2, tuned at the CP6 gate; promote to config only if real usage demands it.

### 2.3 Config schema (hark-config)

Rename `bias_terms` to `terms` with a serde alias so existing config files keep working:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Dictionary {
    /// Canonical terms: post-correction targets and the source for provider biasing.
    #[serde(alias = "bias_terms")]
    pub terms: Vec<String>,
}
```

```toml
[dictionary]
terms = ["Modero", "Vossburg", "hark-stt", "nova-3"]
```

One list drives both mechanisms: `hark_pipeline::provider_config()` (crates/hark-pipeline/src/lib.rs:78) keeps cloning it into `hark_stt::ProviderConfig.bias_terms`, and `Worker` builds a `Corrector` from the same list. Follow the existing settings pattern exactly: `#[serde(default)]` struct, explicit `Default`, unit tests for empty-TOML defaults and explicit-value override (see hark-config/src/lib.rs:271-392).

### 2.4 Pipeline integration

In `crates/hark-pipeline/src/worker.rs::dictate()`: the correction pass slots between the empty-transcript check (line 111) and `hark_inject::inject(&transcript.text, ...)` (line 116). `Worker` gains a `corrector: Corrector` field built once in construction, mirroring how `provider` is held.

Logging discipline (unchanged from Phase 1): counts and millis only, **never transcript text or term content**. Log line shape: `dictionary: N replacements in X ms`.

### 2.5 Biasing hardening (small, riding along)

`openai_compatible.rs::prompt_from_bias_terms()` currently joins all terms with no cap; Whisper-family models truncate prompts at 224 tokens. Enforce a budget: include terms in order until an approximate token count (chars / 4 heuristic) reaches ~200, drop the rest, and log `prompt bias: included M of N terms` once at provider construction. Deepgram `keyterm` path stays as is (one query param per term).

## 3. Checkpoints

One commit per checkpoint (Patch bumps; CP1 and CP5 are Minor candidates, decide at commit time per `.claude/rules/commit-changelog.md`). `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and full tests green at every CP.

### CP0: crate scaffold + dependency proofs
- `crates/hark-dictionary` added to workspace; deps pinned (`rphonetic = "3.0.6"`, `strsim = "0.11.1"`).
- `Corrector::new` + identity `correct` (returns input, 0 replacements).
- **Proof tests for third-party behavior we rely on**: rphonetic does not panic on empty string, non-ASCII ("müller"), digits, hyphens; strsim `jaro_winkler` returns 1.0 for equal 1-char strings (historical bug regression guard, transcripts are full of "a"/"I").

### CP1: config schema
- `Dictionary.terms` with `bias_terms` serde alias; pipeline `provider_config()` reads `terms`.
- Tests: old-key TOML still parses; new-key TOML parses; defaults empty.

### CP2: tokenizer
- Word tokens with byte spans; punctuation captured as prefix/suffix, not part of the comparable token; original casing preserved in spans.
- Tests: punctuation adjacency ("modero," and "(modero)"), unicode words, empty string, repeated whitespace, hyphenated input tokens.

### CP3: single-word matching
- Precomputed term entries; exact-only paths (digits, len <= 3); phonetic + Jaro-Winkler confirm path.
- Tests with realistic ASR misspellings: "madero" -> "Modero", "vosburg" -> "Vossburg"; negative cases that phonetically collide but fail the JW guard; case-insensitive exact hits get canonical casing.

### CP4: multi-word terms + replacement
- N-gram windows sized per term; longest-first overlap resolution; hyphen-split terms ("hark-stt" matches transcript "hark stt" and "hark-stt"); punctuation reattachment.
- Tests: "hark stt" -> "hark-stt", overlapping terms pick the longer, adjacent punctuation survives, no-op when already canonical, replacement count correct.

### CP5: pipeline integration + prompt cap
- `Worker` holds `Corrector`; `dictate()` corrects between empty-check and inject; count/millis logging.
- `prompt_from_bias_terms` token budget + construction-time log.
- Tests: worker-level test with a fake provider returning a misspelled transcript asserts the injected text is corrected; prompt cap unit tests (under, at, over budget).

### CP6: interactive gate (real hardware, Windows)
- User loads a real dictionary (their actual names/jargon), dictates sentences containing the terms plus decoy sentences without them, and validates: corrections applied, no false positives on ordinary speech, no perceptible latency change.
- Tune the JW threshold const here if needed; record observed behavior in Lessons Learned.
- Same deferral rule as Phase 1: macOS validation waits for Mac hardware; nothing in this phase is platform-specific, so no seam work needed.

## 4. Risks / open questions

- **False positives are the product risk.** A dictionary term that sounds like a common English word ("Hark" vs "hark") will rewrite ordinary speech. Mitigation: JW guard, exact-only for short words, and CP6 decoy sentences. If real usage still misfires, next lever is requiring the term to be multi-word or adding a per-term opt-out, not lowering the threshold globally.
- **rphonetic non-ASCII behavior is inferred, not documented.** CP0 proof tests exist precisely to catch this before anything is built on top.
- **Threshold 0.85 is a research-informed guess.** CP6 is the empirical gate.

## 5. Lessons Learned / Gotchas

Pre-implementation, seeded from research (2026-07-16); confirm or amend during implementation, then route durable ones to LL-G via `/add-lesson`:

- Double Metaphone codes (default max length 4) degenerate on words of <= 3 letters: collision rate spikes. Gate short words to exact matching; do not phonetically match them.
- Digits and hyphens are not phonetically encodable. Split terms on hyphens; any word containing a digit takes the exact-match path ("nova-3", "hark-stt").
- Neither rphonetic nor strsim normalizes case; lowercase both sides before encoding/scoring, and apply the canonical term's own casing on replacement.
- strsim had historical bugs on length-1 strings (jaro_winkler 0 instead of 1, and a panic, fixed in 0.9.3/0.5.1); keep the regression tests from CP0 even though the bugs are long fixed.
- "No match" means "left as transcribed", never "verified correct" (BP validation principle): do not log or report unmatched transcripts as clean.
- Hot-path logging stays counts/millis only; transcript text and dictionary terms are user content and must never appear in logs (same discipline as the Phase 1 api_key redaction).
- Provider biasing spike verdict (context for why post-correction is primary): Deepgram `keyterm` showed no lift on clean audio; Groq `prompt` failed to enforce spelling. Do not spend further effort strengthening biasing without new evidence.

Filled in during implementation (2026-07-16, CP0-CP5; threshold findings await CP6):

- **rphonetic 3.0.6 behaved exactly as researched.** `encode`/`encode_alternate` (the former via the `Encoder` trait, which must be imported) return plain `String`s, codes stay <= 4 chars at the default max length, and empty / non-ASCII / digit / hyphen inputs all encode without panicking. Proof tests pin all of this.
- **The phonetic-collision risk is concrete, not hypothetical: "matter" shares "modero"'s Double Metaphone code (both MTR).** Without the Jaro-Winkler confirm guard, the dictionary would rewrite the common English word "matter" to "Modero". JW("modero","matter") = 0.70, comfortably under the 0.85 threshold. Kept as a permanent regression test; this is the single strongest argument that phonetic-code equality alone must never trigger a replacement.
- **Accent bridging works for free:** "müller" and "muller" produce equal codes, so a non-ASCII canonical term corrects its ASCII misrecognition through the normal phonetic path.
- **Span-based tokenization made "punctuation reattachment" a non-step.** Tokens carry byte spans of their punctuation-free cores; replacement splices canonical text over core spans, so surrounding punctuation survives without ever being tracked. Interior punctuation inside a matched multi-word window is absorbed with the misrecognition (deliberate: both words matched, so the STT-inserted comma belongs to the error).
- **Splitting transcript cores on interior hyphens in the tokenizer** is what lets one window size serve both transcript forms of a hyphen-split term ("hark stt" and "hark-stt" both present as two tokens). The already-canonical hyphenated form falls out as an uncounted no-op because the window's combined span text equals the canonical term.
- **Reusing `tokenize()` on the term text itself** (in `build_entries`) guarantees term-side and transcript-side segmentation can never drift; terms that tokenize to nothing are dropped.
- **clippy `op_ref` gotcha:** `&text[range] != entry.canonical` (comparing `&str` to `String`) trips "needlessly taken reference of left operand" under `-D warnings`; write `text[range] != entry.canonical` (str != String comparison exists).
- **`prompt_from_bias_terms` signature change** (returns `(Option<String>, usize)` for the construction-time count log) required touching only the two adapter_pure tests; nothing else consumed it.
- Route the "matter"/"modero" collision lesson (phonetic match needs an edit-distance confirm guard) to LL-G via `/add-lesson` after CP6 empirically validates the 0.85 threshold.
