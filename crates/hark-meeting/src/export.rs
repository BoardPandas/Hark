//! Renders a meeting as Markdown or plain text for the Share menu (plan
//! §4.10): "Copy notes", "Copy transcript" and "Export notes + transcript…"
//! all go through [`to_markdown`] or [`to_text`] over an [`ExportMeeting`].
//!
//! Pure by construction: no clock, no filesystem, no clipboard. The caller
//! (`hark-app`) resolves speaker labels with [`speaker_label`], formats
//! `started` in the user's local time, and does the actual file write or
//! clipboard set on a worker thread.
//!
//! Markdown output escapes user text (see [`escape_markdown`]) so a
//! transcript line can never turn into a heading, list item or link by
//! accident. Plain text has no markup to escape.

use crate::Channel;

mod subtitles;
pub use subtitles::{excerpt, to_srt, to_vtt};

/// One meeting, ready to render. Built by the caller from the database.
pub struct ExportMeeting {
    pub title: String,
    /// Preformatted by the caller in the user's local time, e.g. "Sat 27 Sep
    /// 2026, 14:30".
    pub started: String,
    pub duration_ms: u64,
    pub lines: Vec<ExportLine>,
    pub notes: Option<ExportNotes>,
}

/// One transcript line. `speaker` is already resolved (see [`speaker_label`]);
/// this module never sees a raw speaker id.
pub struct ExportLine {
    pub at_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
    pub text: String,
}

pub struct ExportNotes {
    pub summary: String,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<ExportAction>,
}

pub struct ExportAction {
    pub text: String,
    pub owner: Option<String>,
    pub done: bool,
}

/// Which parts of the meeting to render. All `true` by default: the caller
/// turns parts off for "Copy notes" (transcript off) or "Copy transcript"
/// (notes off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub notes: bool,
    pub transcript: bool,
    pub timestamps: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            notes: true,
            transcript: true,
            timestamps: true,
        }
    }
}

/// Renders the meeting as Markdown: `# Title`, a `started · duration` line,
/// then `## Summary`, `## Key points`, `## Decisions`, `## Action items` and
/// `## Transcript`, each omitted entirely when it would be empty.
pub fn to_markdown(m: &ExportMeeting, opts: ExportOptions) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("# {}", escape_markdown(&m.title)));
    sections.push(format!(
        "{} · {}",
        m.started,
        format_duration(m.duration_ms)
    ));

    if opts.notes {
        if let Some(notes) = &m.notes {
            if !notes.summary.trim().is_empty() {
                sections.push(format!("## Summary\n\n{}", escape_markdown(&notes.summary)));
            }
            if !notes.key_points.is_empty() {
                let bullets = notes
                    .key_points
                    .iter()
                    .map(|p| format!("- {}", escape_markdown(p)))
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("## Key points\n\n{bullets}"));
            }
            if !notes.decisions.is_empty() {
                let bullets = notes
                    .decisions
                    .iter()
                    .map(|d| format!("- {}", escape_markdown(d)))
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("## Decisions\n\n{bullets}"));
            }
            if !notes.action_items.is_empty() {
                let items = notes
                    .action_items
                    .iter()
                    .map(markdown_action_line)
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("## Action items\n\n{items}"));
            }
        }
    }

    if opts.transcript {
        let body = if m.lines.is_empty() {
            "No transcript.".to_string()
        } else {
            m.lines
                .iter()
                .map(|l| markdown_transcript_line(l, opts.timestamps))
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!("## Transcript\n\n{body}"));
    }

    sections.join("\n\n") + "\n"
}

fn markdown_action_line(a: &ExportAction) -> String {
    let mark = if a.done { 'x' } else { ' ' };
    let owner = a
        .owner
        .as_deref()
        .map(|o| format!(" ({})", escape_markdown(o)))
        .unwrap_or_default();
    format!("- [{mark}] {}{owner}", escape_markdown(&a.text))
}

fn markdown_transcript_line(l: &ExportLine, timestamps: bool) -> String {
    let speaker = escape_markdown(&l.speaker);
    let text = escape_markdown(&l.text);
    if timestamps {
        format!("**[{}] {speaker}:** {text}", format_timestamp(l.at_ms))
    } else {
        format!("**{speaker}:** {text}")
    }
}

