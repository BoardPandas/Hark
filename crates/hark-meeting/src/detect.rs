//! Meeting auto-detection, pure: who is using the microphone, and when to
//! offer (or start) meeting notes. The per-OS probe (`probe_win`) takes the
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

/// How often the detector thread polls. The ConsentStore read costs ~0.6 ms.
pub const POLL_MS: u64 = 2_000;

/// An app must hold the mic this long before it counts: a device test or a
/// voice message is shorter.
pub const DEBOUNCE_MS: u64 = 5_000;

/// Built-in `detect_apps`: packaged-app family names and desktop exe names,
/// compared case-insensitively. Zoom, Webex, GoTo and RingCentral exe names
/// are from vendor documentation, not yet seen in a live-call ConsentStore
/// dump (CP0 row 5); the list is user-editable for exactly that reason.
pub const DEFAULT_APPS: &[&str] = &[
    "MSTeams_8wekyb3d8bbwe", // new Teams (packaged)
    "teams.exe",             // classic Teams
    "zoom.exe",
    "webex.exe",
    "ciscocollabhost.exe", // Webex's media process
    "slack.exe",
    "discord.exe",
    "goto.exe",
    "ringcentral.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
];

/// Apps that hold the mic for anything (a dictation site, a voice note), so
/// they only count while a window title says a meeting is open.
pub const BROWSERS: &[&str] = &["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe"];

/// Title fragments that mark a browser window as a meeting.
const MEETING_MARKERS: &[&str] = &["Meet -", "Meet –", "Microsoft Teams", "Zoom"];

/// Does a window title say a meeting is open? Titles are matched in memory
/// and never stored or logged.
pub fn title_has_meeting_marker(title: &str) -> bool {
    MEETING_MARKERS.iter().any(|m| title.contains(m))
}

/// A ConsentStore microphone entry's app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicApp {
    /// A packaged app, by package family name (`MSTeams_8wekyb3d8bbwe`).
    Packaged(String),
    /// A desktop app, by full exe path.
    Desktop(String),
}

impl MicApp {
    /// A desktop entry from its `NonPackaged` subkey name, which is the exe
    /// path with `#` in place of `\`.
    pub fn from_nonpackaged_key(key: &str) -> MicApp {
        MicApp::Desktop(key.replace('#', "\\"))
    }

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

/// One app's microphone use in a snapshot.
#[derive(Debug, Clone)]
pub struct MicUse {
    pub app: MicApp,
    /// `LastUsedTimeStart > 0 && LastUsedTimeStop == 0`.
    pub in_use: bool,
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
        /// The app that triggered it; `None` for a manual start, which is
        /// never auto-stopped.
        app: Option<String>,
        released_at: Option<u64>,
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

    /// Feed one poll.
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
            } => {
                if holding.contains(app) {
                    *released_at = None;
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
            },
            Answer::NotThisMeeting | Answer::Dismissed => {
                self.suppressed.push(app);
                Phase::Watching
            }
        };
    }

    /// A meeting was started by hand (tray or Meetings page). It is never
    /// auto-stopped. Returns [`Verdict::Retract`] if a prompt was showing.
    pub fn started_manually(&mut self) -> Verdict {
        let was_prompting = matches!(self.phase, Phase::Prompting(_));
        self.phase = Phase::Recording {
            app: None,
            released_at: None,
        };
        self.candidate = None;
        if was_prompting {
            Verdict::Retract
        } else {
            Verdict::None
        }
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

/// A process, for resolving a meeting app's loopback target.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Exe file name, any case.
    pub exe: String,
}

/// The exe whose process tree carries a detected app's audio. Packaged apps
/// are keyed by family, so they map to their exe here.
pub fn target_exe(app_id: &str) -> String {
    match app_id {
        "msteams_8wekyb3d8bbwe" => "ms-teams.exe".to_string(),
        other => other.to_string(),
    }
}

