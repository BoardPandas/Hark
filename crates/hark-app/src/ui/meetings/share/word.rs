//! Text-only Word export. Document construction and packing stay on the worker.
//! The one format that carries speaker color: each speaker's name is a bold
//! run in its Light-palette ink, since documents are white whatever the
//! app's theme.
//!
//! The package is written by hand rather than through a docx library: the
//! export needs one paragraph shape, two styles and five parts, and the last
//! library used here (docx-rs) pinned a zip release with open advisories and
//! had no release that moved off it. Each part is the minimal WordprocessingML
//! that Word, LibreOffice, Google Docs and Pages all open.

use std::io::Write;

use crate::theme;
use hark_meeting::export::{self, ExportLine, ExportMeeting};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub(super) fn save(meeting: ExportMeeting) -> String {
    let name = export::safe_file_name(&meeting.title, "docx");
    let Some(path) = super::files::ask_path(&name, "Word document", "docx") else {
        return "Save cancelled.".into();
    };
    let bytes = match pack(&document(&meeting)) {
        Ok(bytes) => bytes,
        Err(error) => return format!("Could not create the Word document: {error}"),
    };
    match super::files::write_file(&path, &bytes) {
        Ok(()) => format!("Saved to {}.", path.display()),
        Err(error) => format!("Could not save the Word document: {error}"),
    }
}

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
    "</Types>",
);

const PACKAGE_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
    "</Relationships>",
);

const DOCUMENT_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    "</Relationships>",
);

// Child elements follow the schema's sequence order (CT_Style, CT_RPr):
// Word repairs or rejects out-of-order children, where other readers shrug.
const STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal">"#,
    r#"<w:name w:val="Normal"/><w:qFormat/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Title">"#,
    r#"<w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/>"#,
    r#"<w:rPr><w:b/><w:bCs/><w:sz w:val="40"/><w:szCs w:val="40"/></w:rPr></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading1">"#,
    r#"<w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/>"#,
    r#"<w:pPr><w:outlineLvl w:val="0"/></w:pPr>"#,
    r#"<w:rPr><w:b/><w:bCs/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style>"#,
    "</w:styles>",
);

// A4 with the margins the export has always used.
const SECTION: &str = concat!(
    r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
    r#"<w:pgMar w:top="1985" w:right="1701" w:bottom="1701" w:left="1701" w:header="851" w:footer="992" w:gutter="0"/>"#,
    r#"<w:cols w:space="425"/></w:sectPr>"#,
);

/// Zip the parts into a .docx. `[Content_Types].xml` goes first by
/// convention: some readers sniff the first entry to recognise the package.
fn pack(document_xml: &str) -> zip::result::ZipResult<Vec<u8>> {
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", PACKAGE_RELS),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS),
        ("word/styles.xml", STYLES),
        ("word/document.xml", document_xml),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(body.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

/// Escape for both element text and attribute values. Characters XML 1.0
/// cannot carry at all (controls other than tab, and the two noncharacters)
/// are dropped: Word refuses the whole file over a single one.
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push('\t'),
            '\u{FFFE}' | '\u{FFFF}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
}

/// Run properties: bold and/or a hex ink (no `#`).
#[derive(Default, Clone, Copy)]
struct Format<'a> {
    bold: bool,
    color: Option<&'a str>,
}

