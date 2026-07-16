---
name: hark-phonetic-correction-crates
description: rphonetic + strsim crate choice, versions, and matching algorithm shape for the dictionary phonetic post-correction pass (hot path, no tokio)
metadata:
  type: project
---

Researched 2026-07-16 for the phonetic post-correction pass design (runs after cloud STT,
before injection, target <10ms for <100 words / <200 dict terms, single-threaded, no tokio).

**Crate recommendation:**
- `rphonetic = "3.0.6"` (Apache-2.0, last published 2026-01-18, ~92k recent downloads).
  Use `DoubleMetaphone` (`encode`/`encode_alternate`/`double_metaphone` -> `DoubleMetaphoneResult`
  with primary+alternate codes). Also ships Metaphone, Soundex, RefinedSoundex, Caverphone1/2,
  Cologne, Nysiis, Phonex, Daitch-Mokotoff, Match Rating Approach, Beider-Morse. Avoid Beider-Morse
  here: it's heavyweight and needs external config/data files, overkill for a <200-term dictionary.
- `strsim = "0.11.1"` (MIT, published 2024-04-02 but the strsim-rs GitHub repo is still active,
  last commit 2025-11-27; 0.11.1 API is stable so no new release was needed). Use `jaro_winkler`
  and/or `normalized_levenshtein` as the edit-distance confirmation guard after a phonetic-code
  match, to suppress false positives.
- Rejected: the `rapidfuzz` crate (Rust port) is stale at 0.5.0 since 2023-12-01 — do not use.
  `triple_accel` was not deeply evaluated (SIMD-oriented, aimed at large-scale batch distance,
  unnecessary complexity for <200 terms / <100-word utterances).

**Matching algorithm shape (from ASR post-correction prior art, e.g. EnviousWispr's fuzzy pass,
NVIDIA SpellMapper n-gram retrieval):**
1. Tokenize transcript preserving original casing/punctuation spans (need this to reinsert
   punctuation and match case of the replacement).
2. For each dictionary term, note its word count (1..N). Build an n-gram sliding window over
   transcript tokens matching that word count (so multi-word terms like "hark stt" or names with
   two words are checked as a phrase, not token-by-token).
3. For each window, phonetic-encode the window's words (Double Metaphone primary, and check
   alternate too) and compare against the dictionary term's precomputed phonetic code(s).
4. On phonetic-code match, apply an edit-distance confirmation guard (e.g.
   `jaro_winkler(window_lowercased, term_lowercased) >= 0.85`, tune per length) before accepting
   the replacement — phonetic-only matching has too high a false-positive rate, especially on
   short words.
5. Replace matched span with the canonical dictionary spelling, preserving the original token's
   leading/trailing punctuation and matching capitalization style (all-caps, Title Case, or
   lowercase) of the matched span.

**Pitfalls for Lessons Learned:**
- Double Metaphone codes are default max-length 4 (`DoubleMetaphone::new(Option<usize>)` to
  change); very short words (<=3 letters) produce short/degenerate codes with high collision
  rates — apply a minimum-length gate (e.g. skip phonetic matching for dictionary terms or
  transcript words under 4 characters, or require exact/near-exact string match instead) rather
  than trusting the phonetic code alone. No authoritative source quantified the collision rate;
  this is a design inference from the algorithm's fixed short code length, not the community's
  documented literature — validate empirically against Hark's actual dictionary terms during
  implementation.
- rphonetic docs do not explicitly document behavior for empty strings or non-ASCII/Unicode
  input; the Cologne example (`cologne.encode("müller")`) shows UTF-8 input is accepted without
  erroring, suggesting the crate is at minimum non-panicking on Unicode, but algorithms are
  "designed for the Latin alphabet" and English-centric — verify no panic on empty string and on
  digits/hyphens (e.g. "nova-3", "hark-stt") during the spike; these are unencoded by phonetic
  algorithms and likely need to bypass phonetic matching and go straight to case-insensitive
  exact/edit-distance matching on the alphabetic segments.
- strsim's jaro/jaro_winkler had a historical bug (fixed in 0.9.3) returning 0 instead of 1 for
  equal single-char strings, and a panic on length-1 strings (fixed 0.5.1) — both are long fixed
  in 0.11.1, but confirms these functions are edge-case-sensitive; still worth a unit test with
  1-2 char tokens given transcripts contain short words ("a", "I").
- Case handling: neither crate is case-normalizing internally; must lowercase before phonetic
  encoding and edit-distance comparison, then reapply original casing pattern on replacement.
