//! Launch-at-login for Hark, app-managed.
//!
//! Windows: a value named [`RUN_VALUE_NAME`] under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` whose data is the
//! quoted current-exe path plus [`HIDDEN_FLAG`]. The Settings toggle drives
//! [`reconcile`]: enabling writes/overwrites the value (self-healing a stale
//! path after an in-place upgrade), disabling deletes it.
//!
//! We touch only the Run *value*, never the `StartupApproved\Run` flag Windows
//! uses to record a Task Manager "disable". A user who turns Hark off in Task
//! Manager therefore stays in control: our value is still present but Windows
//! ignores it, and we never rewrite the approval flag to override them.
//!
//! Writing the key in-process via `winreg` is deliberate. The release binary
//! is windowless (`windows_subsystem = "windows"`), so shelling out to
//! `reg.exe` or `powershell` would flash a focus-stealing console window
//! (LL-G HIGH `kb/rust/gui-subsystem-console-child-window.md`). No child
//! process is spawned here.
//!
//! Non-Windows targets get no-ops so the desktop app compiles everywhere. The
//! macOS login item (`SMAppService` / `LaunchAgent`) is a separate task.

use thiserror::Error;

/// The `Run` value name. Also the friendly name Windows shows in Task
/// Manager's Startup tab, so it is user-facing: keep it "Hark".
pub const RUN_VALUE_NAME: &str = "Hark";

/// Passed to the autostart launch so the intent (start hidden into the tray)
/// is explicit in the command line. The window already starts hidden, so this
/// is currently a no-op at launch; it keeps the stored command stable if a
/// manual launch is ever made to show the window while autostart stays hidden.
pub const HIDDEN_FLAG: &str = "--hidden";

#[derive(Debug, Error)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Error {
    #[error("cannot determine the current executable path: {0}")]
    Exe(#[source] std::io::Error),
    #[error("registry access failed: {0}")]
    Registry(#[source] std::io::Error),
}

/// Make the OS startup entry match `enabled`. Idempotent: enabling twice
/// rewrites the same value; disabling when absent is a no-op.
pub fn reconcile(enabled: bool) -> Result<(), Error> {
    imp::reconcile(enabled)
}

/// True when the startup entry exists and points at the current exe. For
/// diagnostics and tests; the app's source of truth is the config toggle, so
/// nothing on the hot path reads the registry.
pub fn is_enabled() -> Result<bool, Error> {
    imp::is_enabled()
}

/// The `Run` value data for `exe`: `"<path>" --hidden`. The path is quoted so
/// a space in the install directory cannot split the command at login.
#[cfg(any(windows, test))]
fn command_for(exe: &std::path::Path) -> String {
    format!("\"{}\" {}", exe.display(), HIDDEN_FLAG)
}

#[cfg(windows)]
mod imp {
    use super::{command_for, Error, RUN_VALUE_NAME};
    use std::path::PathBuf;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn current_exe() -> Result<PathBuf, Error> {
        std::env::current_exe().map_err(Error::Exe)
    }

    fn is_not_found(e: &std::io::Error) -> bool {
        e.kind() == std::io::ErrorKind::NotFound
    }

    pub(super) fn reconcile(enabled: bool) -> Result<(), Error> {
        if enabled {
            let exe = current_exe()?;
            write_value(RUN_SUBKEY, RUN_VALUE_NAME, &command_for(&exe))
        } else {
            remove_value(RUN_SUBKEY, RUN_VALUE_NAME)
        }
    }

    pub(super) fn is_enabled() -> Result<bool, Error> {
        let current = read_value(RUN_SUBKEY, RUN_VALUE_NAME)?;
        match current {
            Some(value) => Ok(value == command_for(&current_exe()?)),
            None => Ok(false),
        }
    }

    /// Create-or-open the subkey and set the string value (REG_SZ).
    fn write_value(subkey: &str, name: &str, data: &str) -> Result<(), Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu.create_subkey(subkey).map_err(Error::Registry)?;
        key.set_value(name, &data.to_string())
            .map_err(Error::Registry)
    }

    /// Delete the value if present; a missing subkey or value is success (the
    /// desired end state, "not in startup", already holds).
    fn remove_value(subkey: &str, name: &str) -> Result<(), Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(subkey, KEY_WRITE) {
            Ok(k) => k,
            Err(e) if is_not_found(&e) => return Ok(()),
            Err(e) => return Err(Error::Registry(e)),
        };
        match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(e) if is_not_found(&e) => Ok(()),
            Err(e) => Err(Error::Registry(e)),
        }
    }

    /// Read a string value; `None` when either the subkey or the value is
    /// absent.
    fn read_value(subkey: &str, name: &str) -> Result<Option<String>, Error> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(subkey, KEY_READ) {
            Ok(k) => k,
            Err(e) if is_not_found(&e) => return Ok(None),
            Err(e) => return Err(Error::Registry(e)),
        };
        match key.get_value::<String, _>(name) {
            Ok(v) => Ok(Some(v)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(Error::Registry(e)),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // A scratch subkey well away from the real Run key, so tests never
        // register a real autostart entry on the dev machine.
        const TEST_SUBKEY: &str = r"Software\Hark\autostart-test";
        const TEST_VALUE: &str = "roundtrip";

        #[test]
        fn write_read_remove_round_trips_and_delete_is_idempotent() {
            // Clean slate even if a prior aborted run left the value behind.
            remove_value(TEST_SUBKEY, TEST_VALUE).expect("pre-clean");

            assert_eq!(
                read_value(TEST_SUBKEY, TEST_VALUE).expect("read absent"),
                None,
                "value must be absent before writing"
            );

            write_value(TEST_SUBKEY, TEST_VALUE, "hello").expect("write");
            assert_eq!(
                read_value(TEST_SUBKEY, TEST_VALUE).expect("read present"),
                Some("hello".to_string())
            );

            remove_value(TEST_SUBKEY, TEST_VALUE).expect("remove");
            assert_eq!(
                read_value(TEST_SUBKEY, TEST_VALUE).expect("read after remove"),
                None
            );
            // Removing an already-absent value is not an error.
            remove_value(TEST_SUBKEY, TEST_VALUE).expect("second remove is a no-op");

            // Best-effort scratch-key cleanup; harmless if it lingers.
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey(TEST_SUBKEY);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Error;

    pub(super) fn reconcile(_enabled: bool) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn is_enabled() -> Result<bool, Error> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn command_quotes_the_path_and_appends_the_hidden_flag() {
        let cmd = command_for(Path::new(r"C:\Program Files\Hark\Hark.exe"));
        assert_eq!(cmd, "\"C:\\Program Files\\Hark\\Hark.exe\" --hidden");
        // The path is quoted so a space in the directory cannot split argv.
        assert!(cmd.starts_with('"'));
        assert!(cmd.ends_with(HIDDEN_FLAG));
    }
}
