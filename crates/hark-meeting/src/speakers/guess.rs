//! Guessing speakers' names from names said in the call (spec §2.6): the
//! on-device rules, and the validation every guess passes, the text
//! provider's included.
//!
//! A guess is only ever accepted when the line it cites literally contains
//! the name, as a whole word. That deterministic check anchors every guess,
//! whichever engine made it; nothing is matched phonetically or fuzzily.
//!
//! English only. Names are content: they are never logged, and no type here
//! derives `Debug`.

use super::SpeakerKey;
use std::collections::HashMap;

/// One transcript line as the guessers see it.
pub struct NameLine<'a> {
    pub key: SpeakerKey,
    pub start_ms: u64,
    pub text: &'a str,
}

/// A proposed name, before validation: `line` indexes the line that says it.
pub struct Candidate {
    pub key: SpeakerKey,
    pub name: String,
    pub line: usize,
}

/// An accepted guess, with the start of the line the name was heard in.
pub struct Guess {
    pub key: SpeakerKey,
    pub name: String,
    pub evidence_ms: u64,
}

const NAME_MAX_CHARS: usize = 40;
const NAME_MAX_WORDS: usize = 3;

/// Words a name may never contain: the default labels' own words, and roles
/// that say who someone is without saying their name.
const KEY_WORDS: &[&str] = &[
    "me",
    "them",
    "speaker",
    "speakers",
    "unknown",
    "host",
    "guest",
    "participant",
    "window",
    "other",
    "user",
    "caller",
    "moderator",
    "everyone",
    "someone",
    "anyone",
    "nobody",
];

/// Capitalized words that are not names: pronouns, greetings, days, months
/// and conversation filler. Lowercase.
const STOPLIST: &[&str] = &[
    "i",
    "me",
    "you",
    "he",
    "she",
    "we",
    "they",
    "it",
    "him",
    "her",
    "us",
    "them",
    "my",
    "your",
    "our",
    "their",
    "his",
    "its",
    "hi",
    "hey",
    "hello",
    "morning",
    "afternoon",
    "evening",
    "thanks",
    "thank",
    "cheers",
    "bye",
    "goodbye",
    "welcome",
    "sorry",
    "please",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "today",
    "tomorrow",
    "yesterday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "ok",
    "okay",
    "yeah",
    "yes",
    "yep",
    "no",
    "nope",
    "sure",
    "right",
    "well",
    "so",
    "and",
    "but",
    "or",
    "everyone",
    "everybody",
    "all",
    "team",
    "guys",
    "folks",
    "there",
    "here",
    "this",
    "that",
    "these",
    "those",
    "what",
    "who",
    "how",
    "why",
    "when",
    "where",
    "just",
    "great",
    "good",
    "cool",
    "nice",
    "perfect",
    "awesome",
    "alright",
    "absolutely",
    "exactly",
    "maybe",
    "um",
    "uh",
    "oh",
    "hmm",
    "actually",
    "also",
    "now",
    "then",
    "let",
    "lets",
    "can",
    "could",
    "would",
    "should",
    "will",
    "the",
    "a",
    "an",
    "is",
    "are",
    "was",
    "do",
    "does",
    "did",
    "have",
    "has",
    "not",
    "wait",
    "speaker",
    "host",
    "unknown",
    "zoom",
    "teams",
    "slack",
    "google",
    "microsoft",
    "hark",
    "i'm",
    "it's",
    "let's",
    "that's",
    "there's",
    "here's",
    "we're",
    "you're",
    "they're",
    "i'll",
    "i've",
    "i'd",
    "don't",
    "can't",
    "won't",
    "isn't",
];

/// Words that may follow a self-introduced name ("this is Sam from Acme").
const FOLLOWS: &[&str] = &["from", "with", "here", "and", "speaking", "calling"];

