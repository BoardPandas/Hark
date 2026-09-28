//! Select an audio range on the UI thread; perform dialogs/encoding on a worker.

use hark_meeting::export::{self, ExportMeeting, ExportOptions};

pub(super) struct ExcerptDialog {
    id: String,
    export: ExportMeeting,
    start: String,
    end: String,
    mp3: bool,
    first_line: usize,
    last_line: usize,
}

pub(super) enum DialogAction {
    Keep,
    Cancel,
    Save { mp3: bool },
}

impl ExcerptDialog {
    pub fn new(id: &str, export: ExportMeeting) -> Self {
        let end = time_input(export.duration_ms);
        let last_line = export.lines.len();
        Self {
            id: id.to_string(),
            export,
            start: "00:00.000".into(),
            end,
            mp3: true,
            first_line: 1,
            last_line,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DialogAction {
        let mut open = true;
        let mut action = DialogAction::Keep;
        egui::Window::new("Save an excerpt with audio").id(egui::Id::new("meeting-excerpt"))
            .collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.set_max_width(crate::theme::DIALOG_WIDTH);
                ui.label(&self.export.title);
                if !self.export.lines.is_empty() {
                    ui.label("Select transcript lines:");
                    let count = self.export.lines.len();
                    ui.horizontal(|ui| {
                        ui.label("First line");
                        ui.add(egui::DragValue::new(&mut self.first_line).range(1..=count));
                        ui.label("Last line");
                        ui.add(egui::DragValue::new(&mut self.last_line).range(1..=count));
                    });
                    let selected = line_range(&self.export, self.first_line, self.last_line);
                    if ui.add_enabled(selected.is_some(), egui::Button::new("Use selected lines")).clicked() {
                        if let Some((start, end)) = selected {
                            self.start = time_input(start);
                            self.end = time_input(end);
                        }
                    }
                    if selected.is_some() {
                        for index in [self.first_line, self.last_line].into_iter().take(if self.first_line == self.last_line { 1 } else { 2 }) {
                            let line = &self.export.lines[index - 1];
                            let preview: String = line.text.chars().take(180).collect();
                            let ellipsis = if line.text.chars().count() > 180 { "…" } else { "" };
                            ui.label(format!("{index}. [{}] {}: {preview}{ellipsis}", export::format_timestamp(line.at_ms), line.speaker));
                        }
                    }
                    ui.separator();
                }
                ui.label("Fine-tune the range (minutes:seconds, or hours:minutes:seconds).");
                ui.horizontal(|ui| { ui.label("Start"); ui.text_edit_singleline(&mut self.start); });
                ui.horizontal(|ui| { ui.label("End"); ui.text_edit_singleline(&mut self.end); });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.mp3, true, "MP3 (smaller file)");
                    ui.radio_value(&mut self.mp3, false, "WAV (no extra compression)");
                });
                ui.label("Saves the selected audio and matching transcript as two files.\nTranscript lines crossing a boundary are included in full; notes are omitted.");
                let range = self.range();
                if let Err(error) = &range { ui.label(*error); }
                ui.horizontal(|ui| {
                    if ui.add_enabled(range.is_ok(), egui::Button::new("Save excerpt…")).clicked() {
                        action = DialogAction::Save { mp3: self.mp3 };
                    }
                    if ui.button("Cancel").clicked() { action = DialogAction::Cancel; }
                });
            });
        if !open {
            DialogAction::Cancel
        } else {
            action
        }
    }

    fn range(&self) -> Result<(u64, u64), &'static str> {
        let (Some(start), Some(end)) = (parse_time(&self.start), parse_time(&self.end)) else {
            return Err("Use a time such as 01:23.500.");
        };
        if start >= end || end > self.export.duration_ms {
            return Err("Choose an end after the start, within the recording.");
        }
        Ok((start, end))
    }

    pub fn save(self, mp3: bool) -> String {
        let Ok((start, end)) = self.range() else {
            return "The excerpt range is invalid.".into();
        };
        let Some(source) = super::files::meeting_dir(&self.id)
            .as_deref()
            .and_then(hark_audio::meeting_audio)
        else {
            return "This meeting's audio is no longer on this device.".into();
        };
        let (filter, ext) = if mp3 {
            ("MP3 audio", "mp3")
        } else {
            ("WAV audio", "wav")
        };
        let name = export::safe_file_name(&format!("{} excerpt", self.export.title), ext);
        let Some(path) = super::files::ask_path(&name, filter, ext) else {
            return "Save cancelled.".into();
        };
        if !path
            .extension()
            .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(ext))
        {
            return format!("Choose a file name ending in .{ext} for the excerpt audio.");
        }
        if let Err(error) = super::files::check_export_path(&path) {
            return format!("Could not save: {error}");
        }
        let text_path = path.with_extension("txt");
        // The save dialog authorizes replacing the audio, not an unrelated
        // existing .txt next to it. Reserve the companion with create_new.
        let mut text_file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&text_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return "A transcript with that name already exists. Choose another excerpt name."
                    .into()
            }
            Err(error) => return format!("Could not create the excerpt transcript: {error}"),
        };
        let result = if mp3 {
            hark_audio::mp3::export_excerpt_mp3(&source, &path, start, end)
        } else {
            hark_audio::mp3::export_excerpt_wav(&source, &path, start, end)
        };
        let frames = match result {
            Ok(frames) => frames,
            Err(error) => {
                drop(text_file);
                let _ = std::fs::remove_file(&text_path);
                return format!("Could not save the excerpt audio: {error}");
            }
        };
        let actual_end = end.min(start + frames * 1000 / u64::from(hark_meeting::SAMPLE_RATE));
        let selected = match export::excerpt(&self.export, start, actual_end) {
            Ok(selected) => selected,
            Err(error) => {
                drop(text_file);
                let _ = std::fs::remove_file(&text_path);
                return format!(
                    "Audio saved to {}; transcript unavailable: {error}",
                    path.display()
                );
            }
        };
        use std::io::Write;
        let body = export::to_text(
            &selected,
            ExportOptions {
                notes: false,
                ..ExportOptions::default()
            },
        );
        let write = text_file
            .write_all(body.as_bytes())
            .and_then(|()| text_file.sync_all());
        drop(text_file);
        match write {
            Ok(()) => format!("Saved {} and {}.", path.display(), text_path.display()),
            Err(error) => {
                let _ = std::fs::remove_file(&text_path);
                format!(
                    "Audio saved to {}; could not save the transcript: {error}",
                    path.display()
                )
            }
        }
    }
}

