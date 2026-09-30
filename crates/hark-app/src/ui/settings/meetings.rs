//! Settings > Meetings (plan §4.7): detection, capture, speaker labels (the
//! Deepgram key, independent of the dictation provider), and the audio
//! storage cap. Edits go into the draft like every other section; the
//! Deepgram key writes straight to the keychain like the STT key does.

use super::keys::KeySection;
use crate::meeting::{MeetingController, MeetingStatus};
use crate::storage::meetings::MeetingCmd;
use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::widgets;
use egui::{RichText, Ui};
use hark_config::{AutoDetect, FinalPass, Settings, SystemSource};
use hark_meeting::storage::MB;
use hark_meeting::{detect, plan_eviction, storage_fs, StoredAudio};

pub struct MeetingsSettings {
    deepgram: KeySection,
    gemini: KeySection,
    /// One app id per line, mirrored into `detect_apps`.
    apps: String,
    toggle_key: String,
    /// On-disk usage, re-measured when the database changes.
    usage: Option<(u64, Vec<StoredAudio>)>,
    confirm: Option<widgets::Confirm>,
}

impl MeetingsSettings {
    pub fn new(settings: &Settings) -> Self {
        MeetingsSettings {
            deepgram: KeySection::new(
                "meetings-deepgram",
                hark_pipeline::meeting::DEEPGRAM_ACCOUNT,
            ),
            gemini: KeySection::new("meetings-gemini", "gemini"),
            apps: apps_text(settings),
            toggle_key: settings.meeting.toggle_key.clone().unwrap_or_default(),
            usage: None,
            confirm: None,
        }
    }

