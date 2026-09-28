//! The Meetings page (plan §4.7): start/stop and the live transcript while a
//! meeting records, then the list of past meetings and their detail (notes,
//! transcript, speakers, sharing).
//!
//! Queries go through the UI's reader connection and are cached on the
//! storage write generation, like History, so idle frames never touch the
//! database. Every write goes to the storage worker.

mod detail;
mod share;

use crate::meeting::{MeetingController, MeetingStatus};
use crate::storage::StorageHandle;
use crate::theme;
use crate::ui::{format, widgets};
use egui::{RichText, ScrollArea, TextEdit, Ui};
use hark_config::Settings;
use hark_store::{MeetingSummary, StoreError};
use jiff::tz::TimeZone;

/// What the page asks of the app (settings live outside it).
pub enum PageIntent {
    OpenSettings,
    /// The first-run consent notice was acknowledged.
    AcknowledgeConsent,
}

/// Plan §4.7: an optional line the user can paste into the meeting chat.
const ANNOUNCEMENT: &str = "I'm using Hark to transcribe this meeting.";

pub struct MeetingsPage {
    search: String,
    list: Vec<MeetingSummary>,
    /// (generation, search) the list reflects.
    list_key: Option<(u64, String)>,
    fetch_error: Option<String>,
    selected: Option<String>,
    detail: detail::DetailView,
    tz: TimeZone,
}

