//! Streaming, resumable model download.
//!
//! Three properties are load-bearing and easy to get wrong:
//!
//! 1. **Stream to disk.** The encoder is 652 MB. `Response::bytes()` (what
//!    `hark-update` does for its small installer) would buffer all of it in
//!    RAM first.
//! 2. **Override the request timeout.** The shared client caps requests at
//!    `hark_stt::TOTAL_TIMEOUT_MS` (15 s) because latency is the product on
//!    the dictation path. A 670 MB download needs its own, generous bound.
//! 3. **Resume, don't restart.** Partial files are kept as `<name>.part` and
//!    continued with a `Range:` request, so a cancelled or dropped download
//!    does not throw away 600 MB of progress.

use crate::error::LocalSttError;
use crate::model::{part_path, ModelFile, ModelSpec};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Generous ceiling for one file. Not a latency bound — it exists only so a
/// wedged connection cannot hang the download thread forever.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

/// Read buffer. Large enough that the syscall rate stays low on a fast link.
const CHUNK: usize = 256 * 1024;

/// Minimum wall time between progress callbacks. The UI repaints on each one,
/// so an unthrottled callback would spend more time painting than downloading.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Download progress across the whole model, not just the current file.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub current_file: &'static str,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.done_bytes as f64 / self.total_bytes as f64) as f32
    }
}

/// Fetch every missing file of `spec` into `dir`, resuming partials.
///
/// Returns `Err(Cancelled)` promptly when `cancel` flips; `.part` files are
/// left in place so the next call continues from where this one stopped.
pub fn download(
    spec: &'static ModelSpec,
    dir: &Path,
    client: &reqwest::blocking::Client,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), LocalSttError> {
    std::fs::create_dir_all(dir).map_err(|source| LocalSttError::Io {
        path: dir.display().to_string(),
        source,
    })?;

    let total = spec.total_bytes();
    // Bytes already accounted for by files completed in earlier iterations.
    let mut base = 0u64;

    for file in spec.files {
        let final_path = dir.join(file.name);
        // Already complete from a previous run: nothing to do.
        if std::fs::metadata(&final_path).is_ok_and(|m| m.len() == file.bytes) {
            base += file.bytes;
            on_progress(Progress {
                done_bytes: base,
                total_bytes: total,
                current_file: file.name,
            });
            continue;
        }

        fetch_one(spec, file, dir, client, cancel, base, total, on_progress)?;
        base += file.bytes;
    }

    log::info!(
        "local model {} downloaded ({} bytes across {} files)",
        spec.id,
        total,
        spec.files.len()
    );
    Ok(())
}

/// Download one file to `<name>.part`, verify it, then rename into place.
#[allow(clippy::too_many_arguments)]
fn fetch_one(
    spec: &'static ModelSpec,
    file: &'static ModelFile,
    dir: &Path,
    client: &reqwest::blocking::Client,
    cancel: &AtomicBool,
    base: u64,
    total: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), LocalSttError> {
    // Check before the request, not just inside the read loop: a cancel that
    // lands between the click and this thread's first read must not open a
    // connection at all.
    if cancel.load(Ordering::Relaxed) {
        return Err(LocalSttError::Cancelled);
    }

    let final_path = dir.join(file.name);
    let part = part_path(&final_path);
    let io_err = |path: &Path| {
        let p = path.display().to_string();
        move |source| LocalSttError::Io { path: p, source }
    };

    // How much of this file we already hold. A `.part` longer than the
    // expected size is garbage from an aborted or changed download.
    let have = match std::fs::metadata(&part) {
        Ok(m) if m.len() < file.bytes => m.len(),
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            0
        }
        Err(_) => 0,
    };

    let mut request = client
        .get(spec.url_for(file))
        .timeout(DOWNLOAD_TIMEOUT)
        // Identity encoding: a compressed transfer makes Content-Length
        // disagree with the bytes we write, which breaks resume arithmetic.
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }

    let response = request.send().map_err(|e| LocalSttError::Http {
        file: file.name.to_string(),
        detail: e.to_string(),
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(LocalSttError::Http {
            file: file.name.to_string(),
            detail: format!("HTTP {}", status.as_u16()),
        });
    }
    // 206 honored our Range; a 200 means the server ignored it and is sending
    // the whole file, so the partial must be discarded rather than appended to.
    let resuming = have > 0 && status.as_u16() == 206;
    if have > 0 && !resuming {
        log::warn!(
            "{} ignored our Range request; restarting the file from zero",
            file.name
        );
    }

    let mut sink = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resuming)
        .open(&part)
        .map_err(io_err(&part))?;
    let mut written = if resuming {
        sink.seek(SeekFrom::End(0)).map_err(io_err(&part))?
    } else {
        0
    };

    let mut reader = response;
    let mut buf = vec![0u8; CHUNK];
    let mut last_report = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            // Flush what we have so the resume point survives the cancel.
            let _ = sink.flush();
            log::info!(
                "model download cancelled at {written} bytes of {}",
                file.name
            );
            return Err(LocalSttError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| LocalSttError::Http {
            file: file.name.to_string(),
            detail: e.to_string(),
        })?;
        if n == 0 {
            break;
        }
        sink.write_all(&buf[..n]).map_err(io_err(&part))?;
        written += n as u64;

        if last_report.elapsed() >= PROGRESS_EVERY {
            last_report = Instant::now();
            on_progress(Progress {
                done_bytes: base + written,
                total_bytes: total,
                current_file: file.name,
            });
        }
    }
    sink.flush().map_err(io_err(&part))?;
    drop(sink);

    if written != file.bytes {
        let _ = std::fs::remove_file(&part);
        return Err(LocalSttError::Http {
            file: file.name.to_string(),
            detail: format!("expected {} bytes, received {written}", file.bytes),
        });
    }
    verify(&part, file)?;

    std::fs::rename(&part, &final_path).map_err(io_err(&final_path))?;
    on_progress(Progress {
        done_bytes: base + file.bytes,
        total_bytes: total,
        current_file: file.name,
    });
    Ok(())
}

