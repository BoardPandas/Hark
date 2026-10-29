//! Naming the speakers after a final pass (spec §2.6): the "Your name" row
//! for Me, then guesses from names said in the call. The text provider
//! guesses when notes are on and it resolves with a key (the same provider
//! and consent that already receive the transcript for notes); any failure
//! there, or no provider, falls back to the on-device rules. Every guess,
//! either way, must cite a line that says the name.
//!
//! Never during the call. Logs carry the engine, counts and error slugs,
//! never a name or a line.

use super::{LiveSegment, MeetingEvent};
use hark_config::{CleanupKeySource, CleanupResolution, Settings, VoiceName};
use hark_meeting::speakers::{guess_local, validate, Candidate, Guess, NameLine};
use hark_meeting::{default_label, SpeakerKey};
use hark_voice::{RawGuess, SummaryConfig, SummaryError};
use std::sync::mpsc::Sender;

/// The settings field and every rename share this cap.
const YOUR_NAME_MAX_CHARS: usize = 40;

/// A name for the storage worker. `guess` rows never overwrite a user's
/// name; the "Your name" row is the user's own. Carries a name: no `Debug`.
pub struct NamedSpeaker {
    /// A `SpeakerKey` as text.
    pub key: String,
    pub name: String,
    pub guess: bool,
    /// Guess only: start of the line the name was heard in.
    pub evidence_ms: Option<i64>,
}

/// One transcribed line, kept for naming after its segment has gone to
/// the storage worker.
pub(super) struct SpeakerLine {
    pub key: SpeakerKey,
    pub start_ms: u64,
    pub text: String,
}

pub(super) fn lines_of(segments: &[LiveSegment]) -> Vec<SpeakerLine> {
    segments
        .iter()
        .map(|s| SpeakerLine {
            key: SpeakerKey::transcribed(s.channel, s.speaker),
            start_ms: s.start_ms,
            text: s.text.clone(),
        })
        .collect()
}

/// The notes provider, resolved exactly as for the notes themselves.
pub(super) fn notes_provider(settings: &Settings) -> Result<SummaryConfig, String> {
    let resolved = match hark_config::resolve_cleanup_provider(
        &settings.provider,
        &settings.voice,
        VoiceName::Notes,
    ) {
        CleanupResolution::Resolved(r) => r,
        CleanupResolution::VerbatimWithWarning { reason } => return Err(reason),
        CleanupResolution::Verbatim => return Err("no text provider is configured".to_string()),
    };
    let api_key = match &resolved.key_source {
        CleanupKeySource::ReuseSttKey => hark_keychain::resolve_key(settings.provider.kind.label()),
        CleanupKeySource::Account(account) => {
            hark_keychain::resolve_key_for(hark_keychain::CLEANUP_ENV_OVERRIDE, account)
        }
    }
    .map_err(|e| e.to_string())?;
    Ok(SummaryConfig {
        label: resolved.kind.label().to_string(),
        base_url: resolved.base_url.clone(),
        model: resolved.model.clone(),
        api_key,
        temperature: resolved.temperature,
        reasoning_effort: resolved.reasoning_effort.clone(),
    })
}

/// A provider failure as a fixed slug, for logs and the UI's reasons.
pub(super) fn failure_slug(error: &SummaryError) -> &'static str {
    match error {
        SummaryError::Timeout { .. } => "timeout",
        SummaryError::Http { .. } => "network",
        SummaryError::Auth { .. } => "auth",
        SummaryError::RateLimited { .. } => "rate-limited",
        SummaryError::Provider { .. } => "provider",
    }
}

/// Name the speakers of `lines` (empty for a meeting that kept its live
/// transcript: then only "Your name" applies) and send the result.
pub(super) fn name_speakers(
    settings: &Settings,
    id: &str,
    lines: &[SpeakerLine],
    events: &Sender<MeetingEvent>,
) {
    let names = names_for(
        settings,
        lines,
        id,
        || notes_provider(settings),
        |cfg, l, g| {
            let client = hark_stt::shared_client().map_err(|e| SummaryError::Http {
                provider: cfg.label.clone(),
                detail: e.to_string(),
            })?;
            hark_voice::guess_speaker_names(&client, cfg, l, g)
        },
    );
    if !names.is_empty() {
        let _ = events.send(MeetingEvent::SpeakerNames {
            id: id.to_string(),
            names,
        });
    }
}

