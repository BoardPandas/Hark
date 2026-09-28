//! Subtitle timing and excerpt selection, independent of file dialogs/codecs.

use super::{ExportLine, ExportMeeting};

/// SubRip cues. Concurrent speakers remain overlapping cues, in start order.
pub fn to_srt(meeting: &ExportMeeting) -> String {
    subtitles(meeting, false)
}

/// WebVTT cues, with markup in speaker names/transcripts escaped as text.
pub fn to_vtt(meeting: &ExportMeeting) -> String {
    subtitles(meeting, true)
}

fn subtitles(meeting: &ExportMeeting, vtt: bool) -> String {
    let mut output = if vtt {
        "WEBVTT\n\n".to_string()
    } else {
        String::new()
    };
    let mut lines: Vec<_> = meeting.lines.iter().collect();
    lines.sort_by_key(|line| line.at_ms);
    let mut index = 0;
    for line in lines {
        let end = line.end_ms.min(meeting.duration_ms);
        if end <= line.at_ms || line.text.trim().is_empty() {
            continue;
        }
        index += 1;
        output.push_str(&format!(
            "{index}\n{} --> {}\n{}: {}\n\n",
            timestamp(line.at_ms, vtt),
            timestamp(end, vtt),
            cue_text(&line.speaker),
            cue_text(&line.text),
        ));
    }
    output
}

fn timestamp(ms: u64, vtt: bool) -> String {
    let sep = if vtt { '.' } else { ',' };
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1_000) % 60,
        ms % 1_000
    )
}

fn cue_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Select lines intersecting `[start_ms, end_ms)`, clip their timestamps and
/// rebase them to the excerpt. Text uses whole transcript segments: there are
/// no word timestamps to justify cutting inside a sentence. Meeting-wide
/// notes are omitted so an excerpt never silently includes unrelated content.
pub fn excerpt(
    meeting: &ExportMeeting,
    start_ms: u64,
    end_ms: u64,
) -> Result<ExportMeeting, &'static str> {
    if start_ms >= end_ms || end_ms > meeting.duration_ms {
        return Err("Choose an end after the start, within the recording.");
    }
    Ok(ExportMeeting {
        title: format!("{} — excerpt", meeting.title),
        started: format!(
            "{} · excerpt {}–{}",
            meeting.started,
            timestamp(start_ms, true),
            timestamp(end_ms, true)
        ),
        duration_ms: end_ms - start_ms,
        lines: meeting
            .lines
            .iter()
            .filter(|line| line.at_ms < end_ms && line.end_ms > start_ms)
            .map(|line| ExportLine {
                at_ms: line.at_ms.max(start_ms) - start_ms,
                end_ms: line.end_ms.min(end_ms) - start_ms,
                speaker: line.speaker.clone(),
                text: line.text.clone(),
            })
            .collect(),
        notes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting() -> ExportMeeting {
        ExportMeeting {
            title: "Review".into(),
            started: "Today".into(),
            duration_ms: 4_000,
            lines: vec![
                ExportLine {
                    at_ms: 0,
                    end_ms: 1_100,
                    speaker: "Me".into(),
                    text: "First".into(),
                },
                ExportLine {
                    at_ms: 1_001,
                    end_ms: 5_000,
                    speaker: "Dana".into(),
                    text: "Second".into(),
                },
            ],
            notes: None,
        }
    }

    #[test]
    fn exact_subtitle_cues_preserve_overlap_and_clip_to_meeting_end() {
        assert_eq!(to_srt(&meeting()), "1\n00:00:00,000 --> 00:00:01,100\nMe: First\n\n2\n00:00:01,001 --> 00:00:04,000\nDana: Second\n\n");
        assert_eq!(to_vtt(&meeting()), "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.100\nMe: First\n\n2\n00:00:01.001 --> 00:00:04.000\nDana: Second\n\n");
        assert_eq!(timestamp(360_001_002, false), "100:00:01,002");
    }

    #[test]
    fn subtitle_content_cannot_inject_cues_or_markup() {
        let mut m = meeting();
        m.lines[0].text = "<b>A & B</b>\n\n00:00 --> injected".into();
        let rendered = to_vtt(&m);
        assert!(rendered.contains("&lt;b&gt;A &amp; B&lt;/b&gt; 00:00 --&gt; injected"));
        assert_eq!(rendered.matches(" --> ").count(), 2);
        m.lines[0].end_ms = 0;
        m.lines[1].text = " \n".into();
        assert_eq!(to_vtt(&m), "WEBVTT\n\n");
    }

    #[test]
    fn excerpt_includes_boundary_speech_rebases_and_drops_other_content() {
        let clipped = excerpt(&meeting(), 1_050, 2_000).unwrap();
        assert_eq!(clipped.duration_ms, 950);
        assert_eq!(clipped.lines.len(), 2);
        assert_eq!((clipped.lines[0].at_ms, clipped.lines[0].end_ms), (0, 50));
        assert_eq!((clipped.lines[1].at_ms, clipped.lines[1].end_ms), (0, 950));
        assert!(clipped.notes.is_none());
        assert_eq!(excerpt(&meeting(), 1_100, 2_000).unwrap().lines.len(), 1);
        assert!(excerpt(&meeting(), 1_000, 1_000).is_err());
        assert!(excerpt(&meeting(), 0, 4_001).is_err());
    }
}
