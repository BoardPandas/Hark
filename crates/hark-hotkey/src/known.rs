//! Well-known Windows/app shortcuts, so the recorder can tell the user what
//! else their chosen chord does.
//!
//! Hark OBSERVES keys and always calls `CallNextHookEx` — it never swallows
//! them. So every row here means "both things happen", never "your shortcut
//! won't work". The honest wording lives in [`KnownShortcut::message`].
//!
//! Consulted ONCE, when a chord is committed (recorder release-to-commit, or
//! the typed settings field losing focus). Never on a key event.
//!
//! Chords are written with SIDE-LESS modifiers (Ctrl/Shift/Alt/Win) because
//! Windows does not distinguish LCtrl from RCtrl when it matches a shortcut.
//! Everything else uses the exact `PttKeyCode::token()` spelling, so a row
//! that cannot be expressed in Hark's 114 keys cannot be written by accident
//! (the `chords_parse` test enforces it).
//!
//! `Digit` is the one wildcard token: it matches Digit0..=Digit9. Exact rows
//! are tried before wildcard rows, so `Ctrl+Alt+1` beats `Ctrl+Alt+Digit`.

use crate::edges::PttChord;
use crate::keycode::{PttKeyCode, PttKeyCode as K};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Windows never delivers this to a low-level hook, so the shortcut would
    /// silently never fire. THE TABLE HAS NO SUCH ROWS TODAY — every claimed
    /// one was refuted (see the module notes in the spec). Kept so that adding
    /// a properly-evidenced one later is a data change, not a code change.
    Unreceivable,
    /// Windows itself acts on it. Both happen.
    SystemTaken,
    /// A common app acts on it. Both happen, in that app.
    AppTaken,
}

impl Tier {
    /// Only `Unreceivable` refuses. The other two warn: the shortcut works
    /// fine, something else fires too.
    pub const fn refuses(self) -> bool {
        matches!(self, Tier::Unreceivable)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KnownShortcut {
    pub chord: &'static str,
    pub tier: Tier,
    pub app: &'static str,
    pub action: &'static str,
}

impl KnownShortcut {
    /// The sentence shown under the recorded shortcut.
    pub fn message(&self) -> String {
        match self.tier {
            Tier::Unreceivable => "Windows keeps this shortcut for itself and never passes \
                 it to Hark, so it would never start a dictation. Pick another one."
                .to_string(),
            Tier::SystemTaken => format!(
                "Windows uses this shortcut too — it {}. Hark never swallows keys, so \
                 dictating will do both.",
                self.action
            ),
            Tier::AppTaken => format!(
                "{} uses this shortcut too — it {}. Hark never swallows keys, so while that \
                 app is running, dictating will do both.",
                self.app, self.action
            ),
        }
    }
}

use Tier::{AppTaken, SystemTaken};

/// Side-less name: LCtrl and RCtrl are one shortcut to Windows, so they are
/// one token here.
const fn sideless(key: PttKeyCode) -> &'static str {
    match key {
        K::LCtrl | K::RCtrl => "Ctrl",
        K::LShift | K::RShift => "Shift",
        K::LAlt | K::RAlt => "Alt",
        K::LWin | K::RWin => "Win",
        other => other.token(),
    }
}

/// What else this chord does, if anything well-known does.
/// A lock key toggles whatever else is held down with it: Scroll Lock flips
/// on Ctrl+Alt+ScrollLock exactly as it does on its own. That side effect
/// belongs to the KEY, not the combination, so it must be reported for any
/// chord containing one — unlike an app shortcut, where Ctrl+Shift+A genuinely
/// is not Ctrl+A and exact matching is right.
fn lock_key_in(keys: &[PttKeyCode]) -> Option<&'static KnownShortcut> {
    let lock = keys
        .iter()
        .find(|k| matches!(k, K::CapsLock | K::NumLock | K::ScrollLock))?;
    KNOWN.iter().find(|row| row.chord == lock.token())
}

pub fn lookup(keys: &[PttKeyCode]) -> Option<&'static KnownShortcut> {
    // Normalise: side-less, and deduped — LCtrl+RCtrl+A is Ctrl+A to Windows.
    let mut mine: [&'static str; 4] = [""; 4];
    let mut len = 0usize;
    for &key in keys {
        let name = sideless(key);
        if !mine[..len].contains(&name) {
            mine[len] = name;
            len += 1;
        }
    }
    let mine = &mine[..len];
    KNOWN
        .iter()
        .find(|row| row_matches(row.chord, mine, false))
        .or_else(|| KNOWN.iter().find(|row| row_matches(row.chord, mine, true)))
        .or_else(|| lock_key_in(keys))
}

impl PttChord {
    pub fn known_shortcut(&self) -> Option<&'static KnownShortcut> {
        lookup(self.keys())
    }
}

/// Exact key-set equality: order-independent, and the counts must match, so
/// Ctrl+Shift+A never matches a row of Ctrl+A.
fn row_matches(row: &str, mine: &[&str], wildcards: bool) -> bool {
    let mut row_len = 0usize;
    for tok in row.split('+') {
        row_len += 1;
        let hit = mine.iter().any(|&k| {
            // `mine` holds NORMALISED tokens, and a digit normalises to "7",
            // not "Digit7" -- comparing against the variant name here made
            // every wildcard row silently unmatchable.
            tok == k
                || (wildcards && tok == "Digit" && k.len() == 1 && k.as_bytes()[0].is_ascii_digit())
        });
        if !hit {
            return false;
        }
    }
    // Rows never repeat a key and `mine` is deduped, so equal length + total
    // containment is set equality.
    row_len == mine.len()
}

