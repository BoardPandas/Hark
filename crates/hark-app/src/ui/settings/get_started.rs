//! First-run state: progress follows actual provider/model readiness.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Choose,
    Configure,
    Permissions,
    Try,
}

pub struct GetStarted {
    pub active: bool,
    pub dismissed: bool,
    pub step: Step,
    pub local: bool,
}

impl GetStarted {
    pub fn new(active: bool) -> Self {
        Self {
            active,
            dismissed: false,
            step: Step::Choose,
            local: false,
        }
    }

    pub fn visible(&self, injected: bool) -> bool {
        self.active && !self.dismissed && !injected
    }
}

pub fn ready(local: bool, model_ready: bool, provider_passed: bool) -> bool {
    if local {
        model_ready
    } else {
        provider_passed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_dictation_or_dismissal_retires_first_run() {
        assert!(!GetStarted::new(false).visible(false));
        let mut setup = GetStarted::new(true);
        assert!(setup.visible(false));
        setup.step = Step::Try;
        assert!(
            setup.visible(false),
            "the last screen is not a successful dictation"
        );
        assert!(!setup.visible(true));
        setup.dismissed = true;
        assert!(!setup.visible(false));
    }

    #[test]
    fn readiness_follows_the_selected_engine() {
        assert!(!ready(true, false, true));
        assert!(ready(true, true, false));
        assert!(!ready(false, true, false));
        assert!(ready(false, false, true));
    }
}
