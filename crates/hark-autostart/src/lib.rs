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
//! Linux: an XDG autostart entry, `$XDG_CONFIG_HOME/autostart/hark.desktop`
//! (`~/.config/autostart` by default). Every mainstream desktop — GNOME, KDE,
//! XFCE, Cinnamon, sway via its own config — reads that directory at session
//! start, so one file covers them all without touching systemd user units or
//! a desktop-specific API. [`reconcile`] writes it (self-healing a stale path
//! after an upgrade) or deletes it, exactly as the Windows branch does with
//! its registry value.
//!
//! Remaining non-Windows, non-Linux targets get no-ops so the desktop app
//! compiles everywhere. The macOS login item (`SMAppService` / `LaunchAgent`)
//! is a separate task.

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
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub enum Error {
    #[error("cannot determine the current executable path: {0}")]
    Exe(#[source] std::io::Error),
    #[cfg(windows)]
    #[error("registry access failed: {0}")]
    Registry(#[source] std::io::Error),
    #[cfg(target_os = "linux")]
    #[error("no per-user config directory to hold the autostart entry")]
    NoConfigDir,
    #[cfg(target_os = "linux")]
    #[error("cannot write the autostart entry at {path}: {source}")]
    DesktopFile {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
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
#[cfg(any(windows, all(test, not(target_os = "linux"))))]
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

#[cfg(target_os = "linux")]
mod imp {
    use super::Error;
    use std::path::{Path, PathBuf};

    /// Basename of the autostart entry. The desktop-entry spec wants it to
    /// match the application's desktop file id, which the packages install as
    /// `hark.desktop`.
    const ENTRY: &str = "hark.desktop";

    fn entry_path() -> Result<PathBuf, Error> {
        let dir = hark_config::default_config_dir().ok_or(Error::NoConfigDir)?;
        Ok(dir.join("autostart").join(ENTRY))
    }

    fn current_exe() -> Result<PathBuf, Error> {
        std::env::current_exe().map_err(Error::Exe)
    }

    pub(super) fn reconcile(enabled: bool) -> Result<(), Error> {
        let path = entry_path()?;
        if enabled {
            let exe = current_exe()?;
            write_entry(&path, &super::desktop_entry(&exe))
        } else {
            remove_entry(&path)
        }
    }

    pub(super) fn is_enabled() -> Result<bool, Error> {
        let path = entry_path()?;
        let current = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(Error::DesktopFile { path, source }),
        };
        // Compare the Exec line only. Desktops rewrite these files (GNOME
        // appends X-GNOME-Autostart-enabled when you toggle an entry in
        // Tweaks), so a whole-file comparison would report "disabled" for an
        // entry that is working perfectly.
        let expected = super::exec_line(&current_exe()?);
        Ok(current.lines().any(|line| line.trim() == expected))
    }

    fn write_entry(path: &Path, contents: &str) -> Result<(), Error> {
        let err = |source| Error::DesktopFile {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        std::fs::write(path, contents).map_err(err)
    }

    /// A missing file is success: the desired end state, "not in startup",
    /// already holds.
    fn remove_entry(path: &Path) -> Result<(), Error> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::DesktopFile {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn write_read_remove_round_trips_and_delete_is_idempotent() {
            let dir = tempfile::tempdir().expect("tempdir");
            // The autostart directory does not exist on a fresh account.
            let path = dir.path().join("autostart").join(ENTRY);

            write_entry(&path, "hello").expect("write");
            assert_eq!(std::fs::read_to_string(&path).expect("read"), "hello");

            remove_entry(&path).expect("remove");
            assert!(!path.exists());
            remove_entry(&path).expect("a second remove is a no-op");
        }

        #[test]
        fn the_entry_lands_under_the_xdg_autostart_directory() {
            // Every desktop reads exactly this path; a typo here is an
            // autostart toggle that writes a file nothing will ever look at.
            let path = entry_path().expect("a config dir exists on this machine");
            assert!(path.ends_with(Path::new("autostart").join(ENTRY)));
        }

        #[test]
        fn the_written_entry_is_the_one_is_enabled_recognises() {
            // The pair has to agree or the Settings toggle reads back as off
            // immediately after switching it on.
            let exe = Path::new("/usr/bin/hark");
            let entry = super::super::desktop_entry(exe);
            assert!(entry
                .lines()
                .any(|line| line.trim() == super::super::exec_line(exe)));
        }
    }
}

/// The desktop entry written for `exe`. Minimal on purpose: `Name` and `Exec`
/// are all the spec requires beyond `Type`, and every extra key is one more
/// thing that can disagree with the packaged `hark.desktop`.
///
/// `X-GNOME-Autostart-enabled` is included because GNOME writes it when a user
/// toggles an entry and treats its absence as true; stating it makes an entry
/// Hark just wrote unambiguous rather than depending on that default.
#[cfg(target_os = "linux")]
fn desktop_entry(exe: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={RUN_VALUE_NAME}\n\
         Comment=Push-to-talk voice dictation\n\
         {}\n\
         Icon=hark\n\
         Terminal=false\n\
         Categories=Utility;AudioVideo;\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_line(exe)
    )
}

/// The entry's `Exec=` line: the quoted exe path plus [`HIDDEN_FLAG`].
///
/// The desktop-entry spec gives `Exec` its own quoting rules, and they are not
/// the shell's: a value may be double-quoted, and inside those quotes a
/// literal `"`, `` ` ``, `$` or `\` must be prefixed with a backslash. A path
/// containing any of them is unusual but entirely legal on Linux, and getting
/// this wrong means an autostart entry that silently fails to launch.
#[cfg(target_os = "linux")]
fn exec_line(exe: &std::path::Path) -> String {
    let escaped: String = exe
        .to_string_lossy()
        .chars()
        .flat_map(|c| {
            let escape = matches!(c, '"' | '`' | '$' | '\\');
            escape.then_some('\\').into_iter().chain(std::iter::once(c))
        })
        .collect();
    format!("Exec=\"{escaped}\" {HIDDEN_FLAG}")
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Error;

    pub(super) fn reconcile(_enabled: bool) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn is_enabled() -> Result<bool, Error> {
        Ok(false)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_exec_line_quotes_the_path_and_appends_the_hidden_flag() {
        let line = exec_line(Path::new("/opt/Hark Beta/hark"));
        assert_eq!(line, "Exec=\"/opt/Hark Beta/hark\" --hidden");
        // Quoted so a space in the install directory cannot split the command.
        assert!(line.starts_with("Exec=\""));
        assert!(line.ends_with(HIDDEN_FLAG));
    }

    #[test]
    fn the_exec_line_escapes_what_the_desktop_spec_reserves() {
        // Legal path characters that are special inside a quoted Exec value.
        // Unescaped, the entry is malformed and the desktop drops it silently.
        let line = exec_line(Path::new(r#"/home/u/$IT/a"b/`c`/d\e/hark"#));
        assert!(line.contains(r"\$IT"), "$ must be escaped: {line}");
        assert!(line.contains(r#"a\"b"#), "a quote must be escaped: {line}");
        assert!(line.contains(r"\`c\`"), "backticks must be escaped: {line}");
        assert!(
            line.contains(r"d\\e"),
            "a backslash must be escaped: {line}"
        );
    }

    #[test]
    fn an_ordinary_path_is_left_alone() {
        assert_eq!(
            exec_line(Path::new("/usr/bin/hark")),
            "Exec=\"/usr/bin/hark\" --hidden"
        );
    }

    #[test]
    fn the_entry_is_a_wellformed_desktop_file() {
        let entry = desktop_entry(Path::new("/usr/bin/hark"));
        // The three keys the spec actually requires. A file missing any of
        // them is ignored by the session with no error anywhere.
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("\nType=Application\n"));
        assert!(entry.contains(&format!("\nName={RUN_VALUE_NAME}\n")));
        assert!(entry.contains("\nExec=\"/usr/bin/hark\" --hidden\n"));
        assert!(entry.ends_with('\n'), "the file must end with a newline");
    }
}

#[cfg(all(test, not(target_os = "linux")))]
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
