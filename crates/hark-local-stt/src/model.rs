//! What a local model *is*: its files, their pinned sizes and hashes, and
//! how to tell whether a copy on disk is complete and trustworthy.
//!
//! All facts here were verified against Hugging Face on 2026-07-21 (anonymous
//! HTTP 200, `Content-Length` present, `accept-ranges: bytes`). The sha256
//! values are the LFS `x-linked-etag` the repo reports for each file.

use crate::error::LocalSttError;
use std::path::{Path, PathBuf};

/// One file belonging to a model.
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    /// Pinned sha256, or `None` for files Hugging Face stores directly in git
    /// rather than LFS (their ETag is a git SHA-1, not a content hash). Those
    /// are verified by size alone; they are tiny and a corrupt one fails
    /// loudly at model load.
    pub sha256: Option<&'static str>,
}

/// A downloadable on-device model.
pub struct ModelSpec {
    /// Stable id; also the directory name under `<data_dir>/models/`.
    pub id: &'static str,
    /// Human-facing name for the Settings UI.
    pub display_name: &'static str,
    /// Licence of the *weights* (not the code), shown as attribution.
    pub licence: &'static str,
    /// Base URL; file names append directly.
    pub base_url: &'static str,
    pub files: &'static [ModelFile],
}

impl ModelSpec {
    /// Total download size in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    pub fn url_for(&self, file: &ModelFile) -> String {
        format!("{}/{}", self.base_url, file.name)
    }

    /// Where this model's files live once downloaded.
    pub fn dir(&self) -> Result<PathBuf, LocalSttError> {
        hark_config::model_dir(self.id).ok_or(LocalSttError::NoDataDir)
    }

    /// Inspect a directory and classify what is there. Cheap (metadata only,
    /// no hashing) so the Settings page can call it freely.
    pub fn status_in(&self, dir: &Path) -> ModelStatus {
        let mut present = 0u64;
        let mut complete = true;
        for f in self.files {
            match std::fs::metadata(dir.join(f.name)) {
                Ok(m) if m.len() == f.bytes => present += f.bytes,
                _ => {
                    complete = false;
                    // A resumable partial counts toward progress.
                    if let Ok(m) = std::fs::metadata(part_path(&dir.join(f.name))) {
                        present += m.len().min(f.bytes);
                    }
                }
            }
        }
        if complete {
            ModelStatus::Ready
        } else if present == 0 {
            ModelStatus::NotDownloaded
        } else {
            ModelStatus::Partial {
                have_bytes: present,
            }
        }
    }

    /// Convenience: status at the model's canonical directory.
    pub fn status(&self) -> ModelStatus {
        match self.dir() {
            Ok(d) => self.status_in(&d),
            Err(_) => ModelStatus::NotDownloaded,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    NotDownloaded,
    /// Some bytes on disk; a Download resumes from here.
    Partial {
        have_bytes: u64,
    },
    /// Every file present at its exact pinned size.
    Ready,
}

impl ModelStatus {
    pub fn is_ready(self) -> bool {
        matches!(self, ModelStatus::Ready)
    }
}

/// The in-progress name for a file being downloaded. Renamed into place only
/// after the whole file is present and verified, so a half-written file is
/// never mistaken for a usable one.
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// Parakeet TDT 0.6B v3, int8 ONNX. Latest revision; 25 languages.
pub static PARAKEET_V3_INT8: ModelSpec = ModelSpec {
    id: "parakeet-tdt-0.6b-v3-int8",
    display_name: "Parakeet TDT 0.6B v3 (int8)",
    licence: "CC-BY-4.0 — NVIDIA",
    base_url:
        "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main",
    files: &[
        ModelFile {
            name: "encoder.int8.onnx",
            bytes: 652_184_281,
            sha256: Some("acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247"),
        },
        ModelFile {
            name: "decoder.int8.onnx",
            bytes: 11_845_275,
            sha256: Some("179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e"),
        },
        ModelFile {
            name: "joiner.int8.onnx",
            bytes: 6_355_277,
            sha256: Some("3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3"),
        },
        // Stored in git, not LFS: size-only verification (see ModelFile).
        ModelFile {
            name: "tokens.txt",
            bytes: 93_939,
            sha256: None,
        },
    ],
};

/// Every model the app knows how to download.
pub static CATALOG: &[&ModelSpec] = &[&PARAKEET_V3_INT8];

/// Look a model up by the id stored in `[local_stt] model`.
pub fn find(id: &str) -> Result<&'static ModelSpec, LocalSttError> {
    CATALOG
        .iter()
        .copied()
        .find(|m| m.id == id)
        .ok_or_else(|| LocalSttError::UnknownModel(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn the_default_config_model_exists_in_the_catalog() {
        // The config default and the catalog must never drift apart, or a
        // fresh install offers a model it cannot download.
        let spec = find(hark_config::DEFAULT_MODEL).expect("default model is in the catalog");
        assert_eq!(spec.id, "parakeet-tdt-0.6b-v3-int8");
    }

    #[test]
    fn unknown_ids_are_rejected() {
        assert!(matches!(
            find("no-such-model"),
            Err(LocalSttError::UnknownModel(_))
        ));
    }

    #[test]
    fn total_is_the_sum_of_the_verified_file_sizes() {
        // Verified against Hugging Face 2026-07-21.
        assert_eq!(PARAKEET_V3_INT8.total_bytes(), 670_478_772);
    }

    #[test]
    fn urls_join_without_a_double_slash() {
        let f = &PARAKEET_V3_INT8.files[0];
        assert_eq!(
            PARAKEET_V3_INT8.url_for(f),
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main/encoder.int8.onnx"
        );
    }

    fn write(path: &Path, len: usize) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(&vec![0u8; len]).unwrap();
    }

    #[test]
    fn an_empty_directory_is_not_downloaded() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            PARAKEET_V3_INT8.status_in(d.path()),
            ModelStatus::NotDownloaded
        );
    }

    #[test]
    fn every_file_at_its_exact_size_is_ready() {
        let d = tempfile::tempdir().unwrap();
        for f in PARAKEET_V3_INT8.files {
            write(&d.path().join(f.name), f.bytes as usize);
        }
        assert_eq!(PARAKEET_V3_INT8.status_in(d.path()), ModelStatus::Ready);
    }

    #[test]
    fn a_wrong_sized_file_is_never_ready() {
        // A truncated encoder must not read as a usable model just because
        // the file name exists.
        let d = tempfile::tempdir().unwrap();
        for f in PARAKEET_V3_INT8.files {
            write(&d.path().join(f.name), f.bytes as usize);
        }
        write(&d.path().join("tokens.txt"), 10);
        assert!(!PARAKEET_V3_INT8.status_in(d.path()).is_ready());
    }

    #[test]
    fn partial_files_report_the_bytes_already_on_disk() {
        let d = tempfile::tempdir().unwrap();
        // tokens.txt complete, plus 100 bytes of a resumable joiner.
        write(&d.path().join("tokens.txt"), 93_939);
        write(&d.path().join("joiner.int8.onnx.part"), 100);
        assert_eq!(
            PARAKEET_V3_INT8.status_in(d.path()),
            ModelStatus::Partial {
                have_bytes: 93_939 + 100
            }
        );
    }

    #[test]
    fn part_path_appends_rather_than_replacing_the_extension() {
        // `with_extension` would turn encoder.int8.onnx into encoder.int8.part
        // and collide across files; the suffix must be additive.
        assert_eq!(
            part_path(Path::new("/m/encoder.int8.onnx")),
            PathBuf::from("/m/encoder.int8.onnx.part")
        );
    }
}
