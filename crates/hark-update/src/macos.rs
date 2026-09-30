//! Verify an immutable mounted disk image, then replace the complete app bundle.
//! A development/ad-hoc signed build cannot authorize a production update.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::UpdateError;

const BUNDLE_ID: &str = "com.boardpandas.hark";

fn failure(message: impl Into<String>) -> UpdateError {
    UpdateError::Verification(message.into())
}

fn run(command: &mut Command) -> Result<(), UpdateError> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(failure(format!(
            "{}: {}",
            command.get_program().to_string_lossy(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn bundle_for_executable(exe: &Path) -> Result<PathBuf, UpdateError> {
    let macos = exe
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "MacOS"));
    let contents = macos
        .and_then(Path::parent)
        .filter(|p| p.file_name().is_some_and(|n| n == "Contents"));
    contents
        .and_then(Path::parent)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| failure("Install Hark.app in Applications before updating"))
}

fn running_bundle() -> Result<PathBuf, UpdateError> {
    bundle_for_executable(&crate::current_exe()?)
}

pub(super) fn staged_path(_asset_name: &str) -> Result<PathBuf, UpdateError> {
    // An exclusive, mode-0600 file outside the signed app. Writing into the
    // bundle would invalidate its resource seal before verification.
    let file = tempfile::Builder::new()
        .prefix("hark-update-")
        .suffix(".dmg")
        .tempfile()?;
    let (_, path) = file.keep().map_err(|e| UpdateError::Io(e.error))?;
    Ok(path)
}

struct MountedImage {
    mount: tempfile::TempDir,
}

impl MountedImage {
    fn open(image: &Path) -> Result<Self, UpdateError> {
        let mount = tempfile::Builder::new().prefix("hark-mount-").tempdir()?;
        run(Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(mount.path())
            .arg(image)
            .stdin(Stdio::null()))?;
        Ok(Self { mount })
    }

    fn app(&self) -> PathBuf {
        self.mount.path().join("Hark.app")
    }
}

impl Drop for MountedImage {
    fn drop(&mut self) {
        if let Err(error) = run(Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(self.mount.path()))
        {
            log::warn!("could not unmount update image: {error}");
        }
    }
}

fn team_id(output: &str) -> Result<&str, UpdateError> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| {
            team.len() == 10
                && team
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
        .ok_or_else(|| failure("The installed app must have a Developer ID signature to update"))
}

fn requirement(running: &Path) -> Result<String, UpdateError> {
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(running))?;
    let output = Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=4"])
        .arg(running)
        .output()?;
    if !output.status.success() {
        return Err(failure("Cannot read the installed app's signature"));
    }
    let output = String::from_utf8_lossy(&output.stderr);
    let team = team_id(&output)?;
    // Apple's Developer ID chain + the installed publisher + exact app ID.
    // No unsigned-build fallback; a different valid publisher is not Hark.
    Ok(format!("anchor apple generic and identifier \"{BUNDLE_ID}\" and certificate leaf[subject.OU] = \"{team}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"))
}

fn verify_app(app: &Path, requirement: &str) -> Result<(), UpdateError> {
    if app.is_symlink() || !app.join("Contents/MacOS/hark-app").is_file() {
        return Err(failure(
            "The disk image does not contain a complete Hark.app",
        ));
    }
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "-R"])
        .arg(requirement)
        .arg(app))?;
    run(Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute"])
        .arg(app))
}

pub(super) fn self_install_supported() -> bool {
    let check = || -> Result<(), UpdateError> {
        let running = running_bundle()?;
        let requirement = requirement(&running)?;
        run(Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "-R"])
            .arg(requirement)
            .arg(&running))?;
        let parent = running
            .parent()
            .ok_or_else(|| failure("Missing app directory"))?;
        let _probe = tempfile::Builder::new()
            .prefix(".hark-write-check-")
            .tempdir_in(parent)?;
        Ok(())
    };
    check().is_ok()
}

pub fn verify(staged: &Path) -> Result<(), UpdateError> {
    let requirement = requirement(&running_bundle()?)?;
    let mounted = MountedImage::open(staged)?;
    verify_app(&mounted.app(), &requirement)
}

/// Stage and validate before exiting. The helper waits for this process, swaps
/// the whole bundle on the same filesystem, and rolls back if replacement fails.
/// The caller must exit promptly after success, as on Windows.
pub fn install(staged: &Path) -> Result<(), UpdateError> {
    let running = running_bundle()?;
    let requirement = requirement(&running)?;
    let parent = running
        .parent()
        .ok_or_else(|| failure("App has no install directory"))?;
    // Also serves as a write-access check: never quit if the install requires
    // elevation, is on a read-only DMG, or is App Translocation's private copy.
    let workspace = tempfile::Builder::new().prefix(".hark-update-").tempdir_in(parent)
        .map_err(|e| failure(format!("Cannot update this app location; move Hark.app to a writable Applications folder: {e}")))?;
    let incoming = workspace.path().join("Hark.app");
    {
        let mounted = MountedImage::open(staged)?;
        verify_app(&mounted.app(), &requirement)?;
        run(Command::new("/usr/bin/ditto")
            .arg(mounted.app())
            .arg(&incoming))?;
    }
    verify_app(&incoming, &requirement)?;
    let helper = workspace.path().join("install.sh");
    std::fs::write(&helper, include_str!("macos-install.sh"))?;
    let log = std::fs::File::create(workspace.path().join("install.log"))?;
    Command::new("/bin/sh")
        .arg(&helper)
        .arg(std::process::id().to_string())
        .arg(&running)
        .arg(workspace.path())
        .arg(&requirement)
        .arg(crate::RELAUNCHED_FLAG)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    // The detached helper owns cleanup from this point onward.
    let _ = workspace.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_real_bundle_layout_can_be_updated() {
        assert_eq!(
            bundle_for_executable(Path::new("/Applications/Hark.app/Contents/MacOS/hark-app"))
                .unwrap(),
            Path::new("/Applications/Hark.app")
        );
        for path in [
            "/tmp/hark-app",
            "/tmp/Foo/Contents/MacOS/hark-app",
            "/Applications/Hark.app/bin/hark-app",
        ] {
            assert!(bundle_for_executable(Path::new(path)).is_err());
        }
    }

    #[test]
    fn unsigned_or_malformed_team_identifiers_are_rejected() {
        assert_eq!(
            team_id("Signature=Developer ID\nTeamIdentifier=ABC1234567\n").unwrap(),
            "ABC1234567"
        );
        for output in [
            "TeamIdentifier=not set",
            "TeamIdentifier=",
            "TeamIdentifier=ABC1234567\"",
            "Authority=Developer ID",
        ] {
            assert!(team_id(output).is_err());
        }
    }

    #[test]
    fn temporary_download_is_exclusive_and_outside_the_app() {
        let a = staged_path("../../evil.dmg").unwrap();
        let b = staged_path("../../evil.dmg").unwrap();
        assert_ne!(a, b);
        assert!(a.is_file());
        assert!(a
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("hark-update-"));
        std::fs::remove_file(a).unwrap();
        std::fs::remove_file(b).unwrap();
    }
}