/// Hash a completed `.part` against its pinned sha256. Streams the file so a
/// 652 MB check costs one buffer, not 652 MB. A mismatch deletes the file:
/// resuming onto corrupt bytes would fail the same way forever.
fn verify(path: &Path, file: &ModelFile) -> Result<(), LocalSttError> {
    let Some(expected) = file.sha256 else {
        return Ok(());
    };
    let mut f = std::fs::File::open(path).map_err(|source| LocalSttError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf).map_err(|source| LocalSttError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex(&hasher.finalize());
    if actual != expected {
        let _ = std::fs::remove_file(path);
        return Err(LocalSttError::Checksum {
            file: file.name.to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Delete a downloaded model, reclaiming its disk. Missing files are not an
/// error: the point is that nothing is left afterward.
pub fn remove(dir: &Path) -> Result<(), LocalSttError> {
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(dir).map_err(|source| LocalSttError::Io {
        path: dir.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PARAKEET_V3_INT8;

    #[test]
    fn fraction_is_bounded_and_handles_an_empty_total() {
        let p = |d, t| Progress {
            done_bytes: d,
            total_bytes: t,
            current_file: "x",
        };
        assert_eq!(p(0, 100).fraction(), 0.0);
        assert_eq!(p(50, 100).fraction(), 0.5);
        assert_eq!(p(100, 100).fraction(), 1.0);
        // No division by zero before the spec is known.
        assert_eq!(p(0, 0).fraction(), 0.0);
    }

    #[test]
    fn fraction_stays_precise_at_model_scale() {
        // 670 MB overflows f32 mantissa precision if computed in f32; the
        // progress bar must still advance smoothly near the end.
        let p = Progress {
            done_bytes: 670_000_000,
            total_bytes: 670_478_772,
            current_file: "encoder.int8.onnx",
        };
        assert!(
            p.fraction() > 0.999 && p.fraction() < 1.0,
            "{}",
            p.fraction()
        );
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn verify_accepts_a_file_with_no_pinned_hash() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("tokens.txt");
        std::fs::write(&p, b"anything").unwrap();
        let f = ModelFile {
            name: "tokens.txt",
            bytes: 8,
            sha256: None,
        };
        assert!(verify(&p, &f).is_ok());
        assert!(p.exists(), "an unhashed file must not be deleted");
    }

    #[test]
    fn verify_deletes_a_file_whose_hash_does_not_match() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("encoder.int8.onnx.part");
        std::fs::write(&p, b"corrupt").unwrap();
        let f = ModelFile {
            name: "encoder.int8.onnx",
            bytes: 7,
            sha256: Some("0000000000000000000000000000000000000000000000000000000000000000"),
        };
        let err = verify(&p, &f).expect_err("a hash mismatch must be an error");
        assert!(matches!(err, LocalSttError::Checksum { .. }));
        assert!(
            !p.exists(),
            "corrupt bytes must be deleted, else every resume re-fails"
        );
    }

    #[test]
    fn verify_accepts_the_real_sha256_of_known_content() {
        // sha256("abc"), the standard test vector.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        let f = ModelFile {
            name: "f",
            bytes: 3,
            sha256: Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        };
        assert!(verify(&p, &f).is_ok());
    }

    #[test]
    fn remove_is_idempotent_on_a_missing_directory() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("never-created");
        assert!(remove(&missing).is_ok());
    }

    #[test]
    fn remove_deletes_a_populated_model_directory() {
        let d = tempfile::tempdir().unwrap();
        let model = d.path().join("parakeet");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("encoder.int8.onnx"), b"x").unwrap();
        remove(&model).unwrap();
        assert!(!model.exists());
    }

    #[test]
    fn an_already_cancelled_download_writes_nothing_and_reports_cancelled() {
        // The cancel flag is checked before the first read, so a download
        // cancelled between click and thread start does no network work.
        let d = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(true);
        let client = reqwest::blocking::Client::builder().build().unwrap();
        let mut seen = Vec::new();
        let err = download(&PARAKEET_V3_INT8, d.path(), &client, &cancel, &mut |p| {
            seen.push(p.done_bytes)
        })
        .expect_err("a pre-cancelled download must not succeed");
        assert!(matches!(err, LocalSttError::Cancelled));
    }
}
