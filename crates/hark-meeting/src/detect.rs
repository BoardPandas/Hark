//! Meeting auto-detection, pure: who is using the microphone, and when to
//! offer (or start) meeting notes. The per-OS probe (`probe`) takes the
//! snapshots; everything that decides lives here and is tested on fixtures.
//!
//! Rules (plan §4.8): Hark itself never counts (its pre-roll stream holds the
//! mic permanently); only `detect_apps` count, and a browser only while one of
//! its windows carries a meeting title; the app must hold the mic for
//! [`DEBOUNCE_MS`]; one prompt per session, with "Not this meeting", a
//! dismissal, or a stop suppressing that app until it releases the mic; a
//! detected meeting auto-stops once its app has released the mic for
//! `auto_stop_after_ms`, and a manual one never does.
//!
//! Time is a caller-supplied millisecond counter, so tests use plain numbers.

mod apps;
mod loopback;
pub use apps::{title_has_meeting_marker, BROWSERS, DEFAULT_APPS};
pub use loopback::{
    process_tree, root_pid, target_exe, them_source, tree_sessions, Proc, RenderSession,
    RenderSnapshot, ThemSource,
};

/// How often detection polls where no change notification exists (Windows
/// and macOS). A Windows snapshot of the capture sessions costs ~3 ms.
pub const POLL_MS: u64 = 2_000;

/// An app must hold the mic this long before it counts: a device test or a
/// voice message is shorter.
pub const DEBOUNCE_MS: u64 = 5_000;

/// An app holding the microphone, as the probe names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicApp {
    /// A packaged app by package family name (`MSTeams_8wekyb3d8bbwe`), or a
    /// macOS bundle identifier: a stable name with no path or version in it.
    Packaged(String),
    /// A desktop app, by full exe path (Windows) or binary name (Linux).
    Desktop(String),
}

impl MicApp {
    /// The identifier `detect_apps` matches against, lowercase: the package
    /// family, or the exe file name. Desktop paths carry version folders
    /// (`Discord\app-1.0.9259\Discord.exe`), so the path itself cannot be it.
    pub fn id(&self) -> String {
        match self {
            MicApp::Packaged(family) => family.to_lowercase(),
            MicApp::Desktop(path) => path
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(path)
                .to_lowercase(),
        }
    }

    fn is_exe(&self, exe_path: &str) -> bool {
        matches!(self, MicApp::Desktop(p) if p.eq_ignore_ascii_case(exe_path))
    }
}

/// An audio device as Core Audio lists it. Windows only; the other probes
/// leave every endpoint field empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The endpoint id (`{0.0.0.00000000}.{guid}`), which endpoint loopback
    /// opens.
    pub id: String,
    /// `PKEY_Device_FriendlyName` ("Microphone (Shure MV7+)"): the name cpal
    /// shows, so a microphone opens by it.
    pub name: String,
}

/// One app's microphone use in a snapshot.
#[derive(Debug, Clone)]
pub struct MicUse {
    pub app: MicApp,
    /// One of this app's capture streams is running right now.
    pub in_use: bool,
    /// The input endpoints those running streams record from. Teams has its
    /// own device setting, so this is often not the communications default.
    pub capture_endpoints: Vec<Endpoint>,
}

/// The input endpoint `app_id` (a [`MicApp::id`]) is recording from right
/// now, the first in probe order when there are several.
pub fn capture_endpoint<'a>(snapshot: &'a Snapshot, app_id: &str) -> Option<&'a Endpoint> {
    snapshot
        .users
        .iter()
        .filter(|u| u.in_use && u.app.id() == app_id)
        .find_map(|u| u.capture_endpoints.first())
}

/// One poll's view of the machine.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub users: Vec<MicUse>,
    /// Lowercase exe names of processes owning a top-level window whose title
    /// passes [`title_has_meeting_marker`]. Only browsers consult it.
    pub meeting_windows: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectMode {
    Off,
    /// Offer to take notes (non-modal prompt).
    Ask,
    /// Start without asking; the recording state stays visible.
    Auto,
}

#[derive(Debug, Clone)]
pub struct DetectConfig {
    pub mode: DetectMode,
    /// `detect_apps`: package families and exe names, any case.
    pub apps: Vec<String>,
    /// `auto_stop_after_s` in ms; 0 = never auto-stop.
    pub auto_stop_after_ms: u64,
    /// Hark's own exe path (`std::env::current_exe()`), never a meeting.
    pub self_exe: String,
}