/// Validate guesses from either engine. A guess is kept only when its key is
/// guessable, its name looks like a name (1-3 words of letters, marks,
/// spaces, hyphens, apostrophes or periods, at most 40 characters, no label
/// or role word, not `your_name`), and its cited line contains the name as a
/// whole word. A key that comes back with two different names is dropped
/// entirely; two keys with one name are both kept (they merge by name).
/// Returns the guesses, ordered by key, and how many were rejected.
pub fn validate(
    candidates: Vec<Candidate>,
    lines: &[NameLine],
    guessable: &[SpeakerKey],
    your_name: &str,
) -> (Vec<Guess>, usize) {
    let mut rejected = 0;
    let mut by_key: Vec<(SpeakerKey, Vec<Guess>)> = Vec::new();
    for c in candidates {
        let Some(name) = clean_name(&c.name, your_name) else {
            rejected += 1;
            continue;
        };
        let cited = lines.get(c.line);
        if !guessable.contains(&c.key) || !cited.is_some_and(|l| says(l.text, &name)) {
            rejected += 1;
            continue;
        }
        let guess = Guess {
            key: c.key,
            name,
            evidence_ms: cited.map_or(0, |l| l.start_ms),
        };
        match by_key.iter_mut().find(|(k, _)| *k == c.key) {
            Some((_, list)) => list.push(guess),
            None => by_key.push((c.key, vec![guess])),
        }
    }
    let mut out = Vec::new();
    for (_, mut list) in by_key {
        let first = list[0].name.to_lowercase();
        if list.iter().any(|g| g.name.to_lowercase() != first) {
            rejected += list.len();
            continue;
        }
        list.sort_by_key(|g| g.evidence_ms);
        out.push(list.swap_remove(0));
    }
    out.sort_by_key(|g| g.key);
    (out, rejected)
}

/// The trimmed, single-spaced name if it may be a name at all.
fn clean_name(name: &str, your_name: &str) -> Option<String> {
    let name = name.replace('’', "'");
    let words: Vec<&str> = name.split_whitespace().collect();
    let name = words.join(" ");
    let ok_chars = name
        .chars()
        .all(|c| c.is_alphabetic() || is_mark(c) || matches!(c, ' ' | '-' | '\'' | '.'));
    if words.is_empty()
        || words.len() > NAME_MAX_WORDS
        || name.chars().count() > NAME_MAX_CHARS
        || !ok_chars
        || !name.chars().any(char::is_alphabetic)
        || words
            .iter()
            .any(|w| KEY_WORDS.contains(&w.trim_matches(['.', '\'', '-']).to_lowercase().as_str()))
        || (!your_name.trim().is_empty() && name.to_lowercase() == your_name.trim().to_lowercase())
    {
        return None;
    }
    Some(name)
}

/// Combining marks (diacritics written as separate code points).
fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Whether `text` contains `name` as a whole word, ignoring case.
fn says(text: &str, name: &str) -> bool {
    let text = text.replace('’', "'").to_lowercase();
    let name = name.to_lowercase();
    let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || is_mark(c));
    text.match_indices(&name).any(|(at, m)| {
        boundary(text[..at].chars().next_back()) && boundary(text[at + m.len()..].chars().next())
    })
}

struct Word {
    text: String,
    lower: String,
    /// Text between this word and the next (or the end of the line).
    after: String,
}

fn words(text: &str) -> Vec<Word> {
    let text = text.replace('’', "'");
    let is_word = |c: char| c.is_alphanumeric() || is_mark(c) || c == '\'' || c == '-';
    let mut out: Vec<Word> = Vec::new();
    let mut current = String::new();
    let mut gap = String::new();
    for c in text.chars() {
        if is_word(c) {
            if current.is_empty() {
                if let Some(last) = out.last_mut() {
                    last.after = std::mem::take(&mut gap);
                }
                gap.clear();
            }
            current.push(c);
        } else {
            if !current.is_empty() {
                let w = std::mem::take(&mut current);
                out.push(Word {
                    lower: w.to_lowercase(),
                    text: w,
                    after: String::new(),
                });
            }
            gap.push(c);
        }
    }
    if !current.is_empty() {
        out.push(Word {
            lower: current.to_lowercase(),
            text: current,
            after: String::new(),
        });
    } else if let Some(last) = out.last_mut() {
        last.after = gap;
    }
    out
}

/// A capitalized word that is not a common capitalized word: `Sam`,
/// `O'Neil`, `Mary-Jo`.
fn name_word(w: &Word) -> bool {
    let mut chars = w.text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest: Vec<char> = chars.collect();
    let shape = first.is_uppercase()
        && rest.iter().any(|c| c.is_lowercase())
        && rest.iter().enumerate().all(|(i, c)| {
            c.is_lowercase()
                || is_mark(*c)
                || (matches!(c, '\'' | '-') && rest.get(i + 1).is_some_and(|n| n.is_alphabetic()))
                || (c.is_uppercase() && i > 0 && matches!(rest[i - 1], '\'' | '-'))
        });
    shape && !STOPLIST.contains(&w.lower.trim_matches(['\'', '-']))
}

