//! Temporary-file ownership shared by full and excerpt exports.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Own only the unique file we created; a failed export never touches an
/// existing destination or a similarly named user's temporary file.
pub(super) struct PendingFile(PathBuf);

impl PendingFile {
    pub(super) fn new(out: &Path) -> io::Result<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut name = out.file_name().unwrap_or_default().to_os_string();
        name.push(format!(
            ".hark-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = out.with_file_name(name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok((Self(path), file))
    }

    pub(super) fn commit(self, out: &Path) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&self.0)?
            .sync_all()?;
        std::fs::rename(&self.0, out)
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