/// What the coordinator must do after an observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    None,
    /// Ask mode: show "<app> is using your mic. Take meeting notes?"
    Prompt(String),
    /// The prompted app released the mic: hide the prompt.
    Retract,
    /// Auto mode: start recording this app's meeting.
    Start(String),
    /// The detected meeting's app released the mic long enough: stop.
    Stop,
}

/// The user's answer to a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Start,
    NotThisMeeting,
    /// Closed, or timed out after 30 s.
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Watching,
    Prompting(String),
    Recording {
        /// The app whose call this is, which auto-stops it; `None` for a
        /// manual start with no call in progress, which is never auto-stopped.
        app: Option<String>,
        released_at: Option<u64>,
        /// Seen holding the mic since the recording started. A detected app
        /// always has; one adopted by a manual start must be seen once more,
        /// or the adoption is dropped (the call may have ended just before).
        confirmed: bool,
    },
}

pub struct Detector {
    config: DetectConfig,
    phase: Phase,
    /// The app waiting out the debounce, and when it was first seen.
    candidate: Option<(String, u64)>,
    /// Apps not to prompt for again until they release the mic.
    suppressed: Vec<String>,
    /// Meeting apps holding the mic at the last observation.
    last_active: Vec<String>,
}

impl Detector {
    pub fn new(config: DetectConfig) -> Self {
        Detector {
            config,
            phase: Phase::Watching,
            candidate: None,
            suppressed: Vec::new(),
            last_active: Vec::new(),
        }
    }

    /// Settings changed. A meeting in progress keeps its trigger.
    pub fn set_config(&mut self, config: DetectConfig) {
        self.config = config;
    }

