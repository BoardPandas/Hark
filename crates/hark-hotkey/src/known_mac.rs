//! Common Mac shortcuts, verified against Apple's keyboard shortcut guide:
//! https://support.apple.com/en-us/102650
//! Win/Alt are the portable table tokens for Command/Option.
use crate::known::{KnownShortcut, Tier};

macro_rules! shortcuts {
    ($($chord:literal => $action:literal),* $(,)?) => {
        pub(super) const KNOWN_MAC: &[KnownShortcut] = &[
            $(KnownShortcut { chord: $chord, tier: Tier::SystemTaken, app: "macOS", action: $action }),*
        ];
    };
}
shortcuts! {
    "Win+Space" => "opens Spotlight",
    "Win+Alt+Space" => "opens a Finder search",
    "Win+Ctrl+Space" => "opens the character viewer",
    "Win+Tab" => "switches applications",
    "Win+Shift+Tab" => "switches applications in reverse order",
    "Win+Backtick" => "switches windows in the current application",
    "Win+Q" => "quits the focused application",
    "Win+Ctrl+Q" => "locks your screen",
    "Win+Shift+Q" => "opens the logout confirmation",
    "Win+Alt+Shift+Q" => "logs out immediately",
    "Win+H" => "hides the focused application",
    "Win+Alt+H" => "hides other applications",
    "Win+M" => "minimizes the focused window",
    "Win+W" => "closes the focused window",
    "Win+Alt+W" => "closes all windows in the focused application",
    "Win+A" => "selects all",
    "Win+C" => "copies the selection",
    "Win+X" => "cuts the selection",
    "Win+V" => "pastes the clipboard",
    "Win+Z" => "undoes the previous edit",
    "Win+Shift+Z" => "redoes the previous edit",
    "Win+F" => "opens Find",
    "Win+G" => "finds the next match",
    "Win+O" => "opens a file",
    "Win+P" => "opens Print",
    "Win+S" => "saves the document",
    "Win+T" => "opens a tab",
    "Win+Comma" => "opens application settings",
    "Win+Ctrl+F" => "toggles full-screen mode",
    "Win+Shift+3" => "takes a screenshot",
    "Win+Shift+4" => "starts screenshot selection",
    "Win+Shift+5" => "opens screenshot and screen recording controls",
    "Ctrl+Up" => "opens Mission Control",
    "Ctrl+Down" => "shows the focused application's windows",
    "Win+Backspace" => "moves selected Finder items to Trash",
    "CapsLock" => "toggles capitalization",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{known::lookup_in, PttChord};

    #[test]
    fn native_table_is_valid_unique_and_matches_both_modifier_sides() {
        let mut seen = std::collections::HashSet::new();
        for row in KNOWN_MAC {
            let mut tokens: Vec<_> = row.chord.split('+').collect();
            tokens.sort_unstable();
            assert!(seen.insert(tokens), "duplicate {}", row.chord);
            for side in ["L", "R"] {
                let spelling = row
                    .chord
                    .replace("Win", &format!("{side}Win"))
                    .replace("Ctrl", &format!("{side}Ctrl"))
                    .replace("Shift", &format!("{side}Shift"))
                    .replace("Alt", &format!("{side}Alt"));
                let chord = PttChord::parse(&spelling).unwrap();
                assert_eq!(lookup_in(chord.keys(), KNOWN_MAC).unwrap().chord, row.chord);
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn mac_lookup_does_not_claim_windows_shortcuts() {
        assert!(crate::known::lookup(PttChord::parse("LAlt+F4").unwrap().keys()).is_none());
        let command_q = PttChord::parse("LCmd+Q").unwrap();
        let hit = command_q.known_shortcut().unwrap();
        assert!(hit.action.contains("quits"));
        assert!(hit.message().contains("macOS"));
    }
}