    /// Re-seed buffers from the saved model (after Discard).
    pub fn reset(&mut self, settings: &Settings) {
        self.apps = apps_text(settings);
        self.toggle_key = settings.meeting.toggle_key.clone().unwrap_or_default();
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        draft: &mut Settings,
        saved: &Settings,
        mic_devices: &[String],
        meetings: &MeetingController,
        storage: Option<&StorageHandle>,
    ) {
        theme::card(ui, |ui| self.general(ui, draft, mic_devices));
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| self.shortcut(ui, draft));
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| self.detection(ui, draft));
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| self.speakers(ui, draft));
        ui.add_space(theme::SECTION_GAP);
        theme::card(ui, |ui| self.storage(ui, draft, saved, meetings, storage));
    }

    fn shortcut(&mut self, ui: &mut Ui, draft: &mut Settings) {
        ui.label(RichText::new("Start / stop shortcut").text_style(theme::subheading()));
        ui.add_enabled_ui(hark_pipeline::meeting::meetings_supported(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.toggle_key)
                            .hint_text("Unassigned, e.g. LCtrl+F11")
                            .desired_width(250.0),
                    )
                    .changed()
                {
                    draft.meeting.toggle_key = optional_shortcut(&self.toggle_key);
                }
                if draft.meeting.toggle_key.is_some() && ui.small_button("Clear").clicked() {
                    self.toggle_key.clear();
                    draft.meeting.toggle_key = None;
                }
            });
        });
        ui.label(RichText::new("Press once to start meeting notes, then again to stop. Leave blank to use the buttons. Use key names such as LCtrl, LAlt, LShift, F11, separated by +.").small().weak());
        if let Err(error) = draft.validate_meeting_shortcut() {
            ui.label(RichText::new(error.to_string()).color(theme::danger(ui.visuals())));
        } else if let Some(chord) = draft
            .meeting
            .toggle_key
            .as_deref()
            .and_then(|s| hark_hotkey::PttChord::parse(s).ok())
        {
            if let Some(why) = chord.rejection() {
                ui.label(RichText::new(why.message()).color(theme::warning(ui.visuals())));
            }
        }
    }

    fn general(&mut self, ui: &mut Ui, draft: &mut Settings, mic_devices: &[String]) {
        let m = &mut draft.meeting;
        ui.label(RichText::new("Meeting notes").text_style(theme::subheading()));
        ui.checkbox(&mut m.enabled, "Take meeting notes")
            .on_hover_text(
                "Records your microphone and the call's audio, with no bot joining the call.",
            );
        ui.checkbox(
            &mut m.live_transcript,
            "Show a live transcript during the call",
        );
        ui.checkbox(
            &mut m.summary,
            "Write notes after the call (uses your text provider)",
        );
        ui.checkbox(
            &mut m.consent_reminder,
            "Remind me that others may need to consent",
        );
        ui.add_space(theme::GAP);
        ui.label(RichText::new("Microphone").strong());
        let current = m.mic_device.clone().unwrap_or_else(|| {
            if cfg!(windows) {
                "Windows communications default"
            } else {
                "System default microphone"
            }
            .to_string()
        });
        egui::ComboBox::from_id_salt("meeting-mic")
            .selected_text(current)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut m.mic_device,
                    None,
                    if cfg!(windows) {
                        "Windows communications default"
                    } else {
                        "System default microphone"
                    },
                );
                for name in mic_devices {
                    ui.selectable_value(&mut m.mic_device, Some(name.clone()), name);
                }
            });
        ui.label(
            RichText::new(if cfg!(windows) {
                "The communications default is the microphone Teams and Zoom use."
            } else {
                "Choose the same microphone your meeting app uses."
            })
            .small()
            .weak(),
        );
        ui.checkbox(&mut m.echo_cancellation, "Reduce speaker echo")
            .on_hover_text(
                "Uses captured system audio to reduce speaker sound in your microphone recording. \
                 Applies to the next meeting after saving. Leave off for headphones; turn it off \
                 if your voice sounds distorted.",
            );
        ui.add_space(theme::GAP);
        #[cfg(target_os = "macos")]
        {
            ui.label("macOS asks for system audio access when you first start meeting notes. Browser call detection also needs Screen Recording access to read the call window title.");
            crate::macos::settings_button(
                ui,
                "System audio & screen recording settings",
                "Privacy_ScreenCapture",
            );
        }
        ui.label(RichText::new("Other people's audio").strong());
        ui.radio_value(&mut m.system_source, SystemSource::App, "Only the meeting app's audio")
            .on_hover_text("A detected meeting records just that app. A meeting you start yourself records everything except Hark.");
        ui.radio_value(
            &mut m.system_source,
            SystemSource::All,
            "Everything playing, except Hark",
        );
    }

    fn detection(&mut self, ui: &mut Ui, draft: &mut Settings) {
        let m = &mut draft.meeting;
        ui.label(RichText::new("When a call starts").text_style(theme::subheading()));
        ui.radio_value(&mut m.auto_detect, AutoDetect::Ask, "Ask me");
        ui.radio_value(
            &mut m.auto_detect,
            AutoDetect::Auto,
            "Start taking notes on its own",
        );
        ui.radio_value(
            &mut m.auto_detect,
            AutoDetect::Off,
            "Do nothing (start by hand)",
        );
        ui.add_space(theme::GAP);
        ui.horizontal(|ui| {
            ui.label("Stop after the call has released the mic for");
            ui.add(
                egui::DragValue::new(&mut m.auto_stop_after_s)
                    .range(0..=3600)
                    .suffix(" s"),
            );
        });
        ui.label(RichText::new("0 = never stop on its own. A meeting you start by hand is never stopped automatically.").small().weak());
        ui.add_space(theme::GAP);
        ui.label(RichText::new("Meeting apps (one per line)").strong());
        if ui
            .add(
                egui::TextEdit::multiline(&mut self.apps)
                    .desired_rows(4)
                    .desired_width(320.0),
            )
            .changed()
        {
            m.detect_apps = apps_from_text(&self.apps);
        }
        if m.detect_apps.is_some() && ui.small_button("Reset to the built-in list").clicked() {
            m.detect_apps = None;
            self.apps = detect::DEFAULT_APPS.join("\n");
        }
        ui.label(RichText::new("Program names (zoom.exe) or packaged app ids. Browsers count only while a meeting tab is open.").small().weak());
    }

    fn speakers(&mut self, ui: &mut Ui, draft: &mut Settings) {
        let m = &mut draft.meeting;
        ui.label(RichText::new("Speaker labels").text_style(theme::subheading()));
        ui.radio_value(
            &mut m.final_pass,
            FinalPass::Deepgram,
            "Label each speaker with Deepgram after the call",
        );
        ui.radio_value(
            &mut m.final_pass,
            FinalPass::Gemini,
            "Use Gemini after the call (speaker labels per five-minute window)",
        );
        ui.radio_value(
            &mut m.final_pass,
            FinalPass::None,
            "Keep the Me / Them transcript only",
        );
        if m.final_pass == FinalPass::Gemini {
            self.gemini.show(ui);
            ui.horizontal(|ui| {
                ui.label("Gemini model");
                ui.text_edit_singleline(&mut m.gemini_model);
            });
            ui.label(RichText::new("Uploads each audio track in five-minute windows, then requests deletion. Speaker numbers restart in each window. Uses your Gemini key; provider charges apply. Live dictation settings stay separate.").small().weak());
        }
        if m.final_pass == FinalPass::Deepgram {
            ui.add_space(theme::GAP);
            self.deepgram.show(ui);
            ui.label(
                RichText::new(
                    "About $0.52 per meeting hour on Deepgram. This key is separate from your \
                     dictation provider. Without it, meetings keep the Me / Them transcript.",
                )
                .small()
                .weak(),
            );
        }
    }

    fn storage(
        &mut self,
        ui: &mut Ui,
        draft: &mut Settings,
        saved: &Settings,
        meetings: &MeetingController,
        storage: Option<&StorageHandle>,
    ) {
        ui.label(RichText::new("Recordings").text_style(theme::subheading()));
        let Some(storage) = storage else {
            ui.label(RichText::new("The local database is unavailable.").weak());
            return;
        };
        self.refresh_usage(storage);
        let recordings = self
            .usage
            .as_ref()
            .map(|(_, r)| r.as_slice())
            .unwrap_or(&[]);
        let used: u64 = recordings.iter().map(|r| r.bytes).sum();
        ui.label(format!(
            "{} of {} used by {} recordings",
            gb(used),
            gb(draft.meeting.audio_cap_bytes()),
            recordings.len()
        ));
        ui.horizontal(|ui| {
            ui.label("Keep up to");
            let mut cap_gb = draft.meeting.audio_cap_mb as f64 / 1024.0;
            if ui
                .add(
                    egui::DragValue::new(&mut cap_gb)
                        .range(0.0..=1024.0)
                        .speed(0.25)
                        .max_decimals(1)
                        .suffix(" GB"),
                )
                .changed()
            {
                draft.meeting.audio_cap_mb = (cap_gb * 1024.0).round() as u32;
            }
        });
        ui.label(
            RichText::new(
                "When full, the oldest meetings' audio is deleted to make room. Transcripts and \
                 notes are never deleted. 0 = don't keep audio after the notes are written.",
            )
            .small()
            .weak(),
        );
        ui.checkbox(
            &mut draft.meeting.compress_audio,
            "Compress kept recordings (about 29 MB per hour)",
        );

        // Lowering the cap deletes audio; say exactly what, before Save does it.
        if draft.meeting.audio_cap_mb < saved.meeting.audio_cap_mb {
            let protected = protected_ids(meetings);
            let protected: Vec<&str> = protected.iter().map(String::as_str).collect();
            let plan = plan_eviction(recordings, draft.meeting.audio_cap_bytes(), &protected);
            if !plan.evict.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "Saving removes audio from your {} oldest meeting{}. Transcripts stay.",
                        plan.evict.len(),
                        if plan.evict.len() == 1 { "" } else { "s" }
                    ))
                    .color(theme::warning(ui.visuals())),
                );
            }
        }

        ui.add_space(theme::GAP);
        ui.horizontal(|ui| {
            if ui.button("Open folder").clicked() {
                open_meetings_folder();
            }
            if ui
                .add_enabled(!recordings.is_empty(), egui::Button::new("Delete all meeting audio"))
                .clicked()
            {
                self.confirm = Some(widgets::Confirm::new(
                    "Delete all meeting audio?",
                    "Every recording's audio is removed from this device. Transcripts and notes stay.",
                    "Delete audio",
                ));
            }
        });
        if let Some(confirm) = &mut self.confirm {
            match confirm.show(ui, "meetings-delete-audio") {
                Some(true) => {
                    storage.send(StorageCmd::Meeting(MeetingCmd::DeleteAllAudio {
                        protected: protected_ids(meetings),
                    }));
                    self.confirm = None;
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
    }

    fn refresh_usage(&mut self, storage: &StorageHandle) {
        let generation = storage.generation();
        if self.usage.as_ref().is_some_and(|(g, _)| *g == generation) {
            return;
        }
        let recordings = measure(storage).unwrap_or_default();
        self.usage = Some((generation, recordings));
    }
}

/// Recording and finishing meetings: never deleted from here.
fn protected_ids(meetings: &MeetingController) -> Vec<String> {
    let mut ids = meetings.finishing().to_vec();
    if let MeetingStatus::Recording { id, .. } = meetings.status() {
        ids.push(id.clone());
    }
    ids
}

/// Audio on disk per known meeting (plan §4.9 rule 1: from the filesystem).
fn measure(storage: &StorageHandle) -> Option<Vec<StoredAudio>> {
    let dir = hark_config::default_data_dir()?.join("meetings");
    let index = storage.reader().meeting_audio_index().ok()?;
    let known: Vec<&str> = index.iter().map(|(id, _)| id.as_str()).collect();
    let usage = storage_fs::scan(&dir, &known).ok()?;
    Some(
        usage
            .meetings
            .into_iter()
            .map(|(id, bytes)| {
                let ended_ms = index
                    .iter()
                    .find(|(i, _)| *i == id)
                    .and_then(|(_, e)| e.map(|e| e.max(0) as u64));
                StoredAudio {
                    id,
                    bytes,
                    ended_ms,
                }
            })
            .collect(),
    )
}

fn gb(bytes: u64) -> String {
    let gb = bytes as f64 / (1024.0 * MB as f64);
    if gb >= 10.0 {
        format!("{gb:.0} GB")
    } else if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", bytes / MB)
    }
}