/// The testable core: `provider` resolves the notes provider, `ask` makes
/// the call.
fn names_for(
    settings: &Settings,
    lines: &[SpeakerLine],
    id: &str,
    provider: impl FnOnce() -> Result<SummaryConfig, String>,
    ask: impl FnOnce(
        &SummaryConfig,
        &[hark_voice::NameLine],
        &[&str],
    ) -> Result<Vec<RawGuess>, SummaryError>,
) -> Vec<NamedSpeaker> {
    let your_name: String = settings
        .meeting
        .your_name
        .trim()
        .chars()
        .take(YOUR_NAME_MAX_CHARS)
        .collect();
    let mut names = Vec::new();
    if !your_name.is_empty() {
        names.push(NamedSpeaker {
            key: SpeakerKey::Me.to_string(),
            name: your_name.clone(),
            guess: false,
            evidence_ms: None,
        });
    }
    if !settings.meeting.guess_speaker_names || lines.is_empty() {
        return names;
    }
    let mut guessable: Vec<SpeakerKey> = Vec::new();
    for l in lines {
        let named_me = l.key == SpeakerKey::Me && !your_name.is_empty();
        if !named_me && !guessable.contains(&l.key) {
            guessable.push(l.key);
        }
    }
    if guessable.is_empty() {
        return names;
    }
    let name_lines: Vec<NameLine> = lines
        .iter()
        .map(|l| NameLine {
            key: l.key,
            start_ms: l.start_ms,
            text: &l.text,
        })
        .collect();

    let mut engine = "on-device";
    let mut found: Option<(Vec<Guess>, usize)> = None;
    if settings.meeting.summary {
        match provider() {
            Ok(cfg) => match ask_provider(&cfg, lines, &name_lines, &guessable, &your_name, ask) {
                Ok(result) => {
                    engine = "provider";
                    found = Some(result);
                }
                Err(error) => log::info!(
                    "meeting {id}: provider name guess failed ({}); using on-device rules",
                    failure_slug(&error)
                ),
            },
            Err(_) => log::info!("meeting {id}: no text provider; naming speakers on device"),
        }
    }
    let (guesses, rejected) =
        found.unwrap_or_else(|| guess_local(&name_lines, &guessable, &your_name));
    log::info!(
        "meeting {id}: speaker names by {engine}: {} accepted, {rejected} rejected",
        guesses.len()
    );
    names.extend(guesses.into_iter().map(|g| NamedSpeaker {
        key: g.key.to_string(),
        name: g.name,
        guess: true,
        evidence_ms: Some(g.evidence_ms as i64),
    }));
    names
}

