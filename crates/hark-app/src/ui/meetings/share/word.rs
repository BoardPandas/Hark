//! Text-only Word export. Document construction and packing stay on the worker.
//! The one format that carries speaker color: each speaker's name is a bold
//! run in its Light-palette ink, since documents are white whatever the
//! app's theme.

use crate::theme;
use docx_rs::{BreakType, Docx, Paragraph, Run, Style, StyleType};
use hark_meeting::export::{self, ExportLine, ExportMeeting};

pub(super) fn save(meeting: ExportMeeting) -> String {
    let name = export::safe_file_name(&meeting.title, "docx");
    let Some(path) = super::files::ask_path(&name, "Word document", "docx") else {
        return "Save cancelled.".into();
    };
    let mut bytes = std::io::Cursor::new(Vec::new());
    if let Err(error) = document(&meeting).build().pack(&mut bytes) {
        return format!("Could not create the Word document: {error}");
    }
    match super::files::write_file(&path, &bytes.into_inner()) {
        Ok(()) => format!("Saved to {}.", path.display()),
        Err(error) => format!("Could not save the Word document: {error}"),
    }
}

fn paragraph(text: &str) -> Paragraph {
    Paragraph::new().add_run(run(text))
}

fn run(text: &str) -> Run {
    let mut run = Run::new();
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            run = run.add_break(BreakType::TextWrapping);
        }
        let clean: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        run = run.add_text(clean);
    }
    run
}

/// `[mm:ss] Name: text`, with the name bold in the speaker's document ink.
fn transcript_line(line: &ExportLine) -> Paragraph {
    let ink = theme::document_hue(theme::speaker_hue(line.slot));
    let hex = format!("{:02x}{:02x}{:02x}", ink.r(), ink.g(), ink.b());
    Paragraph::new()
        .add_run(run(&format!("[{}] ", export::format_timestamp(line.at_ms))))
        .add_run(run(&line.speaker).bold().color(hex))
        .add_run(run(&format!(": {}", line.text)))
}

fn heading(text: &str) -> Paragraph {
    paragraph(text).style("Heading1")
}

fn document(meeting: &ExportMeeting) -> Docx {
    let mut doc = Docx::new()
        .add_style(
            Style::new("Title", StyleType::Paragraph)
                .name("Title")
                .based_on("Normal")
                .next("Normal")
                .size(40)
                .bold(),
        )
        .add_style(
            Style::new("Heading1", StyleType::Paragraph)
                .name("heading 1")
                .based_on("Normal")
                .next("Normal")
                .size(28)
                .bold()
                .outline_lvl(0),
        )
        .add_paragraph(paragraph(&meeting.title).style("Title"))
        .add_paragraph(paragraph(&format!(
            "{} · {}",
            meeting.started,
            export::format_duration(meeting.duration_ms)
        )));
    if let Some(notes) = &meeting.notes {
        if !notes.summary.trim().is_empty() {
            doc = doc
                .add_paragraph(heading("Summary"))
                .add_paragraph(paragraph(&notes.summary));
        }
        for (label, lines) in [
            ("Key points", &notes.key_points),
            ("Decisions", &notes.decisions),
        ] {
            if !lines.is_empty() {
                doc = doc.add_paragraph(heading(label));
            }
            for line in lines {
                doc = doc.add_paragraph(paragraph(&format!("• {line}")));
            }
        }
        if !notes.action_items.is_empty() {
            doc = doc.add_paragraph(heading("Action items"));
        }
        for action in &notes.action_items {
            let mark = if action.done { "☑" } else { "☐" };
            let owner = action
                .owner
                .as_deref()
                .map(|name| format!(" ({name})"))
                .unwrap_or_default();
            doc = doc.add_paragraph(paragraph(&format!("{mark} {}{owner}", action.text)));
        }
    }
    doc = doc.add_paragraph(heading("Transcript"));
    if meeting.lines.is_empty() {
        doc = doc.add_paragraph(paragraph("No transcript."));
    }
    for line in &meeting.lines {
        doc = doc.add_paragraph(transcript_line(line));
    }
    doc
}

/// The built document's body XML, for the export tests.
#[cfg(test)]
pub(super) fn document_xml(meeting: &ExportMeeting) -> String {
    String::from_utf8(document(meeting).build().document).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_meeting::export::ExportLine;

    #[test]
    fn word_escapes_content_and_preserves_unicode_and_line_breaks() {
        let meeting = ExportMeeting {
            title: "A & B <review>".into(),
            started: "Today".into(),
            duration_ms: 1000,
            lines: vec![ExportLine {
                at_ms: 0,
                end_ms: 1000,
                speaker: "Zoë".into(),
                slot: 0,
                text: "alpha\nbeta\0".into(),
            }],
            notes: None,
        };
        let built = document(&meeting).build();
        let xml = String::from_utf8(built.document.clone()).unwrap();
        let styles = String::from_utf8(built.styles.clone()).unwrap();
        assert!(styles.contains("w:styleId=\"Title\""));
        assert!(styles.contains("w:styleId=\"Heading1\""));
        assert!(xml.contains("A &amp; B &lt;review&gt;"));
        assert!(xml.contains("Zoë"));
        assert!(xml.contains("w:br"));
        // Me's name is bold in Light Blue ink, the table the hue tests cover.
        let blue = theme::document_hue(theme::Hue::Blue);
        let hex = format!("{:02x}{:02x}{:02x}", blue.r(), blue.g(), blue.b());
        assert!(xml.contains(&format!("w:color w:val=\"{hex}\"")), "{xml}");
        assert!(xml.contains("<w:b />") || xml.contains("<w:b/>"), "{xml}");
        assert!(!xml.contains('\0'));
        let mut bytes = std::io::Cursor::new(Vec::new());
        built.pack(&mut bytes).unwrap();
        assert!(docx_rs::read_docx(&bytes.into_inner()).is_ok());
    }
}