impl MeetingsPage {
    pub fn new() -> Self {
        MeetingsPage {
            search: String::new(),
            list: Vec::new(),
            list_key: None,
            fetch_error: None,
            selected: None,
            detail: detail::DetailView::new(),
            tz: TimeZone::system(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        meetings: &mut MeetingController,
        settings: &Settings,
        storage: Option<&StorageHandle>,
        storage_error: Option<&str>,
    ) -> Option<PageIntent> {
        self.detail.poll();
        let mut intent = None;
        ScrollArea::vertical()
            .id_salt("meetings-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(theme::SHADOW_MARGIN)
                    .show(ui, |ui| {
                        intent = self.body(ui, meetings, settings, storage, storage_error);
                    });
            });
        intent
    }

    fn body(
        &mut self,
        ui: &mut Ui,
        meetings: &mut MeetingController,
        settings: &Settings,
        storage: Option<&StorageHandle>,
        storage_error: Option<&str>,
    ) -> Option<PageIntent> {
        let mut intent = None;
        if let Some(id) = self.selected.clone() {
            let Some(storage) = storage else {
                self.selected = None;
                return None;
            };
            if self.detail.show(ui, storage, &id, &self.tz, meetings) {
                self.selected = None;
            }
            return None;
        }

        if settings.meeting.consent_reminder && !settings.meeting.consent_acknowledged {
            intent = consent_notice(ui).or(intent);
            ui.add_space(theme::SECTION_GAP);
        }
        intent = self.control_card(ui, meetings).or(intent);
        ui.add_space(theme::SECTION_GAP);

        let Some(storage) = storage else {
            widgets::empty_state(
                ui,
                theme::icons::WARNING,
                "Meetings cannot be saved.",
                storage_error.unwrap_or("The local database could not be opened."),
            );
            return intent;
        };
        self.refresh(storage);
        self.list_view(ui, meetings);
        intent
    }

    /// Start/stop, the live pane, and notices.
    fn control_card(
        &mut self,
        ui: &mut Ui,
        meetings: &mut MeetingController,
    ) -> Option<PageIntent> {
        let mut intent = None;
        theme::card(ui, |ui| match meetings.status().clone() {
            MeetingStatus::Unavailable(why) => {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::icon_text(theme::icons::WARNING).color(theme::warning(ui.visuals())),
                    );
                    ui.label(why);
                });
                if ui.button("Meeting settings").clicked() {
                    intent = Some(PageIntent::OpenSettings);
                }
            }
            MeetingStatus::Idle => {
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::primary_button(ui.visuals(), "Start meeting notes"))
                        .clicked()
                    {
                        meetings.start_manual();
                    }
                    ui.label(
                        RichText::new(
                            "Or let Hark notice when a call starts (Settings > Meetings).",
                        )
                        .small()
                        .weak(),
                    );
                });
                ui.label(
                    RichText::new(
                        "Use headphones: on speakers, the microphone also picks up the other side.",
                    )
                    .small()
                    .weak(),
                );
            }
            MeetingStatus::Recording {
                started_ms,
                system_audio,
                ..
            } => {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::icon_text(theme::icons::MICROPHONE)
                            .color(theme::danger(ui.visuals())),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Taking meeting notes since {}",
                            local_clock(started_ms, &self.tz)
                        ))
                        .strong(),
                    );
                    if ui.button("Stop").clicked() {
                        meetings.stop();
                    }
                });
                if let Some((at_ms, app)) = meetings.auto_stop() {
                    let app = app.to_string();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            theme::icon_text(theme::icons::CLOCK)
                                .color(theme::warning(ui.visuals())),
                        );
                        ui.label(format!(
                            "{app} ended the call. Notes stop at {}.",
                            local_clock_seconds(at_ms, &self.tz)
                        ));
                        if ui
                            .add(theme::primary_button(ui.visuals(), "Stop now"))
                            .clicked()
                        {
                            meetings.stop();
                        }
                    });
                }
                if !system_audio {
                    ui.label(
                        RichText::new("Only your microphone is being recorded.")
                            .small()
                            .color(theme::warning(ui.visuals())),
                    );
                }
                live_pane(ui, meetings);
            }
        });
        if !meetings.finishing().is_empty() {
            ui.add_space(theme::GAP);
            ui.horizontal(|ui| {
                ui.label(
                    theme::icon_text(theme::icons::SPINNER).color(theme::accent(ui.visuals())),
                );
                ui.label(
                    RichText::new("Finishing the last meeting: speaker labels, notes, archive.")
                        .small()
                        .weak(),
                );
            });
        }
        if !meetings.notices().is_empty() {
            ui.add_space(theme::GAP);
            let mut dismiss = false;
            theme::card(ui, |ui| {
                for notice in meetings.notices() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            theme::icon_text(theme::icons::WARNING)
                                .color(theme::warning(ui.visuals())),
                        );
                        ui.label(RichText::new(notice).small());
                    });
                }
                dismiss = ui.small_button("Dismiss").clicked();
            });
            if dismiss {
                meetings.dismiss_notices();
            }
        }
        intent
    }

    fn refresh(&mut self, storage: &StorageHandle) {
        let key = (storage.generation(), self.search.trim().to_string());
        if self.list_key.as_ref() == Some(&key) {
            return;
        }
        let search = (!key.1.is_empty()).then_some(key.1.as_str());
        match storage.reader().meetings(search) {
            Ok(list) => {
                self.list = list;
                self.fetch_error = None;
            }
            Err(e) => self.fetch_error = Some(describe(&e)),
        }
        self.list_key = Some(key);
    }

    fn list_view(&mut self, ui: &mut Ui, meetings: &MeetingController) {
        ui.add(
            TextEdit::singleline(&mut self.search)
                .hint_text("Search meetings")
                .desired_width(theme::TOOLBAR_SEARCH_WIDTH),
        );
        ui.add_space(theme::GAP);
        if let Some(error) = &self.fetch_error {
            widgets::empty_state(ui, theme::icons::WARNING, "Meetings cannot be read.", error);
            return;
        }
        if self.list.is_empty() {
            let (title, caption) = if self.search.trim().is_empty() {
                (
                    "No meetings yet.",
                    "Start meeting notes above, or from the tray, when a call begins.",
                )
            } else {
                ("No matches.", "Search covers titles and what was said.")
            };
            widgets::empty_state(ui, theme::icons::MAGNIFYING_GLASS, title, caption);
            return;
        }
        for m in &self.list {
            let recording =
                matches!(meetings.status(), MeetingStatus::Recording { id, .. } if *id == m.id);
            let finishing = meetings.finishing().contains(&m.id);
            let response = theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title_of(m)).strong());
                        ui.label(
                            RichText::new(meta_line(m, &self.tz, recording, finishing))
                                .small()
                                .weak(),
                        );
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
            if response.clicked() {
                self.selected = Some(m.id.clone());
            }
            ui.add_space(theme::GAP);
        }
    }
}

