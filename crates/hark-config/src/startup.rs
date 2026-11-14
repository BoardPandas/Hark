use serde::{Deserialize, Serialize};

/// Launch behavior. `launch_at_login` is the source of truth for the OS
/// startup entry: `hark-autostart` reconciles the registry to it at startup
/// and after every Save. Default on, so a fresh install (and an upgrading
/// user whose config predates this section) starts with Windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Startup {
    pub launch_at_login: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Startup {
            launch_at_login: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn startup_defaults_to_launch_at_login_on_and_round_trips() {
        // Default on: fresh installs and pre-[startup] config files both
        // launch at login until the user opts out.
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.startup.launch_at_login);

        let s = Settings::from_toml("[startup]\nlaunch_at_login = false")
            .expect("startup section parses");
        assert!(!s.startup.launch_at_login);
        let text = s.to_toml().expect("serializes");
        assert!(!Settings::from_toml(&text).unwrap().startup.launch_at_login);
    }
}
