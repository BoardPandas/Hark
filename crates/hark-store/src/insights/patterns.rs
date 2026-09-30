//! Explicitly requested local analysis; no derived words are persisted.

use super::{InsightsRequest, PhraseCount, VoicePatterns};
use crate::StoreError;
use rusqlite::{params, Connection};
use std::collections::HashMap;

const MAX_ENTRIES: usize = 5_000;
const MAX_TOKENS: usize = 200_000;
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "do", "for", "from",
    "had", "has", "have", "he", "her", "his", "i", "i'm", "if", "in", "is", "it", "it's", "its",
    "me", "my", "not", "of", "on", "or", "our", "she", "so", "that", "the", "their", "them",
    "there", "they", "this", "to", "was", "we", "were", "will", "with", "would", "you", "your",
];

pub(super) fn analyze(
    conn: &Connection,
    start_ms: i64,
    request: &InsightsRequest,
) -> Result<VoicePatterns, StoreError> {
    let matching_dictations = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE ts_ms >= ?1 AND ts_ms <= ?2",
        params![start_ms, request.now_ms],
        |r| r.get::<_, i64>(0),
    )?;
    let mut query = conn.prepare(
        "SELECT CASE WHEN invocation IS NULL THEN final_text ELSE raw_text END \
         FROM entries WHERE ts_ms >= ?1 AND ts_ms <= ?2 ORDER BY ts_ms DESC, id DESC LIMIT ?3",
    )?;
    let mut rows = query.query(params![start_ms, request.now_ms, MAX_ENTRIES as i64])?;
    let mut words = HashMap::<String, i64>::new();
    let mut phrases = HashMap::<String, i64>::new();
    let mut sampled_dictations = 0;
    let mut token_count = 0;
    let mut truncated = matching_dictations > MAX_ENTRIES as i64;
    while let Some(row) = rows.next()? {
        let text: String = row.get(0)?;
        let remaining = MAX_TOKENS - token_count;
        // One extra token detects truncation without consuming an unbounded
        // allocation on a unusually long invocation or imported transcript.
        let mut tokens: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
            .filter(|part| !part.is_empty())
            .take(remaining + 1)
            .map(|part| part.to_lowercase().replace('’', "'"))
            .collect();
        if tokens.len() > remaining {
            tokens.truncate(remaining);
            truncated = true;
        }
        token_count += tokens.len();
        sampled_dictations += 1;
        for word in &tokens {
            if word.chars().count() > 1 && !STOP_WORDS.contains(&word.as_str()) {
                *words.entry(word.clone()).or_default() += 1;
            }
        }
        for size in [2, 3] {
            for phrase in tokens.windows(size) {
                if phrase
                    .iter()
                    .any(|word| !STOP_WORDS.contains(&word.as_str()))
                {
                    *phrases.entry(phrase.join(" ")).or_default() += 1;
                }
            }
        }
        if token_count == MAX_TOKENS {
            truncated |= sampled_dictations < matching_dictations;
            break;
        }
    }
    Ok(VoicePatterns {
        words: top(words, 1),
        phrases: top(phrases, 2),
        sampled_dictations,
        matching_dictations,
        truncated,
    })
}

fn top(values: HashMap<String, i64>, minimum: i64) -> Vec<PhraseCount> {
    let mut values: Vec<_> = values
        .into_iter()
        .filter(|(_, count)| *count >= minimum)
        .map(|(text, count)| PhraseCount { text, count })
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.text.cmp(&b.text)));
    values.truncate(8);
    values
}
