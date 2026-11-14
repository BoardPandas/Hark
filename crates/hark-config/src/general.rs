use serde::{Deserialize, Serialize};

/// Main-window preferences. Defaults preserve the existing tray behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct General {
    pub always_on_top: bool,
    /// Exit on the window's close button; otherwise hide in the system tray.
    /// An explicit Quit / Close Program action always exits.
    pub exit_on_close: bool,
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn older_configs_keep_normal_window_and_close_to_tray() {
        for text in ["", "[startup]\nlaunch_at_login = false", "[general]"] {
            let settings = Settings::from_toml(text).unwrap();
            assert!(!settings.general.always_on_top);
            assert!(!settings.general.exit_on_close);
        }
        let partial = Settings::from_toml("[general]\nalways_on_top = true").unwrap();
        assert!(partial.general.always_on_top);
        assert!(!partial.general.exit_on_close);
    }
}