/// The provider's guesses, mapped from default labels back to keys and
/// validated against the lines they cite.
fn ask_provider(
    cfg: &SummaryConfig,
    lines: &[SpeakerLine],
    name_lines: &[NameLine],
    guessable: &[SpeakerKey],
    your_name: &str,
    ask: impl FnOnce(
        &SummaryConfig,
        &[hark_voice::NameLine],
        &[&str],
    ) -> Result<Vec<RawGuess>, SummaryError>,
) -> Result<(Vec<Guess>, usize), SummaryError> {
    let labels: Vec<String> = lines.iter().map(|l| default_label(l.key)).collect();
    let request: Vec<hark_voice::NameLine> = lines
        .iter()
        .zip(&labels)
        .map(|(l, label)| hark_voice::NameLine {
            label,
            start_ms: l.start_ms,
            text: &l.text,
        })
        .collect();
    let guessable_labels: Vec<String> = guessable.iter().map(|k| default_label(*k)).collect();
    let guessable_refs: Vec<&str> = guessable_labels.iter().map(String::as_str).collect();
    let raw = ask(cfg, &request, &guessable_refs)?;
    let mut unknown = 0;
    let candidates: Vec<Candidate> = raw
        .into_iter()
        .filter_map(|g| {
            let key = guessable
                .iter()
                .zip(&guessable_labels)
                .find(|(_, label)| **label == g.label)
                .map(|(key, _)| *key);
            if key.is_none() {
                unknown += 1;
            }
            Some(Candidate {
                key: key?,
                name: g.name,
                line: g.line,
            })
        })
        .collect();
    let (guesses, rejected) = validate(candidates, name_lines, guessable, your_name);
    Ok((guesses, rejected + unknown))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(summary: bool, guess: bool, your_name: &str) -> Settings {
        let mut s = Settings::default();
        s.meeting.summary = summary;
        s.meeting.guess_speaker_names = guess;
        s.meeting.your_name = your_name.to_string();
        s
    }

    fn lines() -> Vec<SpeakerLine> {
        [
            (SpeakerKey::Me, "Hi, this is Chaz."),
            (SpeakerKey::Diarized(0), "Hello, this is Sam from Acme."),
            (SpeakerKey::Diarized(1), "And I'm Lee."),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (key, text))| SpeakerLine {
            key,
            start_ms: i as u64 * 1_000,
            text: text.to_string(),
        })
        .collect()
    }

    fn config() -> SummaryConfig {
        SummaryConfig {
            label: "openai".into(),
            base_url: "https://example.invalid".into(),
            model: "m".into(),
            api_key: "k".into(),
            temperature: None,
            reasoning_effort: None,
        }
    }

    fn raw(label: &str, name: &str, line: usize) -> RawGuess {
        RawGuess {
            label: label.into(),
            name: name.into(),
            line,
        }
    }

    fn summary(names: &[NamedSpeaker]) -> Vec<(String, String, bool)> {
        names
            .iter()
            .map(|n| (n.key.clone(), n.name.clone(), n.guess))
            .collect()
    }

    fn row(key: &str, name: &str, guess: bool) -> (String, String, bool) {
        (key.into(), name.into(), guess)
    }

    #[test]
    fn notes_on_with_a_provider_asks_it_with_default_labels_only() {
        let mut asked = false;
        let names = names_for(
            &settings(true, true, "Chaz"),
            &lines(),
            "m",
            || Ok(config()),
            |_, request, guessable| {
                asked = true;
                // No user name reaches the request: labels are defaults, and
                // Me is not guessable once "Your name" names it.
                assert_eq!(guessable, ["Speaker 1", "Speaker 2"]);
                let labels: Vec<&str> = request.iter().map(|l| l.label).collect();
                assert_eq!(labels, ["Me", "Speaker 1", "Speaker 2"]);
                Ok(vec![
                    raw("Speaker 1", "Sam", 1),
                    raw("Speaker 2", "Pat", 2),
                    raw("Speaker 9", "Kim", 2),
                ])
            },
        );
        assert!(asked);
        // Pat is not in the line it cites; Speaker 9 is not in the meeting.
        assert_eq!(
            summary(&names),
            [row("me", "Chaz", false), row("them:0", "Sam", true)]
        );
        assert_eq!(names[1].evidence_ms, Some(1_000));
    }

    #[test]
    fn a_failed_provider_call_falls_back_to_the_on_device_rules() {
        for failure in [
            SummaryError::Timeout {
                provider: "openai".into(),
                configured_ms: 1,
            },
            SummaryError::Provider {
                provider: "openai".into(),
                detail: "empty speaker-names response".into(),
            },
        ] {
            let names = names_for(
                &settings(true, true, ""),
                &lines(),
                "m",
                || Ok(config()),
                |_, _, _| Err(failure),
            );
            assert_eq!(
                summary(&names),
                [
                    row("me", "Chaz", true),
                    row("them:0", "Sam", true),
                    row("them:1", "Lee", true)
                ]
            );
        }
    }

    #[test]
    fn notes_off_or_no_provider_never_asks() {
        for (s, provider_ok) in [
            (settings(false, true, ""), true),
            (settings(true, true, ""), false),
        ] {
            let names = names_for(
                &s,
                &lines(),
                "m",
                || {
                    if provider_ok {
                        Ok(config())
                    } else {
                        Err("none".into())
                    }
                },
                |_, _, _| panic!("the provider must not be asked"),
            );
            assert_eq!(names.len(), 3, "on-device rules named everyone");
        }
    }

    #[test]
    fn guessing_off_still_names_me_from_your_name() {
        let names = names_for(
            &settings(true, false, "  Chaz  "),
            &lines(),
            "m",
            || Ok(config()),
            |_, _, _| panic!("no guessing"),
        );
        assert_eq!(summary(&names), [row("me", "Chaz", false)]);
        let none = names_for(
            &settings(true, false, ""),
            &lines(),
            "m",
            || Ok(config()),
            |_, _, _| panic!("no guessing"),
        );
        assert!(none.is_empty());
        // A kept live transcript: only "Your name" applies.
        let live = names_for(
            &settings(true, true, "Chaz"),
            &[],
            "m",
            || Ok(config()),
            |_, _, _| panic!("no lines"),
        );
        assert_eq!(summary(&live), [row("me", "Chaz", false)]);
    }
}