/// A one- or two-word name starting at `at`: the name, and the index just
/// past it.
fn name_at(ws: &[Word], at: usize) -> Option<(String, usize)> {
    let first = ws.get(at).filter(|w| name_word(w))?;
    let second = ws
        .get(at + 1)
        .filter(|w| first.after == " " && name_word(w) && !FOLLOWS.contains(&w.lower.as_str()));
    Some(match second {
        Some(w) => (format!("{} {}", first.text, w.text), at + 2),
        None => (first.text.clone(), at + 1),
    })
}

/// A self-introduced name must end the sentence or lead into a FOLLOWS word.
fn ends_cleanly(ws: &[Word], next: usize) -> bool {
    let punct = |s: &str| {
        s.chars()
            .any(|c| matches!(c, '.' | ',' | '!' | '?' | ';' | ':'))
    };
    next >= ws.len() || punct(&ws[next - 1].after) || FOLLOWS.contains(&ws[next].lower.as_str())
}

/// "my name is NAME" anywhere, or "this is / it's / I'm / I am NAME" within
/// the first six words.
fn introduces(ws: &[Word]) -> Option<String> {
    let lw = |i: usize| ws.get(i).map_or("", |w| w.lower.as_str());
    for i in 0..ws.len() {
        let at = match (lw(i), lw(i + 1), lw(i + 2)) {
            ("my", "name", "is") => i + 3,
            ("my", "name's", _) => i + 2,
            _ => continue,
        };
        if let Some((name, _)) = name_at(ws, at) {
            return Some(name);
        }
    }
    for i in 0..ws.len().min(6) {
        let at = match (lw(i), lw(i + 1)) {
            ("this", "is") | ("i", "am") => i + 2,
            ("it's", _) | ("i'm", _) => i + 1,
            _ => continue,
        };
        if let Some((name, next)) = name_at(ws, at) {
            if ends_cleanly(ws, next) {
                return Some(name);
            }
        }
    }
    None
}

