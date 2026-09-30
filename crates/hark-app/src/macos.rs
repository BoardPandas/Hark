//! Small AppKit/permission bridge. UI entry points are main-thread only.
use std::ffi::{c_char, CString};

unsafe extern "C" {
    fn hark_microphone_status() -> i32;
    fn hark_request_microphone();
    fn hark_accessibility_status() -> bool;
    fn hark_keyboard_status() -> bool;
    fn hark_request_accessibility();
    fn hark_request_keyboard();
    fn hark_place_overlay(title: *const c_char, primary: bool);
    fn hark_overlay_visible(title: *const c_char, visible: bool) -> bool;
    fn hark_share_text(text: *const c_char) -> bool;
    fn hark_save_file(name: *const c_char, extension: *const c_char) -> *mut c_char;
    fn hark_free_path(path: *mut c_char);
}

pub fn microphone_ready() -> bool {
    // SAFETY: AVFoundation's authorization query takes no pointers and is thread-safe.
    unsafe { hark_microphone_status() == 3 }
}

pub fn permission_controls(ui: &mut egui::Ui) {
    // SAFETY: permission queries neither retain pointers nor prompt.
    let (mic, accessibility, keyboard) = unsafe {
        (
            hark_microphone_status(),
            hark_accessibility_status(),
            hark_keyboard_status(),
        )
    };
    ui.label(format!(
        "Microphone: {}",
        match mic {
            3 => "Allowed",
            0 => "Not requested",
            1 => "Restricted by this Mac",
            _ => "Not allowed",
        }
    ));
    if mic == 0 && ui.button("Allow microphone access").clicked() {
        // SAFETY: this starts an asynchronous system prompt; no borrowed data escapes.
        unsafe { hark_request_microphone() };
    }
    settings_button(ui, "Microphone settings", "Privacy_Microphone");
    ui.add_space(crate::theme::ROW_GAP);
    ui.label(format!(
        "Text insertion: {}",
        if accessibility {
            "Allowed"
        } else {
            "Needs Accessibility access"
        }
    ));
    if !accessibility && ui.button("Allow text insertion").clicked() {
        // SAFETY: called on the main thread in response to the user's action.
        unsafe { hark_request_accessibility() };
    }
    settings_button(ui, "Accessibility settings", "Privacy_Accessibility");
    ui.add_space(crate::theme::ROW_GAP);
    ui.label(format!(
        "Global shortcuts: {}",
        if keyboard {
            "Allowed"
        } else {
            "Needs Input Monitoring access"
        }
    ));
    if !keyboard && ui.button("Allow global shortcuts").clicked() {
        // SAFETY: called on the main thread in response to the user's action.
        unsafe { hark_request_keyboard() };
    }
    settings_button(ui, "Input Monitoring settings", "Privacy_ListenEvent");
    ui.label(
        egui::RichText::new(
            "After changing access, retry dictation below. If macOS asks, quit and reopen Hark.",
        )
        .small()
        .weak(),
    );
    // Refresh only while this settings surface is visible, never while idle in the tray.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(1));
}

pub fn settings_button(ui: &mut egui::Ui, label: &str, pane: &str) {
    if ui.button(label).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
            "x-apple.systempreferences:com.apple.preference.security?{pane}"
        )));
    }
}

pub fn place_overlay(title: &str, primary: bool) {
    if let Ok(title) = CString::new(title) {
        // SAFETY: eframe calls this on the main thread; title lives through the call.
        unsafe { hark_place_overlay(title.as_ptr(), primary) };
    }
}

pub fn overlay_visible(title: &str, visible: bool) -> bool {
    let Ok(title) = CString::new(title) else {
        return false;
    };
    // SAFETY: main-thread eframe callback, native code borrows title only here.
    unsafe { hark_overlay_visible(title.as_ptr(), visible) }
}

pub fn share_text(text: &str) -> Result<(), &'static str> {
    let text =
        CString::new(text).map_err(|_| "The transcript contains an invalid null character.")?;
    // SAFETY: main-thread UI callback; native code copies text before returning.
    if unsafe { hark_share_text(text.as_ptr()) } {
        Ok(())
    } else {
        Err("Open the Hark window to share this meeting.")
    }
}

pub fn save_file(name: &str, extension: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(name).ok()?;
    let extension = CString::new(extension).ok()?;
    // SAFETY: called only by the export worker; the bridge copies the strings,
    // presents on the main queue, then returns a malloc-owned filesystem path.
    let path = unsafe { hark_save_file(name.as_ptr(), extension.as_ptr()) };
    if path.is_null() {
        return None;
    }
    // SAFETY: the native function returns a null-terminated allocation. Copy
    // its bytes losslessly before returning ownership to the native allocator.
    let result = unsafe {
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
            std::ffi::CStr::from_ptr(path).to_bytes(),
        ))
    };
    unsafe { hark_free_path(path) };
    Some(result)
}
