//! The devices a meeting records from: whose audio "Them" is, which
//! microphone "Me" is, and the notice when one of them stays silent.

use super::Coordinator;
use crate::meeting::Trigger;
use hark_audio::LoopbackTarget;
use hark_config::SystemSource;
use hark_meeting::detect::{self, ThemSource};
use hark_meeting::Channel;

impl Coordinator {
    /// Whose audio "Them" is. A call app (detected, or adopted by a manual
    /// start) seen playing is recorded from the output it plays to, since
    /// process loopback was measured deaf to Teams on any endpoint
    /// (`detect::them_source`). Otherwise as before: the detected app's
    /// process tree when the setting asks for it, else everything except
    /// Hark (a manual start always).
    pub(super) fn loopback_target(&self, trigger: Trigger, app: Option<&str>) -> LoopbackTarget {
        let everything_but_hark = LoopbackTarget::ExcludeTree(std::process::id());
        let Some(app) = app else {
            log::info!("meeting Them: everything except Hark (no call app)");
            return everything_but_hark;
        };
        let exe = detect::target_exe(app);
        let found = hark_meeting::probe::processes()
            .ok()
            .and_then(|procs| detect::root_pid(&procs, &exe).map(|root| (procs, root)));
        let Some((procs, root)) = found else {
            log::warn!("meeting app {exe} has no running process; capturing all system audio");
            return everything_but_hark;
        };
        match hark_meeting::probe::render_snapshot() {
            Ok(render) => {
                if let ThemSource::Endpoint(endpoint) = detect::them_source(&render, &procs, root) {
                    log::info!(
                        "meeting Them: endpoint loopback of {:?}, where {app} plays",
                        endpoint.name
                    );
                    return LoopbackTarget::Endpoint { id: endpoint.id };
                }
            }
            Err(e) => log::warn!("meeting Them: output sessions unreadable ({e})"),
        }
        if trigger == Trigger::Manual || self.settings.meeting.system_source != SystemSource::App {
            log::info!(
                "meeting Them: everything except Hark ({} start, system source {:?})",
                trigger.label(),
                self.settings.meeting.system_source
            );
            return everything_but_hark;
        }
        log::info!("meeting Them: process loopback of {exe} (pid {root})");
        LoopbackTarget::IncludeTree(root)
    }

    /// The microphone for "Me" (see [`choose_mic`]), logged with its reason.
    pub(super) fn mic_device(&self, app: Option<&str>) -> Option<String> {
        let (mic, reason) = choose_mic(
            self.settings.meeting.mic_device.clone(),
            || app.and_then(app_mic),
            hark_audio::communications_default_device,
        );
        log::info!(
            "meeting Me: {} ({reason})",
            mic.as_deref().unwrap_or("the system default microphone")
        );
        mic
    }

    /// A track went silent past its watchdog limit. Say so once per track,
    /// naming the right device when the probe knows it. Them is only worth a
    /// notice while the call app is playing something; otherwise the count
    /// goes on and is checked again at the next limit.
    pub(super) fn silence(&mut self, channel: Channel) {
        let Some(active) = &self.active else {
            return;
        };
        let slot = match channel {
            Channel::Me => 0,
            Channel::Them => 1,
        };
        if active.silence_noticed[slot] {
            return;
        }
        let id = active.recorder.id.clone();
        let app_name = active.app_name.clone();
        let text = match channel {
            Channel::Me => {
                let in_app = active.app.as_deref().and_then(app_mic);
                Some(me_silence_text(
                    active.mic.as_deref(),
                    app_name.as_deref(),
                    in_app.as_deref(),
                ))
            }
            Channel::Them => active
                .app
                .as_deref()
                .and_then(playing_endpoint)
                .map(|endpoint| them_silence_text(app_name.as_deref(), &endpoint)),
        };
        let Some(text) = text else {
            log::info!("meeting {id}: {channel:?} silent, but no call app is playing; no notice");
            return;
        };
        log::warn!("meeting {id}: {channel:?} track silent past its limit; telling the user");
        if let Some(active) = self.active.as_mut() {
            active.silence_noticed[slot] = true;
        }
        self.notice(Some(&id), text);
    }
}

/// The microphone for "Me", and why. Order: the explicit Meetings setting;
/// the endpoint the call app is recording from (Teams has its own device
/// setting, so the communications default can be another, even muted, mic:
/// the cause of six silent meetings in October 2026); the communications
/// default; else the system default (`None`). Lookups run only when needed.
pub(super) fn choose_mic(
    explicit: Option<String>,
    app_mic: impl FnOnce() -> Option<String>,
    communications_default: impl FnOnce() -> Option<String>,
) -> (Option<String>, &'static str) {
    if let Some(name) = explicit {
        return (Some(name), "the Meetings microphone setting");
    }
    if let Some(name) = app_mic() {
        return (Some(name), "the microphone the call app is recording from");
    }
    match communications_default() {
        Some(name) => (Some(name), "the communications default"),
        None => (None, "no communications default"),
    }
}

/// The microphone `app` is recording from right now, by name.
fn app_mic(app: &str) -> Option<String> {
    match hark_meeting::probe::snapshot() {
        Ok(snapshot) => detect::capture_endpoint(&snapshot, app).map(|e| e.name.clone()),
        Err(e) => {
            log::warn!("meeting: cannot see which mic {app} uses ({e})");
            None
        }
    }
}

/// The output `app`'s process tree is playing to right now, by name; `None`
/// when it plays nothing or cannot be found.
fn playing_endpoint(app: &str) -> Option<String> {
    let procs = hark_meeting::probe::processes().ok()?;
    let root = detect::root_pid(&procs, &detect::target_exe(app))?;
    let render = hark_meeting::probe::render_snapshot().ok()?;
    detect::tree_sessions(&render, &procs, root)
        .into_iter()
        .find(|s| s.active)
        .map(|s| s.endpoint.name.clone())
}

pub(super) fn me_silence_text(
    mic: Option<&str>,
    app_name: Option<&str>,
    app_mic: Option<&str>,
) -> String {
    let mic = mic.unwrap_or("your default microphone");
    match app_mic {
        Some(other) if other != mic => format!(
            "Hark hears nothing from {mic}. {} is using {other}; switch in Settings > Meetings.",
            app_name.unwrap_or("Your call app")
        ),
        _ => format!(
            "Hark hears nothing from {mic}. Check that it is not muted, or choose another \
             microphone in Settings > Meetings."
        ),
    }
}

fn them_silence_text(app_name: Option<&str>, endpoint: &str) -> String {
    format!(
        "Hark has heard nothing from the call for 2 minutes, although {} is playing to \
         {endpoint}. Other people may be missing from these notes.",
        app_name.unwrap_or("the call app")
    )
}