/// Direct address: `(name, credits the previous line)`. "Thanks, NAME" at
/// the start or ", NAME." at the end thanks whoever just spoke; "Hi NAME" or
/// "NAME, ..." at the start speaks to whoever answers.
fn addresses(ws: &[Word]) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let lw = |i: usize| ws.get(i).map_or("", |w| w.lower.as_str());
    let thanks_at = match (lw(0), lw(1)) {
        ("thank", "you") => Some(1),
        ("thanks" | "cheers" | "bye", _) => Some(0),
        _ => None,
    };
    if let Some(t) = thanks_at.filter(|t| ws[*t].after.contains(',')) {
        if let Some((name, _)) = name_at(ws, t + 1) {
            out.push((name, true));
        }
    }
    let n = ws.len();
    let terminal = ws
        .last()
        .is_some_and(|w| matches!(w.after.trim(), "." | "!" | "?"));
    if terminal && n >= 2 {
        let two = n >= 3 && ws[n - 2].after == " " && ws[n - 3].after.contains(',');
        let start = if two { n - 2 } else { n - 1 };
        if (two || ws[n - 2].after.contains(',')) && ws[start..].iter().all(name_word) {
            let name = ws[start..]
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            out.push((name, true));
        }
    }
    if matches!(lw(0), "hi" | "hey" | "hello" | "morning") {
        if let Some((name, _)) = name_at(ws, 1) {
            out.push((name, false));
        }
    } else if let Some((name, next)) = name_at(ws, 0) {
        if ws[next - 1].after.contains(',') && next < n {
            out.push((name, false));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    out
}

struct Score {
    name: String,
    total: u32,
    /// Strongest single cue, and its line.
    best: (u32, usize),
}

/// The on-device rules. A key takes its best-scoring name when it scores at
/// least 2 and at least twice the runner-up (a self-introduction scores 3, a
/// direct address 1); a name won by more than one key goes to the highest
/// score, and a tie names neither. Then [`validate`], like every engine.
pub fn guess_local(
    lines: &[NameLine],
    guessable: &[SpeakerKey],
    your_name: &str,
) -> (Vec<Guess>, usize) {
    let mine = your_name.trim().to_lowercase();
    let mut scores: HashMap<SpeakerKey, Vec<Score>> = HashMap::new();
    let mut credit = |key: SpeakerKey, name: String, points: u32, line: usize| {
        if !guessable.contains(&key) || name.to_lowercase() == mine {
            return;
        }
        let list = scores.entry(key).or_default();
        match list.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
            Some(s) => {
                s.total += points;
                if points > s.best.0 {
                    s.best = (points, line);
                }
            }
            None => list.push(Score {
                name,
                total: points,
                best: (points, line),
            }),
        }
    };
    for (i, line) in lines.iter().enumerate() {
        let ws = words(line.text);
        if let Some(name) = introduces(&ws) {
            credit(line.key, name, 3, i);
        }
        for (name, previous) in addresses(&ws) {
            let other = if previous {
                lines[..i].iter().rev().find(|l| l.key != line.key)
            } else {
                lines[i + 1..].iter().find(|l| l.key != line.key)
            };
            if let Some(other) = other {
                credit(other.key, name, 1, i);
            }
        }
    }

    let mut winners: Vec<(SpeakerKey, Score)> = Vec::new();
    for (key, mut list) in scores {
        list.sort_by(|a, b| b.total.cmp(&a.total).then(a.best.1.cmp(&b.best.1)));
        let runner_up = list.get(1).map_or(0, |s| s.total);
        let best = list.swap_remove(0);
        if best.total >= 2 && best.total >= 2 * runner_up {
            winners.push((key, best));
        }
    }
    let mut candidates = Vec::new();
    for (key, s) in &winners {
        let rivals = winners
            .iter()
            .filter(|(k, o)| k != key && o.name.eq_ignore_ascii_case(&s.name));
        if rivals.clone().all(|(_, o)| o.total < s.total) {
            candidates.push(Candidate {
                key: *key,
                name: s.name.clone(),
                line: s.best.1,
            });
        }
    }
    validate(candidates, lines, guessable, your_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: SpeakerKey = SpeakerKey::Me;
    const A: SpeakerKey = SpeakerKey::Diarized(0);
    const B: SpeakerKey = SpeakerKey::Diarized(1);
    const C: SpeakerKey = SpeakerKey::Diarized(2);

    fn lines<'a>(spec: &[(SpeakerKey, &'a str)]) -> Vec<NameLine<'a>> {
        spec.iter()
            .enumerate()
            .map(|(i, (key, text))| NameLine {
                key: *key,
                start_ms: i as u64 * 1_000,
                text,
            })
            .collect()
    }

    fn local(spec: &[(SpeakerKey, &str)], your_name: &str) -> Vec<(SpeakerKey, String, u64)> {
        let ls = lines(spec);
        guess_local(&ls, &[ME, A, B, C], your_name)
            .0
            .into_iter()
            .map(|g| (g.key, g.name, g.evidence_ms))
            .collect()
    }

    fn named(key: SpeakerKey, name: &str, at: u64) -> (SpeakerKey, String, u64) {
        (key, name.to_string(), at)
    }

    #[test]
    fn self_introductions_name_the_line_they_are_said_in() {
        let table: &[(&str, Option<&str>)] = &[
            ("Hi, this is Sam from Acme", Some("Sam")),
            ("my name's O'Neil, by the way", Some("O'Neil")),
            (
                "Well, my name is Mary-Jo Kim and I run ops",
                Some("Mary-Jo Kim"),
            ),
            ("Hey all, I'm Priya.", Some("Priya")),
            ("It's Dana calling about the invoice", Some("Dana")),
            ("I am Lee", Some("Lee")),
            // NAME must end the sentence or lead into from/with/here/...
            ("I'm Sam going to talk next", None),
            // Too late in the line for the short forms.
            ("So yesterday we found out that this is Sam's problem", None),
            // Stoplist words are never names.
            ("This is Monday, everyone", None),
            ("I'm Sorry about that", None),
            ("this is great", None),
        ];
        for (text, want) in table {
            let got = local(&[(A, text)], "");
            let want: Vec<_> = want.iter().map(|n| named(A, n, 0)).collect();
            assert_eq!(got, want, "{text:?}");
        }
    }

    #[test]
    fn one_direct_address_names_nobody_and_two_name_the_speaker() {
        let once = [(A, "Here is the update."), (B, "Thanks, Sam.")];
        assert!(local(&once, "").is_empty());
        let twice = [
            (A, "Here is the update."),
            (B, "Thanks, Sam"),
            (A, "One more thing."),
            (C, "Great work, Sam."),
        ];
        assert_eq!(local(&twice, ""), [named(A, "Sam", 1_000)]);
    }

    #[test]
    fn greetings_and_leading_names_credit_whoever_answers() {
        let spec = [
            (B, "Hi Dana, can you start?"),
            (A, "Sure."),
            (B, "Dana, one more question"),
            (A, "Go ahead."),
        ];
        assert_eq!(local(&spec, ""), [named(A, "Dana", 0)]);
    }

    #[test]
    fn a_name_on_two_keys_goes_to_the_higher_score_and_a_tie_names_neither() {
        let higher = [
            (A, "Hi, this is Sam here"),
            (B, "This is Sam."),
            (C, "Thanks, Sam!"),
        ];
        // A and B both introduce as Sam (3 each); C thanks B (+1): B wins.
        assert_eq!(local(&higher, ""), [named(B, "Sam", 1_000)]);
        let tie = [(A, "Hi, this is Sam here"), (B, "This is Sam.")];
        assert!(local(&tie, "").is_empty());
    }

    #[test]
    fn a_clear_runner_up_blocks_the_best_name() {
        let spec = [
            (A, "I'm Sam."),
            (B, "Thanks, Pat."),
            (A, "Right."),
            (C, "Thanks, Pat."),
        ];
        // A: Sam 3, Pat 2. 3 < 2 x 2, so A is not named.
        assert!(local(&spec, "").is_empty());
    }

    #[test]
    fn your_name_is_never_guessed_for_anyone() {
        let spec = [
            (A, "Hi, this is Chaz from Hark Labs"),
            (B, "my name is Dana"),
        ];
        assert_eq!(local(&spec, " chaz "), [named(B, "Dana", 1_000)]);
    }

    #[test]
    fn only_guessable_keys_are_named() {
        let ls = lines(&[(ME, "Hi, this is Chaz"), (A, "This is Dana.")]);
        let (got, _) = guess_local(&ls, &[A], "");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, A);
    }

    fn candidate(key: SpeakerKey, name: &str, line: usize) -> Candidate {
        Candidate {
            key,
            name: name.to_string(),
            line,
        }
    }

    #[test]
    fn validation_requires_the_name_in_the_cited_line_as_a_whole_word() {
        let ls = lines(&[(A, "hello, this is SAM speaking"), (B, "Samantha here")]);
        let check = |c: Candidate| validate(vec![c], &ls, &[A, B], "").0.len();
        assert_eq!(check(candidate(A, "Sam", 0)), 1, "case-insensitive");
        assert_eq!(check(candidate(B, "Sam", 1)), 0, "not inside Samantha");
        assert_eq!(check(candidate(A, "Sam", 1)), 0, "wrong line");
        assert_eq!(check(candidate(A, "Sam", 9)), 0, "no such line");
        assert_eq!(check(candidate(C, "Sam", 0)), 0, "unknown key");
    }

    #[test]
    fn validation_rejects_names_that_are_not_names() {
        let ls = lines(&[(
            A,
            "Speaker 2 is Sam Lee Park Jones, the Host, and Chaz; Me too 1",
        )]);
        for bad in [
            "Sam Lee Park Jones",
            "Speaker 2",
            "Host",
            "Me",
            "Chaz",
            "Sam1",
            " ",
            "Sam;",
        ] {
            let (got, rejected) = validate(vec![candidate(A, bad, 0)], &ls, &[A], "chaz");
            assert!(got.is_empty() && rejected == 1, "{bad:?}");
        }
        let long = "Bartholomew Maximiliano Featherstonehaugh";
        assert!(long.chars().count() > NAME_MAX_CHARS);
        let ls = lines(&[(A, long)]);
        assert!(validate(vec![candidate(A, long, 0)], &ls, &[A], "")
            .0
            .is_empty());
    }

    #[test]
    fn a_key_with_two_names_is_dropped_and_two_keys_may_share_one() {
        let ls = lines(&[(A, "Sam and Pat"), (B, "Sam here")]);
        let (got, rejected) = validate(
            vec![
                candidate(A, "Sam", 0),
                candidate(A, "Pat", 0),
                candidate(B, "sam", 1),
                candidate(B, "Sam", 1),
            ],
            &ls,
            &[A, B],
            "",
        );
        assert_eq!(rejected, 2);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].key, got[0].name.as_str()), (B, "sam"));
        let (merged, _) = validate(
            vec![candidate(A, "Sam", 0), candidate(B, "Sam", 1)],
            &ls,
            &[A, B],
            "",
        );
        assert_eq!(merged.len(), 2);
    }
}
