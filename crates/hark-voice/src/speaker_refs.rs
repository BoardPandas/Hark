//! Speaker references inside meeting notes (spec
//! `intent/meeting-speakers-are-hard-to-tell-apart/spec.md` §2.7).
//!
//! The summary prompt labels every line with its speaker's default label in
//! braces (`{Speaker 2}`) and asks the model to copy those exactly. Parsing
//! turns each known `{Label}` into a `{{key}}` token, so the stored notes
//! name a speaker key, never a name: a rename is a deterministic, instant
//! re-render with no LLM call. Notes written before tokens existed only ever
//! saw default labels, and render by whole-phrase replacement of those.
//!
//! Pure string work; names and notes text are content and never logged.

/// How a stored notes record refers to speakers. Unknown values (a newer
/// build's) read as `Frozen`: shown exactly as stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeakerRefs {
    /// `{{key}}` tokens, rendered through the meeting's names.
    Tokens,
    /// Notes from before tokens: default labels in plain text.
    #[default]
    Labels,
    /// Names already written in (baked before a re-run reset the speakers).
    #[serde(other)]
    Frozen,
}

/// Shown for a token whose key the meeting no longer has.
pub const UNKNOWN_SPEAKER: &str = "Unknown speaker";

/// Longest braced text treated as a speaker label.
const LABEL_MAX_CHARS: usize = 80;

/// `{Label}` (any number of braces around it) becomes `{{key}}` when the
/// label is one this call sent; any other braced text loses its braces, so
/// raw braces never reach the user. `labels` is `(default label, key)`.
pub(crate) fn tokenize(text: &str, labels: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let braced = rest[open..].trim_start_matches('{');
        let close = braced.find(['{', '}', '\n']);
        match close {
            Some(end) if braced[end..].starts_with('}') => {
                let inner = braced[..end].trim();
                if inner.chars().count() > LABEL_MAX_CHARS {
                    out.push_str(&braced[..end]);
                } else if !inner.is_empty() {
                    let found = labels.iter().find(|(label, _)| label == inner).or_else(|| {
                        labels
                            .iter()
                            .find(|(label, _)| label.eq_ignore_ascii_case(inner))
                    });
                    match found {
                        Some((_, key)) => {
                            out.push_str("{{");
                            out.push_str(key);
                            out.push_str("}}");
                        }
                        None => out.push_str(inner),
                    }
                }
                rest = braced[end..].trim_start_matches('}');
            }
            // A brace with no partner: drop it, keep the text.
            _ => rest = braced,
        }
    }
    out.push_str(rest);
    out
}

/// One piece of a notes field as shown: plain text, or a speaker's display
/// label with that speaker's index into the `names` table it was rendered
/// with (the notes card colors those). No `Debug`: carries notes text.
pub struct Span {
    pub text: String,
    pub speaker: Option<usize>,
}

/// One notes field split into plain text and speaker names, with exactly
/// the text [`crate::MeetingNotes::render`] shows. `Frozen` notes are one
/// plain span: their names are already text.
pub fn speaker_spans(
    text: &str,
    names: &[(String, String, String)],
    refs: SpeakerRefs,
) -> Vec<Span> {
    match refs {
        SpeakerRefs::Frozen => vec![Span {
            text: text.to_string(),
            speaker: None,
        }],
        SpeakerRefs::Tokens => spans(text, names, true),
        SpeakerRefs::Labels => spans(text, names, false),
    }
}

pub(crate) fn render_text(text: &str, names: &[(String, String, String)], tokens: bool) -> String {
    spans(text, names, tokens)
        .into_iter()
        .map(|s| s.text)
        .collect()
}

