//! The native landing page: actual shortcut, daily progress and recent local
//! history. Both history and numeric summaries arrive from the storage worker.
use crate::pipeline::PipelineStatus;
use crate::storage::{StorageCmd, StorageHandle};
use crate::theme;
use crate::ui::{format, insights_cache::InsightsCache, pages::Page};
use egui::{RichText, Ui};
use hark_config::Settings;
use hark_store::Entry;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub struct HomePage {
    insights: InsightsCache,
    recent: Vec<Entry>,
    generation: Option<u64>,
    pending: Option<Receiver<Result<Vec<Entry>, String>>>,
    error: Option<String>,
    copied: Option<i64>,
}

impl HomePage {
    pub fn new() -> Self {
        Self {
            insights: InsightsCache::new(),
            recent: Vec::new(),
            generation: None,
            pending: None,
            error: None,
            copied: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        settings: &Settings,
        status: &PipelineStatus,
        storage: Option<&StorageHandle>,
        unavailable: Option<&str>,
    ) -> Option<Page> {
        let mut target = None;
        if let Some(storage) = storage {
            self.insights.refresh(ui.ctx(), storage, 1, false);
            self.refresh_recent(storage);
        }
        egui::ScrollArea::vertical()
            .id_salt("home-content")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading("Make room for your next idea.");
                ui.label(RichText::new(format::date(jiff::Timestamp::now().as_millisecond(), &self.insights.tz)).weak());
                ui.add_space(theme::SECTION_GAP);
                egui::Frame::new()
                    .fill(theme::tint(ui.visuals()))
                    .corner_radius(theme::SURFACE_RADIUS)
                    .inner_margin(theme::HERO_PADDING)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new("LESS TYPING. MORE THINKING.").small().weak());
                        ui.add_space(theme::ROW_GAP);
                        ui.label(RichText::new("Your thoughts,\nin your own words.").font(theme::hero_font()));
                        ui.add_space(theme::ROW_GAP);
                        ui.label("Hold your shortcut, speak naturally, and release.\nHark puts the words wherever you’re working.");
                        ui.add_space(theme::SECTION_GAP);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&settings.hotkey.ptt_key).monospace().strong());
                            ui.label(RichText::new("Hold to talk · release to insert").small().weak());
                            if ui.button("Edit shortcut").clicked() {
                                target = Some(Page::Settings);
                            }
                        });
                        status_line(ui, status);
                    });
                ui.add_space(theme::SECTION_GAP);
                if let Some(data) = &self.insights.data {
                    ui.horizontal_wrapped(|ui| {
                        summary(ui, &format::count(data.period.words), "words recorded today");
                        summary(ui, &data.period.estimated_saved_ms().map(format::duration).unwrap_or_else(|| "—".into()), "saved, estimated");
                        summary(ui, &format!("{} days", data.current_streak), "current streak");
                        if ui.button("View insights").clicked() {
                            target = Some(Page::Stats);
                        }
                    });
                    if !data.coverage.period_complete {
                        ui.label(RichText::new("Partial day: activity before detailed tracking began may be missing.").small().weak());
                    }
                } else if let Some(error) = &self.insights.error {
                    ui.label(RichText::new(error).color(theme::danger(ui.visuals())));
                    if ui.button("Retry insights").clicked() { self.insights.retry(); }
                }
                ui.add_space(theme::SECTION_GAP);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Fresh from your thoughts").text_style(theme::subheading()));
                    if ui.button("All history").clicked() { target = Some(Page::History); }
                });
                ui.add_space(theme::ROW_GAP);
                if storage.is_none() {
                    ui.label(unavailable.unwrap_or("Local history is unavailable."));
                } else if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(theme::danger(ui.visuals())));
                    if ui.button("Retry history").clicked() { self.generation = None; }
                } else if self.pending.is_some() {
                    ui.label("Loading recent dictations…");
                } else if self.recent.is_empty() {
                    ui.label(if settings.history.capture { "Your next thought starts here. Dictate into any text field to see it here." } else { "History capture is off. Your numeric stats still update." });
                } else if let Some(storage) = storage {
                    for entry in &self.recent {
                        ui.push_id(entry.id, |ui| {
                            ui.separator();
                            ui.add_space(theme::GAP);
                            let preview: String = entry.final_text.chars().take(240).collect();
                            ui.label(if entry.final_text.chars().count() > 240 { format!("{preview}…") } else { preview });
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("{} · {} · {} · {}", format::relative_time(entry.ts_ms, jiff::Timestamp::now().as_millisecond()), entry.stt_model, entry.voice, entry.cleanup_model.as_deref().unwrap_or("no separate cleanup"))).small().weak());
                                if ui.button(if self.copied == Some(entry.id) { "Copied" } else { "Copy" }).clicked() {
                                    ui.ctx().copy_text(entry.final_text.clone());
                                    self.copied = Some(entry.id);
                                }
                                if ui.button("Delete").clicked() { storage.send(StorageCmd::DeleteEntry(entry.id)); }
                            });
                            ui.add_space(theme::ROW_GAP);
                        });
                    }
                }
                ui.add_space(theme::SECTION_GAP);
                ui.label(RichText::new("History stays here. Cloud dictation uses your chosen provider.").small().weak());
            });
        target
    }

    fn refresh_recent(&mut self, storage: &StorageHandle) {
        if self.generation != Some(storage.generation()) {
            let (reply, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.error = None;
            self.recent.clear();
            self.generation = Some(storage.generation());
            storage.send(StorageCmd::GetRecentEntries { reply });
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(entries)) => {
                    self.recent = entries;
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("The storage worker is unavailable.".into());
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

fn summary(ui: &mut Ui, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).size(theme::TITLE_SIZE));
        ui.label(RichText::new(label).small().weak());
    });
    ui.add_space(theme::ROW_GAP);
}

fn status_line(ui: &mut Ui, status: &PipelineStatus) {
    let (icon, text) = match status {
        PipelineStatus::Idle => (theme::icons::CHECK, "Ready to dictate"),
        PipelineStatus::Recording => (theme::icons::MICROPHONE, "Listening…"),
        PipelineStatus::Processing => (theme::icons::CIRCLE_NOTCH, "Processing your words…"),
        PipelineStatus::LoadingModel => (theme::icons::CIRCLE_NOTCH, "Loading on-device model…"),
        PipelineStatus::Errored { detail, .. } | PipelineStatus::Stopped { detail, .. } => {
            (theme::icons::WARNING, detail.as_str())
        }
        PipelineStatus::Hint { detail } => (theme::icons::MICROPHONE, detail.as_str()),
    };
    ui.add_space(theme::GAP);
    ui.label(theme::icon_label_job(ui.style(), icon, text));
}
