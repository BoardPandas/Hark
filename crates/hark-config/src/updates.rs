use serde::{Deserialize, Serialize};

/// In-app update behavior. The check hits the GitHub Releases API (network),
/// so it is user-controllable; the default is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// Check GitHub for a newer release once at startup and surface a banner.
    pub check_on_startup: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Updates {
            check_on_startup: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Settings;

    #[test]
    fn updates_default_to_startup_check_on_and_round_trip() {
        let s = Settings::from_toml("").expect("empty TOML parses");
        assert!(s.updates.check_on_startup);

        let s = Settings::from_toml("[updates]\ncheck_on_startup = false")
            .expect("updates section parses");
        assert!(!s.updates.check_on_startup);
        let text = s.to_toml().expect("serializes");
        assert!(!Settings::from_toml(&text).unwrap().updates.check_on_startup);
    }
}