/// Split one notes field at its speaker references. `names` is `(key,
/// default label, display label)` for every key the meeting has. One pass
/// over the text, so a display label that happens to equal another key's
/// default label is never replaced twice. Default labels match whole
/// phrases only, case-sensitively: "Speaker 1" never matches inside
/// "Speaker 12".
fn spans(text: &str, names: &[(String, String, String)], tokens: bool) -> Vec<Span> {
    let mut defaults: Vec<(usize, &str)> = names
        .iter()
        .enumerate()
        .filter(|(_, (_, default, _))| !default.is_empty())
        .map(|(i, (_, default, _))| (i, default.as_str()))
        .collect();
    defaults.sort_by_key(|(_, default)| std::cmp::Reverse(default.len()));

    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::with_capacity(text.len());
    let speaker = |out: &mut Vec<Span>, plain: &mut String, label: &str, index| {
        if !plain.is_empty() {
            out.push(Span {
                text: std::mem::take(plain),
                speaker: None,
            });
        }
        out.push(Span {
            text: label.to_string(),
            speaker: index,
        });
    };
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if tokens && rest.starts_with("{{") {
            if let Some(end) = rest[2..].find("}}") {
                let key = &rest[2..2 + end];
                if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b':') {
                    match names.iter().position(|(k, _, _)| k == key) {
                        Some(n) => speaker(&mut out, &mut plain, &names[n].2, Some(n)),
                        None => speaker(&mut out, &mut plain, UNKNOWN_SPEAKER, None),
                    }
                    i += end + 4;
                    prev = Some('}');
                    continue;
                }
            }
        }
        if !prev.is_some_and(char::is_alphanumeric) {
            let hit = defaults.iter().find(|(_, default)| {
                rest.starts_with(default)
                    && !rest[default.len()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
            });
            if let Some((n, default)) = hit {
                speaker(&mut out, &mut plain, &names[*n].2, Some(*n));
                i += default.len();
                prev = default.chars().last();
                continue;
            }
        }
        let ch = rest
            .chars()
            .next()
            .expect("i is a char boundary inside text");
        plain.push(ch);
        prev = Some(ch);
        i += ch.len_utf8();
    }
    if !plain.is_empty() {
        out.push(Span {
            text: plain,
            speaker: None,
        });
    }
    out
}

/// The words an action item is matched on: lowercased, with tokens,
/// punctuation and the given speaker phrases removed. Phrases are removed as
/// whole words, so stripping "Me" leaves "meeting" alone.
pub(crate) fn match_words(text: &str, strip: &[&str]) -> std::collections::BTreeSet<String> {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        plain.push_str(&rest[..open]);
        rest = match rest[open + 2..].find("}}") {
            Some(end) => &rest[open + 2 + end + 2..],
            None => &rest[open + 2..],
        };
    }
    plain.push_str(rest);
    let split = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let words = split(&plain);
    let mut phrases: Vec<Vec<String>> = strip
        .iter()
        .map(|p| split(p))
        .filter(|p| !p.is_empty())
        .collect();
    phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));
    let mut out = std::collections::BTreeSet::new();
    let mut i = 0;
    'words: while i < words.len() {
        for phrase in &phrases {
            if words[i..].starts_with(phrase) {
                i += phrase.len();
                continue 'words;
            }
        }
        out.insert(words[i].clone());
        i += 1;
    }
    out
}