fn line_range(export: &ExportMeeting, first: usize, last: usize) -> Option<(u64, u64)> {
    if first == 0 || first > last {
        return None;
    }
    let lines = export.lines.get(first - 1..last)?;
    let start = lines.iter().map(|line| line.at_ms).min()?;
    let end = lines
        .iter()
        .map(|line| line.end_ms)
        .max()?
        .min(export.duration_ms);
    (end > start).then_some((start, end))
}

fn time_input(ms: u64) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn parse_time(text: &str) -> Option<u64> {
    let parts: Vec<_> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let (whole, fraction) = parts.last()?.split_once('.').unwrap_or((parts.last()?, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    if !digits(whole) || (!fraction.is_empty() && (!digits(fraction) || fraction.len() > 3)) {
        return None;
    }
    let seconds: u64 = whole.parse().ok()?;
    if parts.len() > 1 && seconds >= 60 {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()? * 10_u64.pow(3 - fraction.len() as u32)
    };
    let mut total = 0_u64;
    for (index, part) in parts[..parts.len() - 1].iter().enumerate() {
        if !digits(part) {
            return None;
        }
        let number: u64 = part.parse().ok()?;
        if parts.len() == 3 && index == 1 && number >= 60 {
            return None;
        }
        total = total.checked_mul(60)?.checked_add(number)?;
    }
    total
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(1000)?
        .checked_add(millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_lines_include_their_whole_time_span_and_validate_indices() {
        use hark_meeting::export::ExportLine;
        let meeting = ExportMeeting {
            title: "Fixture".into(),
            started: "Today".into(),
            duration_ms: 9000,
            lines: vec![
                ExportLine {
                    at_ms: 100,
                    end_ms: 1000,
                    speaker: "Me".into(),
                    text: "One".into(),
                },
                ExportLine {
                    at_ms: 900,
                    end_ms: 3000,
                    speaker: "Them".into(),
                    text: "Two".into(),
                },
                ExportLine {
                    at_ms: 5000,
                    end_ms: 10000,
                    speaker: "Me".into(),
                    text: "Three".into(),
                },
            ],
            notes: None,
        };
        assert_eq!(line_range(&meeting, 1, 2), Some((100, 3000)));
        assert_eq!(line_range(&meeting, 2, 3), Some((900, 9000)));
        for (first, last) in [(0, 1), (2, 1), (1, 4), (4, 4)] {
            assert_eq!(line_range(&meeting, first, last), None);
        }
    }
    #[test]
    fn times_parse_without_rounding_or_ambiguous_overflows() {
        for (text, expected) in [
            ("0", 0),
            ("1.2", 1200),
            ("01:23.456", 83_456),
            ("1:02:03.004", 3_723_004),
            ("65:00", 3_900_000),
        ] {
            assert_eq!(parse_time(text), Some(expected));
            assert_eq!(parse_time(&time_input(expected)), Some(expected));
        }
        for text in [
            "-1",
            "NaN",
            "1:60",
            "1:60:00",
            "1:2:3:4",
            "1.0001",
            "18446744073709551615",
            "1::2",
        ] {
            assert_eq!(parse_time(text), None, "{text}");
        }
    }
}
