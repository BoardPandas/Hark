//! Authenticode verification of a downloaded update before it replaces the
//! running exe. Two gates, both must pass on Windows:
//!
//! 1. `WinVerifyTrust` — the file has a valid Authenticode signature that
//!    chains to a trusted root (rejects unsigned, tampered, or untrusted).
//! 2. Signer-subject match — the downloaded exe's signer equals the *running*
//!    exe's signer, so a validly-signed binary from a *different* publisher is
//!    still refused. This self-anchors to whoever signed the installed copy; no
//!    certificate string is hardcoded.
//!
//! On non-Windows targets there is no published artifact to self-install, so
//! this is a stub that refuses; the UI opens the release page instead.

use std::path::Path;

use crate::UpdateError;

#[cfg(not(windows))]
pub fn verify(_staged: &Path) -> Result<(), UpdateError> {
    Err(UpdateError::Verification(
        "self-install is only supported on Windows".to_string(),
    ))
}

#[cfg(windows)]
pub fn verify(staged: &Path) -> Result<(), UpdateError> {
    verify_trust(staged)?;
    let staged_signer = signer_subject(staged)?;
    // Anchor to the running exe's signer when it is itself signed (release
    // builds always are). A dev/unsigned running build cannot anchor.
    let running = std::env::current_exe()
        .ok()
        .and_then(|p| signer_subject(&p).ok());
    decide_signer(&staged_signer, running.as_deref())
}

/// Gate 2 in isolation: given the downloaded exe's signer subject and the
/// running exe's (if it has a readable one), decide whether the update may
/// proceed.
///
/// Split out from the Win32 calls deliberately. This is the half that decides
/// whether a validly-signed binary is allowed to replace the running one, and
/// as a free function over two strings it can be tested on every platform
/// rather than only where `WinVerifyTrust` exists.
///
/// An **empty or whitespace-only** subject is treated as *unreadable*, never as
/// a value that can match. `cert_name_string` returns `String::new()` when
/// `CertGetNameStringW` reports no name, so without this a certificate with no
/// readable subject on both sides would compare equal and wave the update
/// through on a signer check that had in fact read nothing.
#[cfg_attr(not(windows), allow(dead_code))] // used by `verify` on Windows, and by the tests everywhere
fn decide_signer(staged: &str, running: Option<&str>) -> Result<(), UpdateError> {
    let staged = staged.trim();
    if staged.is_empty() {
        return Err(UpdateError::Verification(
            "downloaded update carries a signature with no readable signer name".to_string(),
        ));
    }

    match running.map(str::trim).filter(|r| !r.is_empty()) {
        Some(running) if running == staged => {
            log::info!("update signer verified: {staged}");
            Ok(())
        }
        Some(running) => Err(UpdateError::Verification(format!(
            "downloaded update is signed by \"{staged}\", not the running app's publisher \"{running}\""
        ))),
        // Falls back to the trusted-signature check alone, loudly.
        None => {
            log::warn!(
                "running exe is unsigned; accepting update on trusted-signature check alone \
                 (downloaded signer: {staged})"
            );
            Ok(())
        }
    }
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn verify_trust(path: &Path) -> Result<(), UpdateError> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    let path_w = wide(path);
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path_w.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };

    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        ..Default::default()
    };

    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        )
    };

    // Always release the state data, regardless of the verdict.
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        );
    }

    if status != 0 {
        return Err(UpdateError::Verification(format!(
            "Authenticode signature is not valid (0x{:08X})",
            status as u32
        )));
    }
    Ok(())
}

