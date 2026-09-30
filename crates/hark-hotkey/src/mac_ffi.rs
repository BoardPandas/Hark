//! Small Quartz/Core Foundation ABI boundary, verified against the macOS SDK.
use std::ffi::c_void;
pub(crate) type Ref = *mut c_void;
pub(crate) type Callback = unsafe extern "C" fn(Ref, u32, Ref, Ref) -> Ref;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: Callback,
        info: Ref,
    ) -> Ref;
    pub(crate) fn CGEventTapEnable(tap: Ref, enable: bool);
    pub(crate) fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    pub(crate) fn CGEventGetFlags(event: Ref) -> u64;
    pub(crate) fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(crate) static kCFRunLoopDefaultMode: Ref;
    pub(crate) fn CFRunLoopGetCurrent() -> Ref;
    pub(crate) fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    pub(crate) fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    pub(crate) fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: u8) -> i32;
    pub(crate) fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    pub(crate) fn CFMachPortInvalidate(port: Ref);
    pub(crate) fn CFRelease(value: Ref);
}