/// Renders the meeting as plain text: the same structure as [`to_markdown`],
/// uppercase section headings, and no Markdown syntax (so nothing needs
/// escaping).
pub fn to_text(m: &ExportMeeting, opts: ExportOptions) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(m.title.clone());
    sections.push(format!(
        "{} · {}",
        m.started,
        format_duration(m.duration_ms)
    ));

    if opts.notes {
        if let Some(notes) = &m.notes {
            if !notes.summary.trim().is_empty() {
                sections.push(format!("SUMMARY\n\n{}", notes.summary));
            }
            if !notes.key_points.is_empty() {
                let bullets = notes
                    .key_points
                    .iter()
                    .map(|p| format!("- {p}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("KEY POINTS\n\n{bullets}"));
            }
            if !notes.decisions.is_empty() {
                let bullets = notes
                    .decisions
                    .iter()
                    .map(|d| format!("- {d}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("DECISIONS\n\n{bullets}"));
            }
            if !notes.action_items.is_empty() {
                let items = notes
                    .action_items
                    .iter()
                    .map(text_action_line)
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!("ACTION ITEMS\n\n{items}"));
            }
        }
    }

    if opts.transcript {
        let body = if m.lines.is_empty() {
            "No transcript.".to_string()
        } else {
            m.lines
                .iter()
                .map(|l| text_transcript_line(l, opts.timestamps))
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!("TRANSCRIPT\n\n{body}"));
    }

    sections.join("\n\n") + "\n"
}

fn text_action_line(a: &ExportAction) -> String {
    let mark = if a.done { 'x' } else { ' ' };
    let owner = a
        .owner
        .as_deref()
        .map(|o| format!(" ({o})"))
        .unwrap_or_default();
    format!("[{mark}] {}{owner}", a.text)
}

fn text_transcript_line(l: &ExportLine, timestamps: bool) -> String {
    if timestamps {
        format!("[{}] {}: {}", format_timestamp(l.at_ms), l.speaker, l.text)
    } else {
        format!("{}: {}", l.speaker, l.text)
    }
}

/// Escapes the Markdown-significant characters in one block of user text, so
/// it can only ever render as a plain paragraph: a leading `#`/`-`/`+`/`>`,
/// or an ordered-list `N.`, would otherwise turn a transcript line into a
/// heading, list item or blockquote, and a bare `*`/`_`/`` ` ``/`[`/`]`
/// anywhere would turn it into emphasis, code or (with both brackets) a link.
fn escape_markdown(text: &str) -> String {
    text.lines()
        .map(escape_markdown_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_markdown_line(line: &str) -> String {
    let ws_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (ws, mut rest) = line.split_at(ws_len);
    let mut out = String::with_capacity(line.len() + 2);
    out.push_str(ws);

    match rest.chars().next() {
        Some('#' | '-' | '+' | '>') => out.push('\\'),
        Some(c) if c.is_ascii_digit() => {
            let digit_len = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            if rest[digit_len..].starts_with('.') {
                out.push_str(&rest[..digit_len]);
                out.push_str("\\.");
                rest = &rest[digit_len + 1..];
            }
        }
        _ => {}
    }

    for c in rest.chars() {
        if matches!(c, '*' | '_' | '[' | ']' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `"mm:ss"` under an hour, `"h:mm:ss"` from an hour.
pub fn format_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// `"1 h 5 min"`, `"12 min"`, `"45 s"`.
pub fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    if total_secs < 60 {
        return format!("{total_secs} s");
    }
    let total_mins = total_secs / 60;
    if total_mins < 60 {
        return format!("{total_mins} min");
    }
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if mins == 0 {
        format!("{hours} h")
    } else {
        format!("{hours} h {mins} min")
    }
}

/// The label for a transcript line. `Me` is always "Me" (the caller
/// substitutes the user's own display name, if set, on top of this). `Them`
/// with no diarized speaker is "Them"; a diarized speaker is its rename if
/// one exists, else `"Speaker {n+1}"` for the 0-based id Deepgram returns.
pub fn speaker_label(channel: Channel, speaker: Option<u32>, renames: &[(u32, String)]) -> String {
    if channel == Channel::Me {
        return "Me".to_string();
    }
    match speaker {
        None => "Them".to_string(),
        Some(id) => renames
            .iter()
            .find(|(rid, _)| *rid == id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| {
                if id & 0x8000_0000 != 0 {
                    let window = (id & 0x7fff_ffff) >> 10;
                    let speaker = id & 0x3ff;
                    format!("Window {} · Speaker {}", window + 1, speaker + 1)
                } else {
                    format!("Speaker {}", id + 1)
                }
            }),
    }
}

/// Windows device names reserved regardless of extension (`CON.txt` collides
/// just like `CON`, since Windows matches everything before the first dot).
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_reserved_device_name(stem: &str) -> bool {
    let name = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    RESERVED_DEVICE_NAMES.contains(&name.as_str())
}

/// A Windows-safe `"<title>.<ext>"`: strips `<>:"/\|?*` and control
/// characters, collapses whitespace, trims trailing dots/spaces, caps the
/// stem at 80 characters on a char boundary (re-trimming any dot/space the
/// cut exposes), falls back to `"Meeting"` when nothing is left, and appends
/// `"_"` if the result collides with a reserved device name.
pub fn safe_file_name(title: &str, ext: &str) -> String {
    const ILLEGAL: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control() && !ILLEGAL.contains(c))
        .collect();

    // split_whitespace both collapses whitespace runs to one space and drops
    // leading/trailing whitespace for free.
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(['.', ' ']);

    let mut stem: String = trimmed.chars().take(80).collect();
    stem = stem.trim_end_matches(['.', ' ']).to_string();

    if stem.is_empty() {
        stem = "Meeting".to_string();
    }
    if is_reserved_device_name(&stem) {
        stem.push('_');
    }

    format!("{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ExportMeeting {
        ExportMeeting {
            title: "Weekly Sync".to_string(),
            started: "Sat 27 Sep 2026, 14:30".to_string(),
            duration_ms: 65 * 60 * 1000,
            lines: vec![
                ExportLine {
                    at_ms: 0,
                    end_ms: 10_000,
                    speaker: "Me".to_string(),
                    text: "Let's get started.".to_string(),
                },
                ExportLine {
                    at_ms: 12_000,
                    end_ms: 14_000,
                    speaker: "Dana".to_string(),
                    text: "Sounds good.".to_string(),
                },
            ],
            notes: Some(ExportNotes {
                summary: "Reviewed Q3 goals and next steps.".to_string(),
                key_points: vec![
                    "Revenue is up 12%.".to_string(),
                    "Hiring plan approved.".to_string(),
                ],
                decisions: vec!["Ship the v2 API by October.".to_string()],
                action_items: vec![
                    ExportAction {
                        text: "Draft the roadmap doc".to_string(),
                        owner: Some("Dana".to_string()),
                        done: false,
                    },
                    ExportAction {
                        text: "Send meeting notes".to_string(),
                        owner: None,
                        done: true,
                    },
                ],
            }),
        }
    }

    #[test]
    fn markdown_golden_full_meeting() {
        let expected = [
            "# Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "## Summary",
            "",
            "Reviewed Q3 goals and next steps.",
            "",
            "## Key points",
            "",
            "- Revenue is up 12%.",
            "- Hiring plan approved.",
            "",
            "## Decisions",
            "",
            "- Ship the v2 API by October.",
            "",
            "## Action items",
            "",
            "- [ ] Draft the roadmap doc (Dana)",
            "- [x] Send meeting notes",
            "",
            "## Transcript",
            "",
            "**[00:00] Me:** Let's get started.",
            "**[00:12] Dana:** Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_markdown(&fixture(), ExportOptions::default()), expected);
    }

    #[test]
    fn text_golden_full_meeting() {
        let expected = [
            "Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "SUMMARY",
            "",
            "Reviewed Q3 goals and next steps.",
            "",
            "KEY POINTS",
            "",
            "- Revenue is up 12%.",
            "- Hiring plan approved.",
            "",
            "DECISIONS",
            "",
            "- Ship the v2 API by October.",
            "",
            "ACTION ITEMS",
            "",
            "[ ] Draft the roadmap doc (Dana)",
            "[x] Send meeting notes",
            "",
            "TRANSCRIPT",
            "",
            "[00:00] Me: Let's get started.",
            "[00:12] Dana: Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_text(&fixture(), ExportOptions::default()), expected);
    }

    #[test]
    fn no_notes_omits_every_notes_section() {
        let mut m = fixture();
        m.notes = None;
        let expected = [
            "# Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "## Transcript",
            "",
            "**[00:00] Me:** Let's get started.",
            "**[00:12] Dana:** Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_markdown(&m, ExportOptions::default()), expected);
    }

    #[test]
    fn empty_notes_subsections_are_each_omitted() {
        let mut m = fixture();
        m.notes = Some(ExportNotes {
            summary: "   ".to_string(),
            key_points: vec![],
            decisions: vec![],
            action_items: vec![],
        });
        let expected = [
            "# Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "## Transcript",
            "",
            "**[00:00] Me:** Let's get started.",
            "**[00:12] Dana:** Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_markdown(&m, ExportOptions::default()), expected);
    }

    #[test]
    fn no_timestamps_drops_the_bracket_but_keeps_the_speaker() {
        let opts = ExportOptions {
            notes: false,
            transcript: true,
            timestamps: false,
        };
        let expected = [
            "# Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "## Transcript",
            "",
            "**Me:** Let's get started.",
            "**Dana:** Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_markdown(&fixture(), opts), expected);

        let expected_text = [
            "Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "TRANSCRIPT",
            "",
            "Me: Let's get started.",
            "Dana: Sounds good.",
            "",
        ]
        .join("\n");

        assert_eq!(to_text(&fixture(), opts), expected_text);
    }

    #[test]
    fn empty_transcript_says_so_instead_of_an_empty_section() {
        let mut m = fixture();
        m.lines = vec![];
        let opts = ExportOptions {
            notes: false,
            transcript: true,
            timestamps: true,
        };
        let expected = [
            "# Weekly Sync",
            "",
            "Sat 27 Sep 2026, 14:30 · 1 h 5 min",
            "",
            "## Transcript",
            "",
            "No transcript.",
            "",
        ]
        .join("\n");

        assert_eq!(to_markdown(&m, opts), expected);
    }

    #[test]
    fn notes_only_option_omits_the_transcript_section() {
        let opts = ExportOptions {
            notes: true,
            transcript: false,
            timestamps: true,
        };
        let md = to_markdown(&fixture(), opts);
        assert!(!md.contains("## Transcript"));
        assert!(md.contains("## Summary"));
    }

    #[test]
    fn transcript_only_option_omits_every_notes_section() {
        let opts = ExportOptions {
            notes: false,
            transcript: true,
            timestamps: true,
        };
        let md = to_markdown(&fixture(), opts);
        assert!(!md.contains("## Summary"));
        assert!(!md.contains("## Key points"));
        assert!(!md.contains("## Decisions"));
        assert!(!md.contains("## Action items"));
        assert!(md.contains("## Transcript"));
    }

    #[test]
    fn markdown_escapes_lines_that_would_become_structure() {
        let mut m = fixture();
        m.lines = vec![
            ExportLine {
                at_ms: 0,
                end_ms: 1_000,
                speaker: "Me".to_string(),
                text: "# not a heading".to_string(),
            },
            ExportLine {
                at_ms: 1_000,
                end_ms: 2_000,
                speaker: "Me".to_string(),
                text: "1. item".to_string(),
            },
            ExportLine {
                at_ms: 2_000,
                end_ms: 3_000,
                speaker: "Me".to_string(),
                text: "[link](x)".to_string(),
            },
        ];
        m.notes = None;
        let md = to_markdown(&m, ExportOptions::default());
        assert!(md.contains(r"\# not a heading"), "{md}");
        assert!(md.contains(r"1\. item"), "{md}");
        assert!(md.contains(r"\[link\](x)"), "{md}");

        // Plain text has no markup to break, so nothing is escaped.
        let txt = to_text(&m, ExportOptions::default());
        assert!(txt.contains("# not a heading"), "{txt}");
        assert!(txt.contains("1. item"), "{txt}");
        assert!(txt.contains("[link](x)"), "{txt}");
    }

    #[test]
    fn escape_markdown_handles_anywhere_characters_and_leading_markers() {
        assert_eq!(
            escape_markdown("*bold* and `code`"),
            r"\*bold\* and \`code\`"
        );
        assert_eq!(escape_markdown("- bullet"), r"\- bullet");
        assert_eq!(escape_markdown("> quote"), r"\> quote");
        assert_eq!(escape_markdown("+ plus"), r"\+ plus");
        assert_eq!(escape_markdown("plain sentence."), "plain sentence.");
        assert_eq!(escape_markdown("under_score"), r"under\_score");
    }

    #[test]
    fn format_timestamp_boundaries() {
        assert_eq!(format_timestamp(0), "00:00");
        assert_eq!(format_timestamp(303_000), "05:03");
        assert_eq!(format_timestamp(3_599_000), "59:59");
        assert_eq!(format_timestamp(3_600_000), "1:00:00");
    }

    #[test]
    fn format_duration_boundaries() {
        assert_eq!(format_duration(45_000), "45 s");
        assert_eq!(format_duration(59_999), "59 s");
        assert_eq!(format_duration(60_000), "1 min");
        assert_eq!(format_duration(12 * 60 * 1000), "12 min");
        assert_eq!(format_duration(3_600_000), "1 h");
        assert_eq!(format_duration(65 * 60 * 1000), "1 h 5 min");
    }

    #[test]
    fn speaker_label_me_ignores_speaker_and_renames() {
        assert_eq!(
            speaker_label(Channel::Me, Some(3), &[(3, "Dana".to_string())]),
            "Me"
        );
        assert_eq!(speaker_label(Channel::Me, None, &[]), "Me");
    }

    #[test]
    fn speaker_label_them_without_diarization_is_them() {
        assert_eq!(speaker_label(Channel::Them, None, &[]), "Them");
    }

    #[test]
    fn speaker_label_them_falls_back_to_a_1_based_number() {
        assert_eq!(speaker_label(Channel::Them, Some(0), &[]), "Speaker 1");
        assert_eq!(speaker_label(Channel::Them, Some(2), &[]), "Speaker 3");
    }

    #[test]
    fn speaker_label_them_uses_the_rename_for_its_0_based_id() {
        let renames = [(0, "Alice".to_string()), (2, "Dana".to_string())];
        assert_eq!(speaker_label(Channel::Them, Some(2), &renames), "Dana");
        // A rename for a different id never matches.
        assert_eq!(speaker_label(Channel::Them, Some(1), &renames), "Speaker 2");
    }

    #[test]
    fn window_scoped_speakers_are_explicit_and_renames_still_win() {
        let id = 0x8000_0000 | (2 << 10) | 1;
        assert_eq!(
            speaker_label(Channel::Them, Some(id), &[]),
            "Window 3 · Speaker 2"
        );
        assert_eq!(
            speaker_label(Channel::Them, Some(id), &[(id, "Dana".into())]),
            "Dana"
        );
        assert_eq!(speaker_label(Channel::Me, Some(id), &[]), "Me");
    }

    #[test]
    fn safe_file_name_strips_illegal_characters() {
        assert_eq!(safe_file_name("Q3: Plan?", "md"), "Q3 Plan.md");
        assert_eq!(safe_file_name("Report <Draft>", "md"), "Report Draft.md");
    }

    #[test]
    fn safe_file_name_collapses_whitespace() {
        assert_eq!(
            safe_file_name("Weekly   Sync   Update", "md"),
            "Weekly Sync Update.md"
        );
    }

    #[test]
    fn safe_file_name_trims_trailing_dots_and_spaces() {
        assert_eq!(safe_file_name("Notes.   ", "md"), "Notes.md");
        assert_eq!(safe_file_name("Notes...", "txt"), "Notes.txt");
    }

    #[test]
    fn safe_file_name_avoids_reserved_device_names() {
        assert_eq!(safe_file_name("con", "md"), "con_.md");
        assert_eq!(safe_file_name("CON", "md"), "CON_.md");
        assert_eq!(safe_file_name("CON.txt", "md"), "CON.txt_.md");
        assert_eq!(safe_file_name("lpt9", "txt"), "lpt9_.txt");
        // A name that merely starts with a reserved word is not reserved.
        assert_eq!(safe_file_name("Console Notes", "md"), "Console Notes.md");
    }

    #[test]
    fn safe_file_name_caps_the_stem_at_80_chars_on_a_char_boundary() {
        let title = "é".repeat(90);
        let name = safe_file_name(&title, "md");
        let stem = name.strip_suffix(".md").expect("has the extension");
        assert_eq!(stem.chars().count(), 80);
        // The cut must land on a char boundary: this would panic otherwise.
        assert_eq!(stem, "é".repeat(80));
    }

    #[test]
    fn safe_file_name_falls_back_to_meeting_when_nothing_is_left() {
        assert_eq!(safe_file_name("", "md"), "Meeting.md");
        assert_eq!(safe_file_name("   ", "md"), "Meeting.md");
        assert_eq!(safe_file_name("???///", "md"), "Meeting.md");
    }
}