/// The signer's "simple display" subject (e.g. the org name) from the exe's
/// embedded PKCS#7 signature.
#[cfg(windows)]
fn signer_subject(path: &Path) -> Result<String, UpdateError> {
    use std::ffi::c_void;
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext, CryptMsgClose,
        CryptMsgGetParam, CryptQueryObject, CERT_FIND_SUBJECT_CERT, CERT_INFO,
        CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_ENCODING_TYPE,
        CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_CERT_INFO_PARAM,
        HCERTSTORE,
    };

    let path_w = wide(path);
    let mut encoding = CERT_QUERY_ENCODING_TYPE::default();
    let mut store = HCERTSTORE::default();
    // The message handle is an untyped HCRYPTMSG (a raw pointer) in this
    // windows-rs version.
    let mut msg: *mut c_void = std::ptr::null_mut();

    unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path_w.as_ptr() as *const c_void,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            Some(&mut encoding),
            None,
            None,
            Some(&mut store),
            Some(&mut msg),
            None,
        )
    }
    .map_err(|e| UpdateError::Verification(format!("no embedded signature: {e}")))?;

    let result = (|| {
        // Size, then fetch, the signer's CERT_INFO from the message.
        let mut size: u32 = 0;
        unsafe { CryptMsgGetParam(msg, CMSG_SIGNER_CERT_INFO_PARAM, 0, None, &mut size) }
            .map_err(|e| UpdateError::Verification(format!("no signer info: {e}")))?;
        let mut buf = vec![0u8; size as usize];
        unsafe {
            CryptMsgGetParam(
                msg,
                CMSG_SIGNER_CERT_INFO_PARAM,
                0,
                Some(buf.as_mut_ptr() as *mut c_void),
                &mut size,
            )
        }
        .map_err(|e| UpdateError::Verification(format!("no signer info: {e}")))?;

        let cert_info = buf.as_ptr() as *const CERT_INFO;
        let cert = unsafe {
            CertFindCertificateInStore(
                store,
                encoding,
                0,
                CERT_FIND_SUBJECT_CERT,
                Some(cert_info as *const c_void),
                None,
            )
        };
        if cert.is_null() {
            return Err(UpdateError::Verification(
                "signer certificate not found".to_string(),
            ));
        }

        let subject = unsafe { cert_name_string(cert) };
        let _ = unsafe { CertFreeCertificateContext(Some(cert)) };
        Ok(subject)
    })();

    unsafe {
        let _ = CryptMsgClose(Some(msg));
        let _ = CertCloseStore(Some(store), 0);
    }
    result
}

/// Read the simple display name (subject) off a certificate context.
#[cfg(windows)]
unsafe fn cert_name_string(
    cert: *const windows::Win32::Security::Cryptography::CERT_CONTEXT,
) -> String {
    use windows::Win32::Security::Cryptography::{
        CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    };

    let len = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
    if len <= 1 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize];
    let written = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buf));
    // `written` counts the trailing NUL; trim it.
    let end = (written as usize).saturating_sub(1).min(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `UpdateError` has no `PartialEq`, so assert on the rendered message.
    fn err(result: Result<(), UpdateError>) -> String {
        match result {
            Err(e) => e.to_string(),
            Ok(()) => panic!("expected the update to be refused, but it was accepted"),
        }
    }

    #[test]
    fn matching_publisher_is_accepted() {
        assert!(decide_signer("Board Pandas LLC", Some("Board Pandas LLC")).is_ok());
    }

    #[test]
    fn a_different_publisher_is_refused_even_though_it_is_validly_signed() {
        // The whole point of gate 2: WinVerifyTrust already passed on this file.
        let msg = err(decide_signer(
            "Totally Legit Software",
            Some("Board Pandas LLC"),
        ));
        assert!(msg.contains("Totally Legit Software"), "{msg}");
        assert!(msg.contains("Board Pandas LLC"), "{msg}");
    }

    #[test]
    fn an_unsigned_running_exe_falls_back_to_the_trust_check_alone() {
        // A dev build cannot anchor. Accepting here is deliberate; the file has
        // still passed WinVerifyTrust before this function is reached.
        assert!(decide_signer("Board Pandas LLC", None).is_ok());
    }

    #[test]
    fn an_unreadable_running_subject_is_treated_as_unsigned_not_as_a_match() {
        // `cert_name_string` yields "" when CertGetNameStringW reports no name.
        assert!(decide_signer("Board Pandas LLC", Some("")).is_ok());
        assert!(decide_signer("Board Pandas LLC", Some("   ")).is_ok());
    }

    #[test]
    fn an_unreadable_staged_subject_is_refused_outright() {
        // The regression this guards: two empty subjects used to compare equal,
        // so a signer check that had read nothing reported a publisher match.
        assert!(err(decide_signer("", Some(""))).contains("no readable signer name"));
        assert!(err(decide_signer("   ", None)).contains("no readable signer name"));
        assert!(
            err(decide_signer("", Some("Board Pandas LLC"))).contains("no readable signer name")
        );
    }

    #[test]
    fn subject_comparison_ignores_surrounding_whitespace_but_not_case() {
        assert!(decide_signer("  Board Pandas LLC  ", Some("Board Pandas LLC")).is_ok());
        // Certificate subjects are compared exactly; a case-folded near-miss is
        // a different publisher, not the same one.
        assert!(decide_signer("board pandas llc", Some("Board Pandas LLC")).is_err());
    }
}