/// The root of `exe`'s process tree: a process running `exe` whose parent is
/// not also `exe` (browsers and Electron apps run many children of one
/// root). Per-process loopback in include mode on this PID covers the app's
/// audio children. With several roots (two browser profiles), the lowest PID
/// wins so the choice is stable across polls.
pub fn root_pid(procs: &[Proc], exe: &str) -> Option<u32> {
    let runs_exe = |pid: u32| {
        procs
            .iter()
            .any(|p| p.pid == pid && p.exe.eq_ignore_ascii_case(exe))
    };
    procs
        .iter()
        .filter(|p| p.exe.eq_ignore_ascii_case(exe) && !runs_exe(p.parent))
        .map(|p| p.pid)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HARK: &str = r"C:\Program Files\Hark\hark.exe";
    const TEAMS: &str = "msteams_8wekyb3d8bbwe";

    fn config(mode: DetectMode) -> DetectConfig {
        DetectConfig {
            mode,
            apps: DEFAULT_APPS.iter().map(|s| s.to_string()).collect(),
            auto_stop_after_ms: 60_000,
            self_exe: HARK.to_string(),
        }
    }

    fn packaged(family: &str, in_use: bool) -> MicUse {
        MicUse {
            app: MicApp::Packaged(family.to_string()),
            in_use,
        }
    }

    fn desktop(key: &str, in_use: bool) -> MicUse {
        MicUse {
            app: MicApp::from_nonpackaged_key(key),
            in_use,
        }
    }

    /// Hark always reads as in use: its pre-roll stream never closes.
    fn hark() -> MicUse {
        desktop(&HARK.replace('\\', "#"), true)
    }

    fn snap(users: Vec<MicUse>) -> Snapshot {
        Snapshot {
            users,
            meeting_windows: Vec::new(),
        }
    }

    fn teams_call() -> Snapshot {
        snap(vec![hark(), packaged("MSTeams_8wekyb3d8bbwe", true)])
    }

    fn quiet() -> Snapshot {
        snap(vec![hark(), packaged("MSTeams_8wekyb3d8bbwe", false)])
    }

    /// Poll `snapshot` every 2 s over [from, to], returning non-None verdicts.
    fn poll(d: &mut Detector, s: &Snapshot, from: u64, to: u64) -> Vec<(u64, Verdict)> {
        (from..=to)
            .step_by(POLL_MS as usize)
            .map(|t| (t, d.observe(s, t)))
            .filter(|(_, v)| *v != Verdict::None)
            .collect()
    }

    #[test]
    fn hark_alone_is_never_a_meeting() {
        let mut d = Detector::new(config(DetectMode::Auto));
        assert!(poll(&mut d, &snap(vec![hark()]), 0, 600_000).is_empty());
    }

    #[test]
    fn packaged_teams_prompts_after_the_debounce() {
        let mut d = Detector::new(config(DetectMode::Ask));
        let fired = poll(&mut d, &teams_call(), 0, 20_000);
        // Seen at 0; 4 s is not enough; the first poll at >= 5 s is 6 s.
        assert_eq!(fired, vec![(6_000, Verdict::Prompt(TEAMS.to_string()))]);
    }

    #[test]
    fn a_short_mic_use_never_prompts() {
        let mut d = Detector::new(config(DetectMode::Ask));
        assert!(poll(&mut d, &teams_call(), 0, 4_000).is_empty());
        assert!(poll(&mut d, &quiet(), 6_000, 8_000).is_empty());
        // Holding again restarts the debounce from scratch.
        assert_eq!(
            poll(&mut d, &teams_call(), 10_000, 20_000),
            vec![(16_000, Verdict::Prompt(TEAMS.to_string()))]
        );
    }

    #[test]
    fn desktop_apps_match_on_exe_name_whatever_the_version_folder() {
        let mut d = Detector::new(config(DetectMode::Auto));
        let zoom = snap(vec![
            hark(),
            desktop(r"C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", true),
        ]);
        assert_eq!(
            poll(&mut d, &zoom, 0, 10_000),
            vec![(6_000, Verdict::Start("zoom.exe".to_string()))]
        );
        assert_eq!(
            MicApp::from_nonpackaged_key(
                r"C:#Users#me#AppData#Local#Discord#app-1.0.9259#Discord.exe"
            )
            .id(),
            "discord.exe"
        );
    }

    #[test]
    fn apps_not_on_the_list_are_ignored() {
        let mut d = Detector::new(config(DetectMode::Auto));
        let recorder = snap(vec![desktop(r"C:#Tools#audacity.exe", true)]);
        assert!(poll(&mut d, &recorder, 0, 60_000).is_empty());
    }

    #[test]
    fn a_browser_counts_only_with_a_meeting_window() {
        let mut d = Detector::new(config(DetectMode::Ask));
        let mut chrome = snap(vec![desktop(
            r"C:#Program Files#Google#Chrome#Application#chrome.exe",
            true,
        )]);
        assert!(
            poll(&mut d, &chrome, 0, 30_000).is_empty(),
            "no meeting title"
        );
        chrome.meeting_windows = vec!["chrome.exe".to_string()];
        assert_eq!(
            poll(&mut d, &chrome, 32_000, 40_000),
            vec![(38_000, Verdict::Prompt("chrome.exe".to_string()))]
        );
    }

    #[test]
    fn meeting_markers_match_meet_teams_and_zoom_titles() {
        assert!(title_has_meeting_marker(
            "Meet – abc-defg-hij - Google Chrome"
        ));
        assert!(title_has_meeting_marker("Meet - abc-defg-hij"));
        assert!(title_has_meeting_marker("Weekly sync | Microsoft Teams"));
        assert!(title_has_meeting_marker("Zoom Meeting"));
        assert!(!title_has_meeting_marker("Inbox - Gmail - Google Chrome"));
    }

    #[test]
    fn off_mode_never_prompts() {
        let mut d = Detector::new(config(DetectMode::Off));
        assert!(poll(&mut d, &teams_call(), 0, 60_000).is_empty());
    }

    #[test]
    fn not_this_meeting_suppresses_until_the_app_releases_the_mic() {
        let mut d = Detector::new(config(DetectMode::Ask));
        poll(&mut d, &teams_call(), 0, 6_000);
        d.answer(Answer::NotThisMeeting);
        assert!(
            poll(&mut d, &teams_call(), 8_000, 120_000).is_empty(),
            "same call: no second prompt"
        );
        poll(&mut d, &quiet(), 122_000, 122_000);
        assert_eq!(
            poll(&mut d, &teams_call(), 124_000, 130_000),
            vec![(130_000, Verdict::Prompt(TEAMS.to_string()))],
            "the next call prompts again"
        );
    }

    #[test]
    fn a_dismissed_prompt_suppresses_like_not_this_meeting() {
        let mut d = Detector::new(config(DetectMode::Ask));
        poll(&mut d, &teams_call(), 0, 6_000);
        d.answer(Answer::Dismissed);
        assert!(poll(&mut d, &teams_call(), 8_000, 60_000).is_empty());
    }

    #[test]
    fn the_prompt_is_retracted_when_the_app_lets_go() {
        let mut d = Detector::new(config(DetectMode::Ask));
        poll(&mut d, &teams_call(), 0, 6_000);
        assert_eq!(d.observe(&quiet(), 8_000), Verdict::Retract);
        d.answer(Answer::Start); // too late: no prompt is showing
        assert!(!d.is_recording());
    }

    #[test]
    fn accepted_detection_auto_stops_after_the_release_delay() {
        let mut d = Detector::new(config(DetectMode::Ask));
        poll(&mut d, &teams_call(), 0, 6_000);
        d.answer(Answer::Start);
        assert!(d.is_recording());
        assert!(poll(&mut d, &teams_call(), 8_000, 3_600_000).is_empty());
        // Released at 3_602_000; 60 s later it stops.
        assert_eq!(
            poll(&mut d, &quiet(), 3_602_000, 3_700_000),
            vec![(3_662_000, Verdict::Stop)]
        );
        assert!(!d.is_recording());
    }

    #[test]
    fn a_brief_release_does_not_stop_the_meeting() {
        // Teams re-opens the mic when switching devices mid-call.
        let mut d = Detector::new(config(DetectMode::Auto));
        poll(&mut d, &teams_call(), 0, 6_000);
        assert!(poll(&mut d, &quiet(), 8_000, 50_000).is_empty());
        assert!(poll(&mut d, &teams_call(), 52_000, 60_000).is_empty());
        assert!(
            poll(&mut d, &quiet(), 62_000, 110_000).is_empty(),
            "the release clock restarted"
        );
        assert_eq!(
            poll(&mut d, &quiet(), 112_000, 130_000),
            vec![(122_000, Verdict::Stop)]
        );
    }

    #[test]
    fn auto_stop_zero_never_stops() {
        let mut cfg = config(DetectMode::Auto);
        cfg.auto_stop_after_ms = 0;
        let mut d = Detector::new(cfg);
        poll(&mut d, &teams_call(), 0, 6_000);
        assert!(poll(&mut d, &quiet(), 8_000, 3_600_000).is_empty());
        assert!(d.is_recording());
    }

    #[test]
    fn a_manual_meeting_is_never_auto_stopped() {
        let mut d = Detector::new(config(DetectMode::Auto));
        assert_eq!(d.started_manually(), Verdict::None);
        assert!(
            poll(&mut d, &teams_call(), 0, 30_000).is_empty(),
            "no start"
        );
        assert!(
            poll(&mut d, &quiet(), 32_000, 3_600_000).is_empty(),
            "no stop"
        );
        assert!(d.is_recording());
    }

    #[test]
    fn a_manual_start_retracts_an_open_prompt() {
        let mut d = Detector::new(config(DetectMode::Ask));
        poll(&mut d, &teams_call(), 0, 6_000);
        assert_eq!(d.started_manually(), Verdict::Retract);
    }

    #[test]
    fn stopping_during_the_call_does_not_reprompt_for_it() {
        let mut d = Detector::new(config(DetectMode::Auto));
        poll(&mut d, &teams_call(), 0, 6_000);
        d.stopped();
        assert!(poll(&mut d, &teams_call(), 8_000, 120_000).is_empty());
        poll(&mut d, &quiet(), 122_000, 122_000);
        assert_eq!(
            poll(&mut d, &teams_call(), 124_000, 130_000),
            vec![(130_000, Verdict::Start(TEAMS.to_string()))]
        );
    }

    #[test]
    fn root_pid_is_the_process_whose_parent_is_another_exe() {
        let procs = [
            Proc {
                pid: 4,
                parent: 0,
                exe: "System".into(),
            },
            Proc {
                pid: 900,
                parent: 800,
                exe: "explorer.exe".into(),
            },
            Proc {
                pid: 1200,
                parent: 900,
                exe: "chrome.exe".into(),
            },
            Proc {
                pid: 1300,
                parent: 1200,
                exe: "chrome.exe".into(),
            },
            Proc {
                pid: 1400,
                parent: 1200,
                exe: "chrome.exe".into(),
            },
            Proc {
                pid: 2000,
                parent: 900,
                exe: "ms-teams.exe".into(),
            },
            Proc {
                pid: 2100,
                parent: 2000,
                exe: "msedgewebview2.exe".into(),
            },
        ];
        assert_eq!(root_pid(&procs, "Chrome.exe"), Some(1200));
        assert_eq!(root_pid(&procs, &target_exe(TEAMS)), Some(2000));
        assert_eq!(root_pid(&procs, "zoom.exe"), None);
    }

    #[test]
    fn root_pid_prefers_the_lowest_of_several_roots() {
        // A parent that exited leaves an orphan whose parent PID is stale.
        let procs = [
            Proc {
                pid: 3000,
                parent: 900,
                exe: "chrome.exe".into(),
            },
            Proc {
                pid: 2500,
                parent: 77,
                exe: "chrome.exe".into(),
            },
        ];
        assert_eq!(root_pid(&procs, "chrome.exe"), Some(2500));
    }
}