/// Token-set Jaccard similarity; two empty sets are not a match.
pub(crate) fn jaccard(
    a: &std::collections::BTreeSet<String>,
    b: &std::collections::BTreeSet<String>,
) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Vec<(String, String)> {
        [
            ("Me", "me"),
            ("Speaker 2", "them:1"),
            ("Speaker 12", "them:11"),
        ]
        .map(|(l, k)| (l.to_string(), k.to_string()))
        .to_vec()
    }

    fn names() -> Vec<(String, String, String)> {
        [
            ("me", "Me", "Chaz"),
            ("them:0", "Speaker 1", "Dana"),
            ("them:1", "Speaker 2", "Sam"),
            ("them:11", "Speaker 12", "Lee"),
        ]
        .map(|(k, d, s)| (k.to_string(), d.to_string(), s.to_string()))
        .to_vec()
    }

    #[test]
    fn known_braced_labels_become_tokens_and_unknown_braces_are_stripped() {
        assert_eq!(
            tokenize("{Speaker 2} will send the deck to {Me}.", &labels()),
            "{{them:1}} will send the deck to {{me}}."
        );
        assert_eq!(
            tokenize("{{speaker 2}} agreed", &labels()),
            "{{them:1}} agreed"
        );
        assert_eq!(
            tokenize("Ask {Finance} about {Speaker 9}", &labels()),
            "Ask Finance about Speaker 9"
        );
        assert_eq!(
            tokenize("a {stray and} brace {", &labels()),
            "a stray and brace "
        );
        assert_eq!(tokenize("no braces", &labels()), "no braces");
        assert_eq!(tokenize("{}{", &labels()), "");
    }

    #[test]
    fn tokens_render_as_the_roster_label_and_unknown_keys_say_so() {
        assert_eq!(
            render_text(
                "{{them:1}} owes {{me}} a reply; {{added:7}} too",
                &names(),
                true
            ),
            "Sam owes Chaz a reply; Unknown speaker too"
        );
    }

    #[test]
    fn default_labels_replace_whole_phrases_only() {
        assert_eq!(
            render_text(
                "Speaker 1 and Speaker 12 met Speaker 1x and Me.",
                &names(),
                false
            ),
            "Dana and Lee met Speaker 1x and Chaz."
        );
        // Case-sensitive, and never inside a word.
        assert_eq!(
            render_text("Meeting with me", &names(), false),
            "Meeting with me"
        );
        // A token-less pass leaves token-looking text alone.
        assert_eq!(render_text("{{me}}", &names(), false), "{{me}}");
    }

    #[test]
    fn a_display_name_equal_to_another_default_label_is_not_replaced_twice() {
        let names = [
            ("them:0", "Speaker 1", "Speaker 2"),
            ("them:1", "Speaker 2", "Sam"),
        ]
        .map(|(k, d, s)| (k.to_string(), d.to_string(), s.to_string()))
        .to_vec();
        assert_eq!(
            render_text("{{them:0}}, Speaker 1, Speaker 2", &names, true),
            "Speaker 2, Speaker 2, Sam"
        );
    }

    #[test]
    fn spans_mark_each_speaker_and_join_to_the_rendered_text() {
        let text = "{{them:1}} owes {{me}}; Speaker 12 agreed. {{added:3}} left.";
        let spans = speaker_spans(text, &names(), SpeakerRefs::Tokens);
        let pieces: Vec<(&str, Option<usize>)> =
            spans.iter().map(|s| (s.text.as_str(), s.speaker)).collect();
        assert_eq!(
            pieces,
            [
                ("Sam", Some(2)),
                (" owes ", None),
                ("Chaz", Some(0)),
                ("; ", None),
                ("Lee", Some(3)),
                (" agreed. ", None),
                (UNKNOWN_SPEAKER, None),
                (" left.", None),
            ]
        );
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, render_text(text, &names(), true));
        let frozen = speaker_spans("Sam said so", &names(), SpeakerRefs::Frozen);
        assert_eq!(frozen.len(), 1);
        assert_eq!(frozen[0].speaker, None);
    }

    #[test]
    fn unknown_and_missing_refs_read_as_frozen_and_labels() {
        #[derive(serde::Deserialize)]
        struct Holder {
            #[serde(default)]
            refs: SpeakerRefs,
        }
        let read = |json: &str| serde_json::from_str::<Holder>(json).unwrap().refs;
        assert_eq!(read(r#"{"refs":"tokens"}"#), SpeakerRefs::Tokens);
        assert_eq!(read(r#"{"refs":"frozen"}"#), SpeakerRefs::Frozen);
        assert_eq!(read(r#"{"refs":"emoji-v9"}"#), SpeakerRefs::Frozen);
        assert_eq!(read("{}"), SpeakerRefs::Labels);
    }

    #[test]
    fn matching_ignores_tokens_names_case_and_punctuation() {
        let a = match_words("{{them:1}} will SEND the deck, today!", &[]);
        let b = match_words("Speaker 2 will send the deck today", &["Speaker 2"]);
        assert_eq!(a, b);
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-9);
        let meeting = match_words("Me books the meeting room", &["Me"]);
        assert!(meeting.contains("meeting") && !meeting.contains("me"));
        assert_eq!(jaccard(&match_words("", &[]), &match_words("!", &[])), 0.0);
    }
}