fn apps_text(settings: &Settings) -> String {
    match &settings.meeting.detect_apps {
        Some(apps) => apps.join("\n"),
        None => detect::DEFAULT_APPS.join("\n"),
    }
}

fn optional_shortcut(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The edited list; `None` when it is exactly the built-in one.
fn apps_from_text(text: &str) -> Option<Vec<String>> {
    let apps: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let builtin: Vec<String> = detect::DEFAULT_APPS.iter().map(|s| s.to_string()).collect();
    (apps != builtin).then_some(apps)
}

fn open_meetings_folder() {
    let Some(dir) = hark_config::default_data_dir().map(|d| d.join("meetings")) else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("cannot create the meetings folder: {e}");
        return;
    }
    #[cfg(windows)]
    open_in_explorer(&dir);
    #[cfg(target_os = "macos")]
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg(&dir)
        .spawn()
    {
        log::warn!("cannot open the meetings folder: {error}");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Err(e) = std::process::Command::new("xdg-open")
        .arg(&dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        log::warn!("cannot open the meetings folder: {e}");
    }
}

#[cfg(windows)]
fn open_in_explorer(dir: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Err(e) = std::process::Command::new("explorer.exe")
        .arg(dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        log::warn!("cannot open the meetings folder: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unedited_app_list_stays_the_built_in_one() {
        assert_eq!(apps_from_text(&detect::DEFAULT_APPS.join("\n")), None);
        assert_eq!(
            apps_from_text(" zoom.exe \n\n teams.exe"),
            Some(vec!["zoom.exe".to_string(), "teams.exe".to_string()])
        );
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(gb(500 * MB), "500 MB");
        assert_eq!(gb(5 * 1024 * MB), "5.0 GB");
        assert_eq!(gb(20 * 1024 * MB), "20 GB");
    }
}