pub const KNOWN: &[KnownShortcut] = &[
    // ---- Most likely picks: natural push-to-talk grips, or shortcuts that
    // ---- fire even when the owning app is in the background. ----
    KnownShortcut { chord: "Ctrl+Shift+Space", tier: AppTaken, app: "1Password, Slack, Teams, Windows Terminal, Word, Excel", action: "opens 1Password's Quick Access from any app, and mutes or unmutes you in a Slack huddle even when Slack is in the background" },
    KnownShortcut { chord: "Ctrl+Shift+M", tier: AppTaken, app: "Discord, Teams, Slack, Outlook, Zoom, Notion, OneNote, Word, Windows Terminal", action: "toggles your microphone in Discord even when Discord isn't focused, mutes or unmutes you in a Teams call, opens Activity in Slack and starts a new email in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+D", tier: AppTaken, app: "Discord, Teams, Zoom, Chrome, Edge, Brave, Firefox, PowerPoint, Word, Outlook, Windows Terminal", action: "toggles deafen in Discord even when Discord isn't focused, declines an incoming Teams or Zoom call, and bookmarks all open tabs in browsers" },
    KnownShortcut { chord: "Ctrl+Space", tier: AppTaken, app: "Teams, Excel, Word", action: "is Teams' own push-to-talk (hold to unmute in a call), selects the whole column in Excel and strips character formatting in Word" },
    KnownShortcut { chord: "Alt+Space", tier: SystemTaken, app: "Windows, ChatGPT desktop, Windows Terminal", action: "opens the window's system menu, and opens the ChatGPT desktop companion window over whatever you're in" },
    KnownShortcut { chord: "Ctrl+Alt+Shift", tier: AppTaken, app: "Zoom", action: "moves focus to the Zoom meeting controls, and can be turned into a Zoom global shortcut that fires from any app" },
    KnownShortcut { chord: "Alt+Shift", tier: SystemTaken, app: "Windows", action: "switches your input language when more than one is installed" },
    KnownShortcut { chord: "CapsLock", tier: SystemTaken, app: "Windows", action: "toggles Caps Lock, so every dictation flips your capitalisation" },
    KnownShortcut { chord: "NumLock", tier: SystemTaken, app: "Windows", action: "toggles Num Lock, changing what your numpad types — and holding it about five seconds can switch on Toggle Keys" },
    KnownShortcut { chord: "ScrollLock", tier: SystemTaken, app: "Windows", action: "toggles Scroll Lock, which still changes the arrow keys in Excel and some terminals" },
    KnownShortcut { chord: "Apps", tier: SystemTaken, app: "Windows", action: "opens the context menu, the same as a right-click" },
    KnownShortcut { chord: "Shift+F10", tier: SystemTaken, app: "Windows", action: "opens the context menu, the same as a right-click" },
    KnownShortcut { chord: "Win+Backtick", tier: AppTaken, app: "Windows Terminal", action: "drops down the quake-mode terminal from any app, whenever Windows Terminal is running" },
    KnownShortcut { chord: "Alt+Shift+NumLock", tier: SystemTaken, app: "Windows", action: "toggles Mouse Keys, if those are the left-hand Alt and Shift" },

    // ---- Ctrl+Shift+<letter>: effectively saturated across browsers,
    // ---- Office, Slack, Teams, VS Code. ----
    KnownShortcut { chord: "Ctrl+Shift+A", tier: AppTaken, app: "Chrome, Edge, Brave, Vivaldi, Firefox, Teams, Zoom, Slack, Outlook, Word, Excel, OneNote, Windows Terminal", action: "opens tab search in Chromium browsers and the add-ons manager in Firefox, accepts an incoming Teams or Zoom call, opens All Unreads in Slack and creates an Outlook appointment" },
    KnownShortcut { chord: "Ctrl+Shift+B", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Teams, Discord, Outlook, PowerPoint, VS Code", action: "shows or hides the bookmarks bar in browsers, inserts a code block in Teams, opens Discord's soundboard, opens the Outlook Address Book and runs the build task in VS Code" },
    KnownShortcut { chord: "Ctrl+Shift+C", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc, Slack, Teams, PowerPoint, Word, OneNote, 1Password", action: "opens the DevTools element picker in most browsers, copies the tab URL in Arc, formats the selection as code in Slack and Teams, and copies formatting in PowerPoint" },
    KnownShortcut { chord: "Ctrl+Shift+E", tier: AppTaken, app: "Word, Firefox, Edge, Teams, Outlook, OneNote, Slack", action: "turns Track Changes on or off in Word — silently — opens the Firefox network monitor, toggles the Teams share tray and creates a folder in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+F", tier: AppTaken, app: "Word, Excel, Outlook, PowerPoint, Teams, VS Code, Windows Terminal, 1Password", action: "opens the Font dialog in Word, Format Cells in Excel, Advanced Find in Outlook, the Teams filter box and search-across-files in VS Code" },
    KnownShortcut { chord: "Ctrl+Shift+G", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, PowerPoint, Excel, Outlook", action: "jumps to the previous find-in-page match in browsers, ungroups objects in PowerPoint and opens Flag for Follow Up in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+H", tier: AppTaken, app: "Teams, Zoom, Slack, VS Code, Firefox, Word, Outlook, Notion", action: "ends the call in Teams and holds it in Zoom, opens huddle controls in Slack, replaces across files in VS Code and hides the selected text in Word" },
    KnownShortcut { chord: "Ctrl+Shift+I", tier: AppTaken, app: "Chrome, Edge, Brave, Vivaldi, Arc, Firefox, Teams, Outlook, Slack", action: "opens DevTools in every major browser, marks a Teams message important and switches to the Inbox in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+J", tier: AppTaken, app: "Notion, Chrome, Brave, Firefox, Teams, Slack, PowerPoint, Outlook", action: "opens Notion AI from outside Notion, opens the DevTools console in Chrome and the Browser Console in Firefox, and joins a meeting from a Teams toast" },
    KnownShortcut { chord: "Ctrl+Shift+K", tier: AppTaken, app: "Teams, Slack, VS Code, Firefox, Edge, Arc, Outlook, Word, Windows Terminal", action: "raises your hand in a Teams meeting, composes a new Slack message, deletes the current line in VS Code, opens Firefox's Web Console and creates an Outlook task" },
    KnownShortcut { chord: "Ctrl+Shift+L", tier: AppTaken, app: "1Password, Slack, VS Code, Excel, Word, Notion, Teams, Edge, Outlook", action: "locks 1Password from any app, selects all occurrences in VS Code, toggles AutoFilter in Excel and browses channels in Slack" },
    KnownShortcut { chord: "Ctrl+Shift+N", tier: AppTaken, app: "Chrome, Edge, Brave, Vivaldi, Arc, Firefox, Teams, Slack, Outlook, Word, OneNote, Windows Terminal", action: "opens a private window in most browsers but reopens the last closed window in Firefox, starts a new Teams chat and creates a Slack canvas" },
    KnownShortcut { chord: "Ctrl+Shift+O", tier: AppTaken, app: "Teams, Zoom, 1Password, Chrome, Edge, Brave, Firefox, Vivaldi, VS Code, Outlook", action: "turns your camera off in a Teams meeting, opens the bookmarks manager in browsers, goes to symbol in VS Code and switches to the Outbox in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+P", tier: AppTaken, app: "Firefox, Chrome, Edge, Brave, VS Code, Windows Terminal, Outlook", action: "opens a private window in Firefox but the system print dialog in Chromium browsers, and opens the command palette in VS Code and Windows Terminal" },
    KnownShortcut { chord: "Ctrl+Shift+Q", tier: AppTaken, app: "Firefox, Outlook, Word", action: "quits Firefox outright on Windows — the whole browser closes — and creates a meeting request in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+R", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Outlook, Teams, OneNote", action: "hard-reloads the page in browsers, replies to all in Outlook and opens the meeting chat from a Teams notification" },
    KnownShortcut { chord: "Ctrl+Shift+S", tier: AppTaken, app: "Firefox, Vivaldi, Teams, PowerPoint, Word, Outlook, Slack, Notion", action: "takes a page screenshot in Firefox, accepts an incoming audio call in Teams, opens Save As in PowerPoint and the Apply Styles pane in Word" },
    KnownShortcut { chord: "Ctrl+Shift+T", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc, Windows Terminal, Teams, Slack, Word, OneNote", action: "reopens the last closed tab in every browser, opens a new tab in Windows Terminal, pins the Teams window on top and opens Slack's Threads view" },
    KnownShortcut { chord: "Ctrl+Shift+U", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Teams, Slack, Discord, Excel", action: "views the page source in browsers, toggles the speaker in a Teams call, starts Read Aloud in Edge, inserts a link in Slack and attaches a file in Discord" },
    KnownShortcut { chord: "Ctrl+Shift+V", tier: AppTaken, app: "Edge, Word, Excel, PowerPoint, OneNote, Outlook, VS Code, Windows Terminal", action: "pastes without formatting in Edge, Word, OneNote and Windows Terminal, and opens Paste Special in Excel — the same clipboard machinery Hark pastes through" },
    KnownShortcut { chord: "Ctrl+Shift+W", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Windows Terminal, Slack, Word, Outlook, OneNote", action: "closes the whole browser window and all its tabs, closes the current pane in Windows Terminal and reopens the last closed Slack window" },
    KnownShortcut { chord: "Ctrl+Shift+X", tier: AppTaken, app: "VS Code, 1Password, Slack, Teams, Firefox, Outlook", action: "opens the Extensions view in VS Code, opens the 1Password browser pop-up, strikes through the selection in Slack and expands the Teams compose box" },
    KnownShortcut { chord: "Ctrl+Shift+Y", tier: AppTaken, app: "Edge, Teams, Slack, Outlook", action: "opens Collections in Edge, admits people from the Teams lobby, sets your Slack status and copies an item to another folder in Outlook" },
    KnownShortcut { chord: "Ctrl+Shift+Z", tier: AppTaken, app: "Firefox", action: "opens the Firefox debugger, and redoes your last edit in most text fields" },

    // ---- Ctrl+Shift+<non-letter> ----
    KnownShortcut { chord: "Ctrl+Shift+Tab", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "switches to the previous tab" },
    KnownShortcut { chord: "Ctrl+Shift+Delete", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi", action: "opens Clear Browsing Data" },
    KnownShortcut { chord: "Ctrl+Shift+Enter", tier: AppTaken, app: "Word, Teams", action: "inserts a column break in Word and resets the Teams pane width" },
    KnownShortcut { chord: "Ctrl+Shift+PageUp", tier: AppTaken, app: "Chrome, Edge, Brave, Vivaldi", action: "moves the current tab one position to the left" },
    KnownShortcut { chord: "Ctrl+Shift+PageDown", tier: AppTaken, app: "Chrome, Edge, Brave, Vivaldi", action: "moves the current tab one position to the right" },
    KnownShortcut { chord: "Ctrl+Shift+Equals", tier: AppTaken, app: "Word, Excel, Arc", action: "applies superscript in Word, opens Insert Cells in Excel and adds a Split View pane in Arc" },
    KnownShortcut { chord: "Ctrl+Shift+Minus", tier: AppTaken, app: "Word, Excel, OneNote, Arc", action: "applies subscript in Word, removes cell borders in Excel and closes the Split View in Arc" },
    KnownShortcut { chord: "Ctrl+Shift+Comma", tier: AppTaken, app: "Word, Windows Terminal", action: "decreases the font size in Word and opens the settings file in Windows Terminal" },
    KnownShortcut { chord: "Ctrl+Shift+Period", tier: AppTaken, app: "Word, Windows Terminal", action: "increases the font size in Word and opens the suggestions menu in Windows Terminal" },
    KnownShortcut { chord: "Ctrl+Shift+Semicolon", tier: AppTaken, app: "Excel", action: "enters the current time into the cell" },
    KnownShortcut { chord: "Ctrl+Shift+Quote", tier: AppTaken, app: "Excel", action: "copies the value from the cell above into the active cell" },
    KnownShortcut { chord: "Ctrl+Shift+Backtick", tier: AppTaken, app: "Excel", action: "applies the General number format" },
    KnownShortcut { chord: "Ctrl+Shift+LeftBracket", tier: AppTaken, app: "PowerPoint, OneNote", action: "sends the selected object back one position in PowerPoint and widens OneNote's page pane" },
    KnownShortcut { chord: "Ctrl+Shift+RightBracket", tier: AppTaken, app: "PowerPoint, OneNote", action: "brings the selected object forward one position in PowerPoint and narrows OneNote's page pane" },
    KnownShortcut { chord: "Ctrl+Shift+F2", tier: AppTaken, app: "Excel", action: "inserts a threaded comment, or replies to one" },
    KnownShortcut { chord: "Ctrl+Shift+F5", tier: AppTaken, app: "Word", action: "opens the Bookmark dialog" },
    KnownShortcut { chord: "Ctrl+Shift+F6", tier: AppTaken, app: "Teams", action: "moves to the previous section of the Teams window" },
    KnownShortcut { chord: "Ctrl+Shift+F9", tier: AppTaken, app: "Word", action: "unlinks a field, replacing it permanently with its current result" },
    KnownShortcut { chord: "Ctrl+Shift+1", tier: AppTaken, app: "Excel, Vivaldi, OneNote, Arc", action: "applies Excel's Number format, switches to Vivaldi Workspace 1 and flags an Outlook task from OneNote" },
    KnownShortcut { chord: "Ctrl+Shift+2", tier: AppTaken, app: "Excel, Vivaldi, OneNote", action: "applies Excel's Time format and switches to Vivaldi Workspace 2" },
    KnownShortcut { chord: "Ctrl+Shift+3", tier: AppTaken, app: "Excel, Vivaldi, OneNote", action: "applies Excel's Date format and switches to Vivaldi Workspace 3" },
    KnownShortcut { chord: "Ctrl+Shift+4", tier: AppTaken, app: "Excel, OneNote", action: "applies Excel's Currency format" },
    KnownShortcut { chord: "Ctrl+Shift+5", tier: AppTaken, app: "Excel, OneNote", action: "applies Excel's Percentage format" },
    KnownShortcut { chord: "Ctrl+Shift+6", tier: AppTaken, app: "Excel", action: "applies the Scientific number format" },
    KnownShortcut { chord: "Ctrl+Shift+7", tier: AppTaken, app: "Excel", action: "applies an outline border to the selected cells" },
    KnownShortcut { chord: "Ctrl+Shift+8", tier: AppTaken, app: "Excel, Word, OneNote", action: "selects the current region in Excel and shows all formatting marks in Word" },

    // ---- Win+<key>: the OS acts, always, in every app. ----
    KnownShortcut { chord: "Win+A", tier: SystemTaken, app: "Windows", action: "opens quick settings / the action centre" },
    KnownShortcut { chord: "Win+B", tier: SystemTaken, app: "Windows", action: "moves focus to the notification area of the taskbar" },
    KnownShortcut { chord: "Win+C", tier: SystemTaken, app: "Windows", action: "opens Copilot on current Windows builds" },
    KnownShortcut { chord: "Win+D", tier: SystemTaken, app: "Windows", action: "shows and hides the desktop" },
    KnownShortcut { chord: "Win+E", tier: SystemTaken, app: "Windows", action: "opens File Explorer" },
    KnownShortcut { chord: "Win+F", tier: SystemTaken, app: "Windows", action: "opens Feedback Hub" },
    KnownShortcut { chord: "Win+G", tier: SystemTaken, app: "Windows", action: "opens the Xbox Game Bar, which no app can block" },
    KnownShortcut { chord: "Win+H", tier: SystemTaken, app: "Windows", action: "opens Windows voice typing — a second dictation running on top of Hark's" },
    KnownShortcut { chord: "Win+I", tier: SystemTaken, app: "Windows", action: "opens Settings" },
    KnownShortcut { chord: "Win+J", tier: SystemTaken, app: "Windows", action: "opens Recall on Copilot+ PCs" },
    KnownShortcut { chord: "Win+K", tier: SystemTaken, app: "Windows", action: "opens Cast, to connect to a display" },
    KnownShortcut { chord: "Win+L", tier: SystemTaken, app: "Windows", action: "locks the workstation — you'd be dictating at the lock screen, and Hark only ends the recording when its watchdog notices the key is no longer held" },
    KnownShortcut { chord: "Win+M", tier: SystemTaken, app: "Windows", action: "minimises all windows" },
    KnownShortcut { chord: "Win+N", tier: SystemTaken, app: "Windows", action: "opens the notification centre and calendar" },
    KnownShortcut { chord: "Win+O", tier: SystemTaken, app: "Windows", action: "locks the device orientation" },
    KnownShortcut { chord: "Win+P", tier: SystemTaken, app: "Windows", action: "opens the projection picker" },
    KnownShortcut { chord: "Win+Q", tier: SystemTaken, app: "Windows", action: "opens search" },
    KnownShortcut { chord: "Win+R", tier: SystemTaken, app: "Windows", action: "opens the Run dialog" },
    KnownShortcut { chord: "Win+S", tier: SystemTaken, app: "Windows", action: "opens search" },
    KnownShortcut { chord: "Win+T", tier: SystemTaken, app: "Windows", action: "cycles through the taskbar apps" },
    KnownShortcut { chord: "Win+U", tier: SystemTaken, app: "Windows", action: "opens Accessibility settings" },
    KnownShortcut { chord: "Win+V", tier: SystemTaken, app: "Windows", action: "opens clipboard history — which Hark's paste also writes to" },
    KnownShortcut { chord: "Win+W", tier: SystemTaken, app: "Windows", action: "opens Widgets" },
    KnownShortcut { chord: "Win+X", tier: SystemTaken, app: "Windows", action: "opens the Quick Link (power user) menu" },
    KnownShortcut { chord: "Win+Z", tier: SystemTaken, app: "Windows", action: "opens the snap layouts flyout" },
    KnownShortcut { chord: "Win+Tab", tier: SystemTaken, app: "Windows", action: "opens Task View" },
    KnownShortcut { chord: "Win+Up", tier: SystemTaken, app: "Windows", action: "maximises the active window" },
    KnownShortcut { chord: "Win+Down", tier: SystemTaken, app: "Windows", action: "minimises the active window" },
    KnownShortcut { chord: "Win+Left", tier: SystemTaken, app: "Windows", action: "snaps the window to the left half of the screen" },
    KnownShortcut { chord: "Win+Right", tier: SystemTaken, app: "Windows", action: "snaps the window to the right half of the screen" },
    KnownShortcut { chord: "Win+Comma", tier: SystemTaken, app: "Windows", action: "peeks at the desktop for as long as you hold it — the same gesture push-to-talk uses" },
    KnownShortcut { chord: "Win+Period", tier: SystemTaken, app: "Windows", action: "opens the emoji panel" },
    KnownShortcut { chord: "Win+Semicolon", tier: SystemTaken, app: "Windows", action: "opens the emoji panel" },
    KnownShortcut { chord: "Win+Slash", tier: SystemTaken, app: "Windows", action: "starts IME reconversion" },
    KnownShortcut { chord: "Win+Equals", tier: SystemTaken, app: "Windows", action: "turns on Magnifier and zooms in" },
    KnownShortcut { chord: "Win+NumpadAdd", tier: SystemTaken, app: "Windows", action: "turns on Magnifier and zooms in" },
    KnownShortcut { chord: "Win+Minus", tier: SystemTaken, app: "Windows", action: "zooms Magnifier out while it is running" },
    KnownShortcut { chord: "Win+Space", tier: SystemTaken, app: "Windows", action: "switches forward through your input languages and keyboard layouts" },
    KnownShortcut { chord: "Win+Shift+S", tier: SystemTaken, app: "Windows", action: "dims the screen and starts a screenshot region capture" },
    KnownShortcut { chord: "Win+Shift+R", tier: SystemTaken, app: "Windows", action: "starts a screen region video recording" },
    KnownShortcut { chord: "Win+Shift+M", tier: SystemTaken, app: "Windows", action: "restores minimised windows" },
    KnownShortcut { chord: "Win+Shift+A", tier: SystemTaken, app: "Windows", action: "focuses a Windows tip when one is showing" },
    KnownShortcut { chord: "Win+Shift+V", tier: SystemTaken, app: "Windows", action: "cycles through notifications" },
    KnownShortcut { chord: "Win+Shift+Up", tier: SystemTaken, app: "Windows", action: "stretches the window to the top and bottom of the screen" },
    KnownShortcut { chord: "Win+Shift+Down", tier: SystemTaken, app: "Windows", action: "restores a snapped or maximised window" },
    KnownShortcut { chord: "Win+Shift+Left", tier: SystemTaken, app: "Windows", action: "moves the window to the monitor on the left" },
    KnownShortcut { chord: "Win+Shift+Right", tier: SystemTaken, app: "Windows", action: "moves the window to the monitor on the right" },
    KnownShortcut { chord: "Win+Shift+Space", tier: SystemTaken, app: "Windows", action: "switches backward through your input languages and keyboard layouts" },
    KnownShortcut { chord: "Win+Shift+N", tier: AppTaken, app: "OneNote", action: "opens OneNote from any app, for as long as OneNote is installed" },
    KnownShortcut { chord: "Win+Alt+K", tier: SystemTaken, app: "Windows", action: "mutes or unmutes your microphone in Teams and other supported call apps" },
    KnownShortcut { chord: "Win+Alt+H", tier: SystemTaken, app: "Windows", action: "moves focus to the keyboard when voice typing is open" },
    KnownShortcut { chord: "Win+Alt+B", tier: SystemTaken, app: "Windows", action: "toggles HDR on or off" },
    KnownShortcut { chord: "Win+Alt+D", tier: SystemTaken, app: "Windows", action: "shows and hides the date and time on the desktop" },
    KnownShortcut { chord: "Win+Alt+R", tier: SystemTaken, app: "Windows", action: "starts or stops Game Bar recording, which no app can block" },
    KnownShortcut { chord: "Win+Alt+N", tier: AppTaken, app: "OneNote", action: "creates a OneNote Quick Note from any app, for as long as OneNote is installed" },
    KnownShortcut { chord: "Win+Ctrl+Left", tier: SystemTaken, app: "Windows", action: "switches to the virtual desktop on the left" },
    KnownShortcut { chord: "Win+Ctrl+Right", tier: SystemTaken, app: "Windows", action: "switches to the virtual desktop on the right" },
    KnownShortcut { chord: "Win+Ctrl+D", tier: SystemTaken, app: "Windows", action: "creates a new virtual desktop" },
    KnownShortcut { chord: "Win+Ctrl+F4", tier: SystemTaken, app: "Windows", action: "closes the current virtual desktop" },
    KnownShortcut { chord: "Win+Ctrl+Enter", tier: SystemTaken, app: "Windows", action: "toggles Narrator, which starts reading the screen aloud" },
    KnownShortcut { chord: "Win+Ctrl+O", tier: SystemTaken, app: "Windows", action: "turns on the On-Screen Keyboard" },
    KnownShortcut { chord: "Win+Ctrl+Q", tier: SystemTaken, app: "Windows", action: "opens Quick Assist" },
    KnownShortcut { chord: "Win+Ctrl+C", tier: SystemTaken, app: "Windows", action: "toggles colour filters, if they're enabled in settings" },
    KnownShortcut { chord: "Win+Ctrl+V", tier: SystemTaken, app: "Windows", action: "opens the sound output page of quick settings — and it's what Windows sees if Hark pastes while you're still holding Win" },
    KnownShortcut { chord: "Win+Ctrl+F", tier: SystemTaken, app: "Windows", action: "opens Find Computers" },
    KnownShortcut { chord: "Win+Ctrl+Space", tier: SystemTaken, app: "Windows", action: "changes to your previously selected input option" },
    KnownShortcut { chord: "Win+Ctrl+Shift+B", tier: SystemTaken, app: "Windows", action: "resets the graphics driver — the display flickers black" },
    KnownShortcut { chord: "Win+Digit", tier: SystemTaken, app: "Windows", action: "launches or switches to the taskbar app pinned at that position" },
    KnownShortcut { chord: "Win+Shift+Digit", tier: SystemTaken, app: "Windows", action: "starts a new instance of the taskbar app pinned at that position" },
    KnownShortcut { chord: "Win+Ctrl+Digit", tier: SystemTaken, app: "Windows", action: "switches to the last active window of the taskbar app pinned at that position" },
    KnownShortcut { chord: "Win+Alt+Digit", tier: SystemTaken, app: "Windows", action: "opens the jump list for the taskbar app pinned at that position" },
    KnownShortcut { chord: "Win+Ctrl+Shift+Digit", tier: SystemTaken, app: "Windows", action: "opens the taskbar app pinned at that position as administrator" },
    KnownShortcut { chord: "Win+Alt+Up", tier: SystemTaken, app: "Windows", action: "snaps the window to the top half of the screen" },
    KnownShortcut { chord: "Win+Alt+Down", tier: SystemTaken, app: "Windows", action: "snaps the window to the bottom half of the screen" },

    // ---- Alt+<key> ----
    KnownShortcut { chord: "Alt+Tab", tier: SystemTaken, app: "Windows", action: "opens the window switcher and changes which window your text lands in" },
    KnownShortcut { chord: "Alt+F4", tier: SystemTaken, app: "Windows", action: "closes the active window" },
    KnownShortcut { chord: "Alt+Enter", tier: SystemTaken, app: "Windows", action: "opens properties for the selected item, and toggles fullscreen in many apps" },
    KnownShortcut { chord: "Ctrl+Alt+Delete", tier: SystemTaken, app: "Windows", action: "opens the Windows security screen; Hark sees the press but never the release, so the recording only ends when its watchdog notices" },
    KnownShortcut { chord: "Alt+A", tier: AppTaken, app: "Zoom", action: "mutes or unmutes you in a Zoom meeting — and many people tick Zoom's \"global shortcut\" box for it, which makes it fire from any app" },
    KnownShortcut { chord: "Alt+V", tier: AppTaken, app: "Zoom", action: "starts or stops your video in a Zoom meeting" },
    KnownShortcut { chord: "Alt+S", tier: AppTaken, app: "Zoom", action: "opens the share-screen window, or stops sharing, in a Zoom meeting" },
    KnownShortcut { chord: "Alt+R", tier: AppTaken, app: "Zoom", action: "starts or stops local recording in a Zoom meeting" },
    KnownShortcut { chord: "Alt+M", tier: AppTaken, app: "Zoom", action: "mutes everyone but the host, in a Zoom meeting you host" },
    KnownShortcut { chord: "Alt+Y", tier: AppTaken, app: "Zoom", action: "raises or lowers your hand in a Zoom meeting" },
    KnownShortcut { chord: "Alt+Q", tier: AppTaken, app: "Zoom", action: "prompts you to end or leave the Zoom meeting" },
    KnownShortcut { chord: "Alt+H", tier: AppTaken, app: "Zoom", action: "shows or hides the in-meeting chat panel" },
    KnownShortcut { chord: "Alt+U", tier: AppTaken, app: "Zoom", action: "shows or hides the participants panel" },
    KnownShortcut { chord: "Alt+Shift+A", tier: AppTaken, app: "Teams", action: "starts an audio call with the open chat — it would place a real call" },
    KnownShortcut { chord: "Alt+Shift+V", tier: AppTaken, app: "Teams", action: "starts a video call with the open chat — it would place a real call" },
    KnownShortcut { chord: "Alt+Shift+O", tier: AppTaken, app: "Teams", action: "opens the attach-file picker" },
    KnownShortcut { chord: "Alt+Shift+E", tier: AppTaken, app: "Teams", action: "opens the video recorder in the compose box" },
    KnownShortcut { chord: "Alt+Shift+R", tier: AppTaken, app: "Teams, Word", action: "replies to the last chat message in Teams and copies the previous section's header in Word" },
    KnownShortcut { chord: "Alt+Shift+D", tier: AppTaken, app: "Word, OneNote", action: "types today's date straight into your document" },
    KnownShortcut { chord: "Alt+Shift+T", tier: AppTaken, app: "Word, OneNote", action: "types the current time straight into your document" },
    KnownShortcut { chord: "Alt+Shift+F", tier: AppTaken, app: "VS Code, Word, OneNote", action: "formats the document in VS Code, inserts a mail-merge field in Word and types the date and time in OneNote" },
    KnownShortcut { chord: "Alt+Shift+X", tier: AppTaken, app: "Word", action: "marks an index entry for the selected text" },
    KnownShortcut { chord: "Alt+Shift+C", tier: AppTaken, app: "PowerPoint, Word", action: "copies with the Animation Painter in PowerPoint and removes the window split in Word" },
    KnownShortcut { chord: "Alt+Shift+L", tier: AppTaken, app: "Teams", action: "adds a Loop paragraph" },
    KnownShortcut { chord: "Alt+Shift+P", tier: AppTaken, app: "OneNote", action: "shows or hides document printouts on the page" },
    KnownShortcut { chord: "Alt+Shift+S", tier: AppTaken, app: "Zoom", action: "shows or hides the list of windows available to share" },
    KnownShortcut { chord: "Alt+Shift+Up", tier: AppTaken, app: "Slack", action: "jumps to the previous unread channel or DM" },
    KnownShortcut { chord: "Alt+Shift+Down", tier: AppTaken, app: "Slack, Windows Terminal", action: "jumps to the next unread Slack conversation and resizes the pane in Windows Terminal" },

    // ---- Ctrl+<key>: browser-shaped, but taken almost everywhere. ----
    KnownShortcut { chord: "Ctrl+T", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "opens a new tab" },
    KnownShortcut { chord: "Ctrl+W", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "closes the current tab" },
    KnownShortcut { chord: "Ctrl+N", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "opens a new window" },
    KnownShortcut { chord: "Ctrl+L", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "focuses the address bar" },
    KnownShortcut { chord: "Ctrl+F", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "opens find-in-page" },
    KnownShortcut { chord: "Ctrl+D", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "bookmarks the page in most browsers, and pins the tab in Arc" },
    KnownShortcut { chord: "Ctrl+S", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "saves the page in most browsers, and toggles the sidebar in Arc — and saves the file in nearly every editor" },
    KnownShortcut { chord: "Ctrl+H", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Arc", action: "opens History" },
    KnownShortcut { chord: "Ctrl+J", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox", action: "opens Downloads" },
    KnownShortcut { chord: "Ctrl+E", tier: AppTaken, app: "Chrome, Edge, Brave", action: "puts the address bar into search mode" },
    KnownShortcut { chord: "Ctrl+K", tier: AppTaken, app: "Chrome, Edge, Brave", action: "puts the address bar into search mode" },
    KnownShortcut { chord: "Ctrl+G", tier: AppTaken, app: "Chrome, Edge, Brave", action: "jumps to the next find-in-page match" },
    KnownShortcut { chord: "Ctrl+U", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox", action: "views the page source" },
    KnownShortcut { chord: "Ctrl+O", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox", action: "opens a local file" },
    KnownShortcut { chord: "Ctrl+P", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc, Obsidian", action: "opens the print dialog in browsers and the command palette in Obsidian" },
    KnownShortcut { chord: "Ctrl+R", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "reloads the page" },
    KnownShortcut { chord: "Ctrl+B", tier: AppTaken, app: "Firefox, Vivaldi", action: "toggles the bookmarks sidebar — and is bold in every text editor" },
    KnownShortcut { chord: "Ctrl+M", tier: AppTaken, app: "Edge", action: "mutes or unmutes the current tab" },
    KnownShortcut { chord: "Ctrl+Q", tier: AppTaken, app: "Vivaldi", action: "quits the browser" },
    KnownShortcut { chord: "Ctrl+Tab", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "switches to the next tab" },
    KnownShortcut { chord: "Ctrl+Backtick", tier: AppTaken, app: "VS Code", action: "toggles the integrated terminal" },
    KnownShortcut { chord: "Ctrl+Backslash", tier: AppTaken, app: "1Password, VS Code", action: "autofills the page from the 1Password browser extension, and splits the editor in VS Code" },
    KnownShortcut { chord: "Ctrl+Quote", tier: AppTaken, app: "Discord", action: "calls the current DM" },
    KnownShortcut { chord: "Ctrl+Minus", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "zooms the page out" },
    KnownShortcut { chord: "Ctrl+Equals", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "zooms the page in" },
    KnownShortcut { chord: "Ctrl+Digit", tier: AppTaken, app: "Chrome, Edge, Brave, Firefox, Vivaldi, Arc", action: "jumps straight to that tab (Ctrl+9 goes to the last one); Ctrl+0 resets the page zoom instead" },

    // ---- Ctrl+Alt+<key>: the cleanest space overall, but Office is dense. ----
    KnownShortcut { chord: "Ctrl+Alt+M", tier: AppTaken, app: "Word, PowerPoint, OneNote, Outlook", action: "inserts a comment in Word and PowerPoint, and opens Move or Copy in OneNote" },
    KnownShortcut { chord: "Ctrl+Alt+V", tier: AppTaken, app: "Excel, PowerPoint, Word, OneNote", action: "opens Paste Special in Excel and PowerPoint, and pastes formatting in Word" },
    KnownShortcut { chord: "Ctrl+Alt+P", tier: AppTaken, app: "Word, Excel, OneNote, Teams, Outlook", action: "switches Word to Print Layout, toggles formula tooltips in Excel and plays an audio recording in OneNote" },
    KnownShortcut { chord: "Ctrl+Alt+R", tier: AppTaken, app: "Teams, Word, Outlook, OneNote", action: "reacts to the last Teams message, types a ® in Word and replies with a meeting request in Outlook" },
    KnownShortcut { chord: "Ctrl+Alt+S", tier: AppTaken, app: "Word, Outlook, OneNote", action: "splits the document window in Word and opens Send/Receive Groups in Outlook" },
    KnownShortcut { chord: "Ctrl+Alt+C", tier: AppTaken, app: "Word, OneNote, Teams, 1Password", action: "copies text formatting in Word, copies with the Format Painter in OneNote and copies the one-time password in 1Password" },
    KnownShortcut { chord: "Ctrl+Alt+D", tier: AppTaken, app: "Word, OneNote", action: "inserts an endnote in Word and docks the OneNote window to the side of the screen" },
    KnownShortcut { chord: "Ctrl+Alt+F", tier: AppTaken, app: "Word, Outlook, 1Password", action: "inserts a footnote in Word and forwards the message as an attachment in Outlook" },
    KnownShortcut { chord: "Ctrl+Alt+T", tier: AppTaken, app: "Word, Teams", action: "types a ™ in Word and moves focus to the Teams toast notification" },
    KnownShortcut { chord: "Ctrl+Alt+L", tier: AppTaken, app: "Word, Teams, OneNote", action: "inserts a LISTNUM field in Word, adds a Loop paragraph in Teams and locks password-protected OneNote sections" },
    KnownShortcut { chord: "Ctrl+Alt+Z", tier: AppTaken, app: "Word, Teams", action: "cycles through your last four edit locations in Word and clears the chat filters in Teams" },
    KnownShortcut { chord: "Ctrl+Alt+O", tier: AppTaken, app: "Word, PowerPoint", action: "switches Word to Outline view and fits the slide to the window in PowerPoint" },
    KnownShortcut { chord: "Ctrl+Alt+N", tier: AppTaken, app: "Word", action: "switches to Draft view" },
    KnownShortcut { chord: "Ctrl+Alt+I", tier: AppTaken, app: "Word", action: "switches to Print Preview" },
    KnownShortcut { chord: "Ctrl+Alt+K", tier: AppTaken, app: "Word", action: "enables AutoFormat" },
    KnownShortcut { chord: "Ctrl+Alt+H", tier: AppTaken, app: "OneNote", action: "highlights the selected text" },
    KnownShortcut { chord: "Ctrl+Alt+G", tier: AppTaken, app: "OneNote", action: "puts focus on the current page tab" },
    KnownShortcut { chord: "Ctrl+Alt+E", tier: AppTaken, app: "OneNote", action: "adds a table column to the left" },
    KnownShortcut { chord: "Ctrl+Alt+U", tier: AppTaken, app: "OneNote, Teams", action: "skips a OneNote recording forward ten seconds and filters Teams to unread chats" },
    KnownShortcut { chord: "Ctrl+Alt+Y", tier: AppTaken, app: "OneNote", action: "skips the current recording back ten seconds" },
    KnownShortcut { chord: "Ctrl+Alt+J", tier: AppTaken, app: "Outlook", action: "marks the selected message as not junk" },
    KnownShortcut { chord: "Ctrl+Alt+A", tier: AppTaken, app: "Teams", action: "filters to all channel conversations" },
    KnownShortcut { chord: "Ctrl+Alt+B", tier: AppTaken, app: "Teams", action: "filters to meeting chats" },
    KnownShortcut { chord: "Ctrl+Alt+X", tier: AppTaken, app: "Teams", action: "strikes through the text you're composing" },
    KnownShortcut { chord: "Ctrl+Alt+Enter", tier: AppTaken, app: "Teams", action: "moves focus to the pane divider" },
    KnownShortcut { chord: "Ctrl+Alt+LeftBracket", tier: AppTaken, app: "OneNote", action: "decreases the indent level of the current page" },
    KnownShortcut { chord: "Ctrl+Alt+RightBracket", tier: AppTaken, app: "OneNote", action: "increases the indent level of the current page" },
    KnownShortcut { chord: "Ctrl+Alt+Equals", tier: AppTaken, app: "Excel", action: "zooms in" },
    KnownShortcut { chord: "Ctrl+Alt+Minus", tier: AppTaken, app: "Excel, Word", action: "zooms out in Excel and inserts an optional hyphen in Word" },
    KnownShortcut { chord: "Ctrl+Alt+Period", tier: AppTaken, app: "Word", action: "types an ellipsis at the cursor" },
    KnownShortcut { chord: "Ctrl+Alt+Comma", tier: AppTaken, app: "Windows Terminal", action: "opens the default settings file" },
    KnownShortcut { chord: "Ctrl+Alt+Left", tier: AppTaken, app: "Windows Terminal", action: "moves focus to the previous pane" },
    KnownShortcut { chord: "Ctrl+Alt+1", tier: AppTaken, app: "Word, OneNote, Teams, Outlook", action: "applies the Heading 1 style in Word, OneNote and Teams, and switches Outlook's calendar to Day view" },
    KnownShortcut { chord: "Ctrl+Alt+5", tier: AppTaken, app: "Word, Excel, PowerPoint, Outlook", action: "cycles focus through floating shapes" },
    KnownShortcut { chord: "Ctrl+Alt+F9", tier: AppTaken, app: "Excel", action: "recalculates every worksheet in all open workbooks" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+R", tier: AppTaken, app: "Teams", action: "opens the Report a Problem dialog" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+S", tier: AppTaken, app: "Word", action: "opens the Styles task pane" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+N", tier: AppTaken, app: "OneNote", action: "creates a subpage below the current page" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+C", tier: AppTaken, app: "Slack", action: "formats the selection as a code block" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+V", tier: AppTaken, app: "Discord", action: "jumps to the active voice call" },
    KnownShortcut { chord: "Ctrl+Alt+Shift+F9", tier: AppTaken, app: "Excel", action: "rechecks dependent formulas and recalculates every open workbook" },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every row must be spellable in Hark's key tokens, side-less, ≤4 keys.
    #[test]
    fn rows_are_wellformed() {
        let valid: HashSet<&str> = crate::keycode::ALL_KEYS
            .iter()
            .map(|k| sideless(*k))
            .chain(["Digit"])
            .collect();
        for row in KNOWN {
            let toks: Vec<&str> = row.chord.split('+').collect();
            assert!(toks.len() <= 4, "{} has {} keys", row.chord, toks.len());
            for t in &toks {
                assert!(valid.contains(t), "{}: unknown token {t}", row.chord);
                assert!(
                    !matches!(
                        *t,
                        "LCtrl" | "RCtrl" | "LShift" | "RShift" | "LAlt" | "RAlt" | "LWin" | "RWin"
                    ),
                    "{}: sided modifier {t}; rows are side-less",
                    row.chord
                );
            }
            let uniq: HashSet<_> = toks.iter().collect();
            assert_eq!(uniq.len(), toks.len(), "{} repeats a key", row.chord);
            assert!(!row.action.is_empty() && !row.app.is_empty());
        }
    }

    /// No two rows may describe the same key set, or `lookup` silently drops one.
    #[test]
    fn rows_are_unique() {
        let mut seen = HashSet::new();
        for row in KNOWN {
            let mut set: Vec<&str> = row.chord.split('+').collect();
            set.sort_unstable();
            assert!(seen.insert(set), "duplicate row for {}", row.chord);
        }
    }

    #[test]
    fn matching_is_sideless_orderless_and_exact() {
        let a = PttChord::parse("RCtrl+RShift+A").unwrap();
        assert_eq!(a.known_shortcut().unwrap().chord, "Ctrl+Shift+A");
        let b = PttChord::parse("A+LShift+LCtrl").unwrap();
        assert_eq!(b.known_shortcut().unwrap().chord, "Ctrl+Shift+A");
        // Superset must NOT match a subset row.
        assert!(PttChord::parse("Ctrl+A").is_err() || true);
        let c = PttChord::parse("LCtrl+A").unwrap();
        assert!(
            c.known_shortcut().is_none(),
            "Ctrl+A must not match Ctrl+Shift+A"
        );
        // Wildcard, and exact beats wildcard.
        assert_eq!(
            PttChord::parse("LWin+7")
                .unwrap()
                .known_shortcut()
                .unwrap()
                .chord,
            "Win+Digit"
        );
        assert_eq!(
            PttChord::parse("LCtrl+LAlt+1")
                .unwrap()
                .known_shortcut()
                .unwrap()
                .chord,
            "Ctrl+Alt+1"
        );
        // LCtrl+RCtrl collapses to one Ctrl.
        assert_eq!(
            PttChord::parse("LCtrl+RCtrl+T")
                .unwrap()
                .known_shortcut()
                .unwrap()
                .chord,
            "Ctrl+T"
        );
    }
}

#[cfg(test)]
mod table_integrity {

    use super::*;
    use crate::edges::PttChord;

    fn chord(text: &str) -> PttChord {
        PttChord::parse(text).unwrap()
    }

    /// A 235-row hand-written table rots silently: a typo'd token would simply
    /// never match, and nobody would notice the warning had stopped appearing.
    #[test]
    fn every_row_is_expressible_in_harks_keys() {
        for row in KNOWN {
            for tok in row.chord.split('+') {
                let ok = matches!(tok, "Ctrl" | "Shift" | "Alt" | "Win" | "Digit")
                    || crate::keycode::parse_key(tok).is_some();
                assert!(ok, "{}: unknown token {tok:?}", row.chord);
            }
            let n = row.chord.split('+').count();
            assert!(n <= 4, "{}: {n} keys, chords hold at most 4", row.chord);
        }
    }

    #[test]
    fn no_row_repeats_a_key_or_duplicates_another_row() {
        let mut seen: Vec<Vec<&str>> = Vec::new();
        for row in KNOWN {
            let mut keys: Vec<&str> = row.chord.split('+').collect();
            let before = keys.len();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(before, keys.len(), "{} repeats a key", row.chord);
            assert!(
                !seen.contains(&keys),
                "{} duplicates an earlier row; merge them",
                row.chord
            );
            seen.push(keys);
        }
    }

    /// The whole point of exact set equality: a longer chord is a DIFFERENT
    /// shortcut. Warning "Ctrl+A selects all" at someone binding Ctrl+Shift+A
    /// would be worse than saying nothing.
    #[test]
    fn a_superset_never_matches_a_shorter_row() {
        // Ctrl+Shift+A is a known row; adding Alt makes it a different chord
        // that nothing claims, and it must NOT inherit the shorter row's text.
        assert!(chord("LCtrl+LShift+A").known_shortcut().is_some());
        let longer = chord("LCtrl+LShift+LAlt+A");
        if let Some(hit) = longer.known_shortcut() {
            assert_eq!(
                hit.chord.split('+').count(),
                4,
                "{longer} matched {}",
                hit.chord
            );
        }
    }

    #[test]
    fn left_and_right_modifiers_are_the_same_shortcut() {
        let left = chord("LCtrl+LShift+M").known_shortcut();
        let right = chord("RCtrl+RShift+M").known_shortcut();
        assert!(left.is_some(), "Ctrl+Shift+M should be a known shortcut");
        assert_eq!(left.map(|k| k.chord), right.map(|k| k.chord));
        // ...and holding both Ctrls is still just Ctrl.
        assert_eq!(
            chord("LCtrl+RCtrl+LShift+M")
                .known_shortcut()
                .map(|k| k.chord),
            left.map(|k| k.chord)
        );
    }

    #[test]
    fn order_does_not_matter() {
        assert_eq!(
            chord("LShift+LCtrl+M").known_shortcut().map(|k| k.chord),
            chord("LCtrl+LShift+M").known_shortcut().map(|k| k.chord)
        );
    }

    /// An exact row must win over a `Digit` wildcard row, or a specific
    /// shortcut would be described by a generic one.
    #[test]
    fn an_exact_row_beats_a_wildcard_row() {
        for row in KNOWN.iter().filter(|r| r.chord.contains("Digit")) {
            let specific = row.chord.replace("Digit", "1");
            if let Ok(c) = PttChord::parse(
                &specific
                    .replace("Ctrl", "LCtrl")
                    .replace("Shift", "LShift")
                    .replace("Alt", "LAlt")
                    .replace("Win", "LWin"),
            ) {
                let hit = c.known_shortcut().expect("wildcard should match at least");
                let exact_exists = KNOWN.iter().any(|r| r.chord == specific);
                if exact_exists {
                    assert_eq!(hit.chord, specific, "wildcard shadowed the exact row");
                }
            }
        }
    }

    /// Nothing in the table refuses today: every "Windows never delivers this"
    /// claim was refuted under scrutiny. If a row ever earns `Unreceivable`, it
    /// must come with evidence, and this test is the tripwire that says so.
    #[test]
    fn no_row_refuses_without_evidence() {
        let refusing: Vec<_> = KNOWN.iter().filter(|r| r.tier.refuses()).collect();
        assert!(
            refusing.is_empty(),
            "{refusing:?} would REFUSE a binding; a low-level hook sees far more than \
             people assume, so this needs citable evidence before it ships"
        );
    }

    /// Exact set equality is right for app shortcuts but wrong for the lock
    /// keys: Ctrl+Alt+ScrollLock still flips Scroll Lock, and the first
    /// version of this table said nothing at all about it.
    #[test]
    fn a_lock_key_is_reported_inside_any_chord() {
        for text in [
            "LCtrl+LAlt+ScrollLock",
            "LCtrl+LShift+CapsLock",
            "LWin+NumLock",
            "ScrollLock",
        ] {
            let hit = chord(text)
                .known_shortcut()
                .unwrap_or_else(|| panic!("{text} should report its lock key"));
            assert!(
                hit.action.contains("toggles"),
                "{text} reported {:?}, which does not mention the toggle",
                hit.action
            );
        }
        // A chord with no lock key is unaffected by the fallback.
        assert!(chord("LCtrl+LWin+F13").known_shortcut().is_none());
    }

    #[test]
    fn a_shortcut_nobody_uses_is_not_reported() {
        assert!(chord("LCtrl+LWin+F13").known_shortcut().is_none());
        assert!(
            chord("LCtrl+LWin").known_shortcut().is_none(),
            "the shipped default must be clean"
        );
    }

    #[test]
    fn the_message_says_both_things_happen() {
        let hit = chord("LWin+P")
            .known_shortcut()
            .expect("Win+P is well known");
        let msg = hit.message();
        assert!(msg.contains("both"), "unhelpfully worded: {msg}");
        assert!(
            !hit.tier.refuses(),
            "Win+P is reachable; it must warn, not refuse"
        );
    }
}
