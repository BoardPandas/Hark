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
    // builds always are). A dev/unsigned running build cannot anchor, so fall
    // back to the trusted-signature check alone, loudly.
    match std::env::current_exe().ok().and_then(|p| signer_subject(&p).ok()) {
        Some(running_signer) if running_signer == staged_signer => {
            log::info!("update signer verified: {staged_signer}");
            Ok(())
        }
        Some(running_signer) => Err(UpdateError::Verification(format!(
            "downloaded update is signed by \"{staged_signer}\", not the running app's publisher \"{running_signer}\""
        ))),
        None => {
            log::warn!(
                "running exe is unsigned; accepting update on trusted-signature check alone \
                 (downloaded signer: {staged_signer})"
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
