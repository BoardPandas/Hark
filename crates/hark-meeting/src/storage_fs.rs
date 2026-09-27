//! The storage cap, I/O half: measure meeting audio on disk and delete one
//! meeting's audio. The only file deletion in meeting mode, and deliberately
//! small, because it deletes user data.
//!
//! Guard (plan §4.9 rule 7): only a directory that is a direct, real (not
//! symlinked) child of `<data_dir>/meetings/` and is named by a meeting id the
//! database knows can be deleted. Anything else under `meetings/` is reported
//! as stray and never deleted. Deletions are logged by id and size only.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What is on disk under `meetings/`.
#[derive(Debug, Default)]
pub struct DiskUsage {
    /// Known meetings' directories and their total size, in directory order.
    pub meetings: Vec<(String, u64)>,
    /// Entries that are not a known meeting's directory (loose files,
    /// symlinks, unknown directories). Shown in Settings, never deleted.
    pub stray: Vec<PathBuf>,
}

impl DiskUsage {
    /// Bytes used by known meetings: the figure the cap is enforced against.
    pub fn total(&self) -> u64 {
        self.meetings.iter().map(|(_, b)| b).sum()
    }
}

/// Measure `meetings_dir`. A missing directory is no meetings yet.
pub fn scan(meetings_dir: &Path, known_ids: &[&str]) -> io::Result<DiskUsage> {
    let entries = match fs::read_dir(meetings_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DiskUsage::default()),
        Err(e) => return Err(e),
    };
    let mut usage = DiskUsage::default();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // DirEntry::file_type does not follow symlinks (or junctions).
        let is_real_dir = entry.file_type()?.is_dir();
        if is_real_dir && is_plain_id(&name) && known_ids.contains(&name.as_str()) {
            usage.meetings.push((name, dir_bytes(&entry.path())?));
        } else {
            usage.stray.push(entry.path());
        }
    }
    Ok(usage)
}

/// Delete one meeting's audio directory and return the bytes freed. Refuses
/// (`PermissionDenied`, nothing touched) anything the guard does not allow.
pub fn delete_audio(meetings_dir: &Path, id: &str, known_ids: &[&str]) -> io::Result<u64> {
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to delete meeting audio: {why}"),
        ))
    };
    if !is_plain_id(id) {
        return refuse("not a plain meeting id");
    }
    if !known_ids.contains(&id) {
        return refuse("no such meeting");
    }
    let root = fs::canonicalize(meetings_dir)?;
    let target = root.join(id);
    if !fs::symlink_metadata(&target)?.file_type().is_dir() {
        return refuse("not a real directory");
    }
    if fs::canonicalize(&target)?.parent() != Some(root.as_path()) {
        return refuse("outside the meetings directory");
    }
    let bytes = dir_bytes(&target)?;
    // remove_dir_all removes symlinks inside, never what they point at.
    fs::remove_dir_all(&target)?;
    log::info!("meeting {id}: audio deleted ({bytes} bytes)");
    Ok(bytes)
}

/// A meeting id usable as one path component: no separators, no `.`/`..`,
/// nothing a Windows path would reinterpret.
fn is_plain_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', ':', '\0'])
}

