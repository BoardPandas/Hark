//! The built-in app lists: which apps count as a meeting, which of them are
//! browsers, and the window titles that mark a browser tab as a meeting.

/// Built-in `detect_apps`: packaged-app family names and desktop exe names,
/// compared case-insensitively. Zoom, Webex, GoTo and RingCentral exe names
/// are from vendor documentation, not yet seen holding the mic on a live call
/// (CP0 row 5); the list is user-editable for exactly that reason.
///
/// The Linux entries are the binary names apps carry on their PipeWire
/// stream nodes (`firefox`, `chromium`, `zoom`, ...), including the wrapper
/// spellings distros differ on; they can never collide with the Windows ids
/// because those all end in `.exe` or are package families.
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
    // Serverdata's Elevate desktop app. Inferred, not documented: an
    // Electron NSIS install names its exe after the product, "Elevate UC".
    "elevate uc.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    // macOS bundle identifiers (Core Audio process objects report these).
    "com.microsoft.teams2",
    "com.microsoft.teams",
    "us.zoom.xos",
    "com.cisco.webexmeetingsapp",
    "com.cisco.webex",
    "com.tinyspeck.slackmacgap",
    "com.hnc.discord",
    "com.logmein.goto",
    "com.ringcentral.glip",
    "com.google.chrome",
    "com.microsoft.edgemac",
    "org.mozilla.firefox",
    "com.brave.browser",
    "com.apple.safari",
    // Linux binaries.
    "zoom",
    "teams",
    "teams-for-linux", // the community Electron client's binary
    "slack",
    "discord",
    "webex",
    "firefox",
    "chromium",
    "chromium-browser",
    "chrome",
    "google-chrome",
    "msedge",
    "microsoft-edge",
    "brave",
    "brave-browser",
];

/// Apps that hold the mic for anything (a dictation site, a voice note), so
/// they only count while a window title says a meeting is open. On Linux the
/// title check needs X11 (the probe documents the Wayland consequence).
pub const BROWSERS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "com.google.chrome",
    "com.microsoft.edgemac",
    "org.mozilla.firefox",
    "com.brave.browser",
    "com.apple.safari",
    "firefox",
    "chromium",
    "chromium-browser",
    "chrome",
    "google-chrome",
    "msedge",
    "microsoft-edge",
    "brave",
    "brave-browser",
];

/// Title fragments that mark a browser window as a meeting. Elevate's web
/// phone keeps the same title idle and on a call, but holds the mic only
/// during a call (seen on a live call, 2026-09-29), so the title plus the
/// mic is a call.
const MEETING_MARKERS: &[&str] = &["Meet -", "Meet –", "Microsoft Teams", "Zoom", "Elevate UC"];

/// Does a window title say a meeting is open? Titles are matched in memory
/// and never stored or logged.
pub fn title_has_meeting_marker(title: &str) -> bool {
    MEETING_MARKERS.iter().any(|m| title.contains(m))
}