    /// True while a meeting is recording (either trigger).
    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. })
    }

    /// Milliseconds until a detected meeting auto-stops: `Some` once its app
    /// has released the mic (as of the last observation), `None` while the
    /// app holds it, for a manual meeting, or with auto-stop off. The UI
    /// shows it, so hanging up visibly leads somewhere.
    pub fn stop_pending_ms(&self, now_ms: u64) -> Option<u64> {
        let Phase::Recording {
            app: Some(_),
            released_at: Some(since),
            ..
        } = &self.phase
        else {
            return None;
        };
        let limit = self.config.auto_stop_after_ms;
        (limit > 0).then(|| limit.saturating_sub(now_ms.saturating_sub(*since)))
    }

    /// Delay until another snapshot is required even without a registry change.
    /// A mic-open notification starts the debounce, but no further write need
    /// occur when its five seconds elapse. The same applies to auto-stop after
    /// mic release. Callers must take a fresh snapshot at this deadline: a
    /// cached snapshot could start or stop a call whose state changed meanwhile.
    pub fn next_observation_in_ms(&self, now_ms: u64) -> Option<u64> {
        match &self.phase {
            Phase::Watching if self.config.mode != DetectMode::Off => self
                .candidate
                .as_ref()
                .map(|(_, since)| DEBOUNCE_MS.saturating_sub(now_ms.saturating_sub(*since))),
            Phase::Recording {
                app: Some(_),
                confirmed: false,
                ..
            } => Some(0),
            Phase::Recording { .. } => self.stop_pending_ms(now_ms),
            _ => None,
        }
    }

    /// Feed one snapshot, prompted by a notification, deadline, or backstop poll.
    pub fn observe(&mut self, snapshot: &Snapshot, now_ms: u64) -> Verdict {
        let holding = self.holding(snapshot);
        let active = self.meeting_apps(&holding, snapshot);
        // Suppression lasts until the app lets go of the mic entirely.
        self.suppressed.retain(|app| holding.contains(app));
        self.last_active = active.clone();

        match &mut self.phase {
            Phase::Recording {
                app: Some(app),
                released_at,
                confirmed,
            } => {
                if holding.contains(app) {
                    *released_at = None;
                    *confirmed = true;
                    return Verdict::None;
                }
                if !*confirmed {
                    // Adopted from a call that was already over: this is a
                    // plain manual meeting after all, never auto-stopped.
                    self.phase = Phase::Recording {
                        app: None,
                        released_at: None,
                        confirmed: true,
                    };
                    return Verdict::None;
                }
                let since = *released_at.get_or_insert(now_ms);
                let limit = self.config.auto_stop_after_ms;
                if limit > 0 && now_ms.saturating_sub(since) >= limit {
                    self.phase = Phase::Watching;
                    self.candidate = None;
                    return Verdict::Stop;
                }
                Verdict::None
            }
            Phase::Recording { app: None, .. } => Verdict::None,
            Phase::Prompting(app) => {
                if active.contains(app) {
                    Verdict::None
                } else {
                    self.phase = Phase::Watching;
                    self.candidate = None;
                    Verdict::Retract
                }
            }
            Phase::Watching => self.watch(&active, now_ms),
        }
    }

    fn watch(&mut self, active: &[String], now_ms: u64) -> Verdict {
        if self.config.mode == DetectMode::Off {
            self.candidate = None;
            return Verdict::None;
        }
        let Some(app) = active.iter().find(|a| !self.suppressed.contains(a)) else {
            self.candidate = None;
            return Verdict::None;
        };
        let since = match &self.candidate {
            Some((current, since)) if current == app => *since,
            _ => {
                self.candidate = Some((app.clone(), now_ms));
                now_ms
            }
        };
        if now_ms.saturating_sub(since) < DEBOUNCE_MS {
            return Verdict::None;
        }
        let app = app.clone();
        self.candidate = None;
        match self.config.mode {
            DetectMode::Ask => {
                self.phase = Phase::Prompting(app.clone());
                Verdict::Prompt(app)
            }
            DetectMode::Auto => {
                self.phase = Phase::Recording {
                    app: Some(app.clone()),
                    released_at: None,
                    confirmed: true,
                };
                Verdict::Start(app)
            }
            DetectMode::Off => unreachable!("handled above"),
        }
    }

    /// The user answered the prompt. Ignored if no prompt is showing.
    pub fn answer(&mut self, answer: Answer) {
        let Phase::Prompting(app) = &self.phase else {
            return;
        };
        let app = app.clone();
        self.phase = match answer {
            Answer::Start => Phase::Recording {
                app: Some(app),
                released_at: None,
                confirmed: true,
            },
            Answer::NotThisMeeting | Answer::Dismissed => {
                self.suppressed.push(app);
                Phase::Watching
            }
        };
    }

    /// A meeting was started by hand (tray or Meetings page). Returns
    /// [`Verdict::Retract`] if a prompt was showing, and the app it adopted.
    ///
    /// **A manual start during a call adopts that call:** if a meeting app
    /// held the mic at the last observation (prompted, suppressed after an
    /// unanswered prompt, or still in its debounce), the meeting auto-stops
    /// when that app hangs up, like a detected one. Without a call in
    /// progress it is a plain manual meeting (an in-person one, say) and is
    /// never auto-stopped. The first real Meet call was started by hand after
    /// its prompt went unseen, and then recorded a minute past the hang-up.
    pub fn started_manually(&mut self) -> (Verdict, Option<String>) {
        let was_prompting = matches!(self.phase, Phase::Prompting(_));
        let adopted = self.last_active.first().cloned();
        self.phase = Phase::Recording {
            app: adopted.clone(),
            released_at: None,
            confirmed: false,
        };
        self.candidate = None;
        let verdict = if was_prompting {
            Verdict::Retract
        } else {
            Verdict::None
        };
        (verdict, adopted)
    }

    /// The meeting stopped (by the user, or after a `Stop` verdict). Every
    /// meeting app still holding the mic is suppressed, so stopping notes on
    /// a call that carries on does not immediately re-prompt for it.
    pub fn stopped(&mut self) {
        if !self.is_recording() {
            return;
        }
        for app in &self.last_active {
            if !self.suppressed.contains(app) {
                self.suppressed.push(app.clone());
            }
        }
        self.phase = Phase::Watching;
        self.candidate = None;
    }

    /// Ids of every app (not Hark) holding the mic, meeting app or not.
    fn holding(&self, snapshot: &Snapshot) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for user in &snapshot.users {
            if !user.in_use || user.app.is_exe(&self.config.self_exe) {
                continue;
            }
            let id = user.app.id();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// The subset of `holding` that counts as a meeting right now.
    fn meeting_apps(&self, holding: &[String], snapshot: &Snapshot) -> Vec<String> {
        holding
            .iter()
            .filter(|id| self.config.apps.iter().any(|a| a.eq_ignore_ascii_case(id)))
            .filter(|id| {
                !BROWSERS.contains(&id.as_str())
                    || snapshot
                        .meeting_windows
                        .iter()
                        .any(|w| w.eq_ignore_ascii_case(id))
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests;