fn live_pane(ui: &mut Ui, meetings: &MeetingController) {
    let (_, lines) = meetings.live();
    ui.add_space(theme::GAP);
    if lines.is_empty() {
        ui.label(
            RichText::new("Lines appear here about half a minute after they are said.")
                .small()
                .weak(),
        );
        return;
    }
    ScrollArea::vertical()
        .id_salt("meeting-live")
        .max_height(220.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in lines {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(hark_meeting::export::format_timestamp(line.start_ms))
                            .small()
                            .weak(),
                    );
                    ui.label(RichText::new(if line.channel == 0 { "Me" } else { "Them" }).strong());
                    ui.label(&line.text);
                });
            }
        });
}

fn consent_notice(ui: &mut Ui) -> Option<PageIntent> {
    let mut intent = None;
    theme::card(ui, |ui| {
        ui.label(RichText::new("Before you record").strong());
        ui.label(
            "Some places require everyone's consent to record a call. Hark records your \
             microphone and the other participants' audio on this device. Let people know.",
        );
        ui.horizontal(|ui| {
            if ui.button("Copy an announcement line").clicked() {
                // Plain clipboard copy, never the injection stash/restore path
                // (that restores the old clipboard and undoes this copy).
                ui.ctx().copy_text(ANNOUNCEMENT.to_string());
            }
            if ui
                .add(theme::primary_button(ui.visuals(), "Got it"))
                .clicked()
            {
                intent = Some(PageIntent::AcknowledgeConsent);
            }
        });
    });
    intent
}

fn title_of(m: &MeetingSummary) -> String {
    match m.title.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => match m.app_hint.as_deref() {
            Some(app) => format!("{} meeting", hark_pipeline::meeting::app_display_name(app)),
            None => "Meeting".to_string(),
        },
    }
}

fn meta_line(m: &MeetingSummary, tz: &TimeZone, recording: bool, finishing: bool) -> String {
    let when = format::full_timestamp(m.started_ms, tz);
    let state = if recording {
        " · recording".to_string()
    } else if finishing {
        " · finishing".to_string()
    } else {
        match m.ended_ms {
            Some(end) => format!(" · {}", format::duration(end - m.started_ms)),
            None => String::new(),
        }
    };
    let lines = format!(" · {} lines", m.segment_count);
    let audio = if m.audio_evicted_ms.is_some() {
        " · audio removed"
    } else {
        ""
    };
    format!("{when}{state}{lines}{audio}")
}

/// "14:30" in the user's zone (also the tray's "since" time).
pub(crate) fn local_clock(ts_ms: i64, tz: &TimeZone) -> String {
    jiff::Timestamp::from_millisecond(ts_ms)
        .map(|t| t.to_zoned(tz.clone()).strftime("%H:%M").to_string())
        .unwrap_or_default()
}

/// "14:30:05" in the user's zone: the pending auto-stop time, where the
/// seconds are the point.
pub(crate) fn local_clock_seconds(ts_ms: i64, tz: &TimeZone) -> String {
    jiff::Timestamp::from_millisecond(ts_ms)
        .map(|t| t.to_zoned(tz.clone()).strftime("%H:%M:%S").to_string())
        .unwrap_or_default()
}

fn describe(e: &StoreError) -> String {
    format!("The local database returned an error: {e}")
}