/// One run; each newline in `text` becomes a soft line break.
fn run(out: &mut String, text: &str, format: Format) {
    out.push_str("<w:r>");
    if format.bold || format.color.is_some() {
        out.push_str("<w:rPr>");
        if format.bold {
            out.push_str("<w:b/><w:bCs/>");
        }
        if let Some(hex) = format.color {
            out.push_str(r#"<w:color w:val=""#);
            escape_into(out, hex);
            out.push_str(r#""/>"#);
        }
        out.push_str("</w:rPr>");
    }
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push_str("<w:br/>");
        }
        out.push_str(r#"<w:t xml:space="preserve">"#);
        escape_into(out, line);
        out.push_str("</w:t>");
    }
    out.push_str("</w:r>");
}

fn paragraph(out: &mut String, style: Option<&str>, runs: &[(&str, Format)]) {
    out.push_str("<w:p>");
    if let Some(style) = style {
        out.push_str(r#"<w:pPr><w:pStyle w:val=""#);
        escape_into(out, style);
        out.push_str(r#""/></w:pPr>"#);
    }
    for (text, format) in runs {
        run(out, text, *format);
    }
    out.push_str("</w:p>");
}

fn plain(out: &mut String, text: &str) {
    paragraph(out, None, &[(text, Format::default())]);
}

fn heading(out: &mut String, text: &str) {
    paragraph(out, Some("Heading1"), &[(text, Format::default())]);
}

/// `[mm:ss] Name: text`, with the name bold in the speaker's document ink.
fn transcript_line(out: &mut String, line: &ExportLine) {
    let ink = theme::document_hue(theme::speaker_hue(line.slot));
    let hex = format!("{:02x}{:02x}{:02x}", ink.r(), ink.g(), ink.b());
    let stamp = format!("[{}] ", export::format_timestamp(line.at_ms));
    let text = format!(": {}", line.text);
    let name = Format {
        bold: true,
        color: Some(&hex),
    };
    paragraph(
        out,
        None,
        &[
            (&stamp, Format::default()),
            (&line.speaker, name),
            (&text, Format::default()),
        ],
    );
}

/// `word/document.xml` for the meeting.
fn document(meeting: &ExportMeeting) -> String {
    let mut out = String::with_capacity(4096 + meeting.lines.len() * 256);
    out.push_str(DECLARATION);
    out.push_str(r#"<w:document xmlns:w=""#);
    out.push_str(W_NS);
    out.push_str(r#""><w:body>"#);

    paragraph(
        &mut out,
        Some("Title"),
        &[(&meeting.title, Format::default())],
    );
    plain(
        &mut out,
        &format!(
            "{} · {}",
            meeting.started,
            export::format_duration(meeting.duration_ms)
        ),
    );
    if let Some(notes) = &meeting.notes {
        if !notes.summary.trim().is_empty() {
            heading(&mut out, "Summary");
            plain(&mut out, &notes.summary);
        }
        for (label, lines) in [
            ("Key points", &notes.key_points),
            ("Decisions", &notes.decisions),
        ] {
            if !lines.is_empty() {
                heading(&mut out, label);
            }
            for line in lines {
                plain(&mut out, &format!("• {line}"));
            }
        }
        if !notes.action_items.is_empty() {
            heading(&mut out, "Action items");
        }
        for action in &notes.action_items {
            let mark = if action.done { "☑" } else { "☐" };
            let owner = action
                .owner
                .as_deref()
                .map(|name| format!(" ({name})"))
                .unwrap_or_default();
            plain(&mut out, &format!("{mark} {}{owner}", action.text));
        }
    }
    heading(&mut out, "Transcript");
    if meeting.lines.is_empty() {
        plain(&mut out, "No transcript.");
    }
    for line in &meeting.lines {
        transcript_line(&mut out, line);
    }

    out.push_str(SECTION);
    out.push_str("</w:body></w:document>");
    out
}

/// The built document's body XML, for the export tests.
#[cfg(test)]
pub(super) fn document_xml(meeting: &ExportMeeting) -> String {
    document(meeting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hark_meeting::export::ExportLine;
    use std::io::Read;

    fn meeting() -> ExportMeeting {
        ExportMeeting {
            title: "A & B <review>".into(),
            started: "Today".into(),
            duration_ms: 1000,
            lines: vec![ExportLine {
                at_ms: 0,
                end_ms: 1000,
                speaker: "Zoë \"Z\"".into(),
                slot: 0,
                text: "alpha\nbeta\0\u{1b}\u{FFFF}".into(),
            }],
            notes: None,
        }
    }

    /// Fails on any malformed XML, including a mismatched end tag.
    fn assert_well_formed(name: &str, xml: &str) {
        let mut reader = quick_xml::Reader::from_str(xml);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(error) => panic!("{name} is not well-formed XML: {error}\n{xml}"),
            }
        }
    }

    #[test]
    fn word_escapes_content_and_preserves_unicode_and_line_breaks() {
        let xml = document(&meeting());
        assert!(STYLES.contains(r#"w:styleId="Title""#));
        assert!(STYLES.contains(r#"w:styleId="Heading1""#));
        assert!(xml.contains("A &amp; B &lt;review&gt;"));
        assert!(xml.contains("Zoë &quot;Z&quot;"));
        assert!(xml.contains("<w:br/>"));
        // Me's name is bold in Light Blue ink, the table the hue tests cover.
        let blue = theme::document_hue(theme::Hue::Blue);
        let hex = format!("{:02x}{:02x}{:02x}", blue.r(), blue.g(), blue.b());
        assert!(
            xml.contains(&format!(r#"<w:color w:val="{hex}"/>"#)),
            "{xml}"
        );
        assert!(xml.contains("<w:b/>"), "{xml}");
        for banned in ['\0', '\u{1b}', '\u{FFFF}'] {
            assert!(!xml.contains(banned), "{banned:?} reached the XML");
        }
        assert_well_formed("word/document.xml", &xml);
    }

    /// The package a reader actually opens: every part present and
    /// well-formed, every content-type override and relationship pointing at
    /// a part that exists, `[Content_Types].xml` first.
    #[test]
    fn word_package_is_complete_and_self_consistent() {
        let bytes = pack(&document(&meeting())).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(
            archive.by_index(0).unwrap().name().unwrap(),
            "[Content_Types].xml"
        );

        let mut parts = std::collections::HashMap::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let mut text = String::new();
            file.read_to_string(&mut text).unwrap();
            parts.insert(file.name().unwrap().into_owned(), text);
        }
        let mut names: Vec<&str> = parts.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "word/_rels/document.xml.rels",
                "word/document.xml",
                "word/styles.xml",
            ]
        );
        for (name, xml) in &parts {
            assert_well_formed(name, xml);
        }
        for part in ["/word/document.xml", "/word/styles.xml"] {
            assert!(
                parts["[Content_Types].xml"].contains(&format!(r#"PartName="{part}""#)),
                "{part} has no content type"
            );
        }
        assert!(parts["_rels/.rels"].contains(r#"Target="word/document.xml""#));
        assert!(parts["word/_rels/document.xml.rels"].contains(r#"Target="styles.xml""#));
        assert!(parts["word/document.xml"].contains(r#"<w:pStyle w:val="Title"/>"#));
    }
}