/// Total size of the regular files under `dir`. Symlinks are neither
/// followed nor counted: their targets are not the meeting's audio.
fn dir_bytes(dir: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            total += dir_bytes(&entry.path())?;
        } else if kind.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, vec![7u8; bytes]).expect("write");
    }

    /// meetings/{m1: 300 B + 50 B nested, m2: 1000 B, ghost: 5 B} + stray.txt
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let meetings = dir.path().join("meetings");
        write(&meetings.join("m1").join("me.wav"), 200);
        write(&meetings.join("m1").join("them.wav"), 100);
        write(&meetings.join("m1").join("tmp").join("part"), 50);
        write(&meetings.join("m2").join("archive.mp3"), 1000);
        write(&meetings.join("ghost").join("me.wav"), 5);
        write(&meetings.join("stray.txt"), 9);
        (dir, meetings)
    }

    #[test]
    fn scan_measures_known_meetings_and_reports_the_rest() {
        let (_dir, meetings) = fixture();
        let usage = scan(&meetings, &["m1", "m2"]).expect("scan");
        let mut found = usage.meetings.clone();
        found.sort();
        assert_eq!(
            found,
            vec![("m1".to_string(), 350), ("m2".to_string(), 1000)]
        );
        assert_eq!(usage.total(), 1350);
        let mut stray: Vec<String> = usage
            .stray
            .iter()
            .map(|p| p.file_name().expect("name").to_string_lossy().into_owned())
            .collect();
        stray.sort();
        assert_eq!(stray, ["ghost", "stray.txt"], "unknown dir and loose file");
    }

    #[test]
    fn scan_without_a_meetings_dir_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let usage = scan(&dir.path().join("meetings"), &["m1"]).expect("scan");
        assert!(usage.meetings.is_empty() && usage.stray.is_empty());
    }

    #[test]
    fn delete_removes_exactly_one_known_meeting() {
        let (_dir, meetings) = fixture();
        assert_eq!(
            delete_audio(&meetings, "m1", &["m1", "m2"]).expect("delete"),
            350
        );
        assert!(!meetings.join("m1").exists());
        assert!(meetings.join("m2").join("archive.mp3").exists());
        assert!(meetings.join("ghost").exists() && meetings.join("stray.txt").exists());
    }

    #[test]
    fn delete_refuses_unknown_and_non_plain_ids() {
        let (dir, meetings) = fixture();
        write(&dir.path().join("precious").join("keep.txt"), 1);
        // Refused even when the database claims to know them.
        for id in [
            "",
            ".",
            "..",
            "../precious",
            "m1/../../precious",
            r"..\precious",
            "C:",
        ] {
            let err = delete_audio(&meetings, id, &[id]).expect_err(id);
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{id:?}");
        }
        let err = delete_audio(&meetings, "ghost", &["m1", "m2"]).expect_err("unknown id");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(meetings.join("ghost").exists());
        assert!(dir.path().join("precious").join("keep.txt").exists());
    }

    #[test]
    fn delete_of_a_missing_meeting_is_an_error_not_a_success() {
        let (_dir, meetings) = fixture();
        let err = delete_audio(&meetings, "gone", &["gone"]).expect_err("missing");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// Symlinks need privileges on Windows (Developer Mode); skip if denied.
    fn try_symlink_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(target, link);
        match made {
            Ok(()) => true,
            Err(e) => {
                eprintln!("skipping: cannot create a symlink here ({e})");
                false
            }
        }
    }

    #[test]
    fn a_symlinked_meeting_dir_is_stray_and_never_deleted_through() {
        let (dir, meetings) = fixture();
        let outside = dir.path().join("outside");
        write(&outside.join("keep.txt"), 1);
        if !try_symlink_dir(&outside, &meetings.join("m3")) {
            return;
        }
        let usage = scan(&meetings, &["m1", "m2", "m3"]).expect("scan");
        assert!(usage.stray.iter().any(|p| p.ends_with("m3")));
        let err = delete_audio(&meetings, "m3", &["m3"]).expect_err("symlink");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(outside.join("keep.txt").exists());
    }

    #[test]
    fn a_symlink_inside_a_meeting_is_removed_but_its_target_kept() {
        let (dir, meetings) = fixture();
        let outside = dir.path().join("outside");
        write(&outside.join("keep.txt"), 1);
        if !try_symlink_dir(&outside, &meetings.join("m1").join("link")) {
            return;
        }
        assert_eq!(
            delete_audio(&meetings, "m1", &["m1"]).expect("delete"),
            350,
            "the link's target is not counted"
        );
        assert!(outside.join("keep.txt").exists());
    }
}
