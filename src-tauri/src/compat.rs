//! Running on every macOS from 10.14 Mojave on (PLAN.md §7.9). The newest capture calls aren't everywhere:
//!
//! | What | macOS 26+ | 15.2 – 25 | 10.14 – 15.1 |
//! |---|---|---|---|
//! | A display | ScreenCaptureKit `captureImageInRect` | same | CoreGraphics `CGDisplayCreateImage` |
//! | A window | ScreenCaptureKit `captureScreenshotWithFilter` | `CGWindowListCreateImage` | `CGWindowListCreateImage` |
//! | Screen Recording permission | `CGPreflightScreenCaptureAccess` | same | same from 10.15; Mojave has none |
//!
//! The CoreGraphics calls are deprecated (and missing from the newest SDKs' headers) but still present and working
//! as of macOS 27, so they're looked up by name at run time rather than linked. `MINGSHOT_LEGACY=1` forces the old
//! paths on a new Mac, so the self-test can check them against the Mac's own screenshots (docs/M1.md).

use std::ffi::{c_void, CString};
use std::ptr::NonNull;

use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};

/// Whether this Mac runs macOS `major.minor` or later.
pub fn os_at_least(major: isize, minor: isize) -> bool {
    let v = NSOperatingSystemVersion { majorVersion: major, minorVersion: minor, patchVersion: 0 };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(v)
}

/// The old CoreGraphics paths, forced (testing) or because the new ones aren't on this macOS.
fn forced_legacy() -> bool {
    std::env::var("MINGSHOT_LEGACY").is_ok_and(|v| v != "0")
}

/// Displays through ScreenCaptureKit's `captureImageInRect` (macOS 15.2+).
pub fn modern_displays() -> bool {
    !forced_legacy() && os_at_least(15, 2)
}

/// Windows through ScreenCaptureKit's `captureScreenshotWithFilter` (macOS 26+).
pub fn modern_windows() -> bool {
    !forced_legacy() && os_at_least(26, 0)
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// A C function by name, if this macOS has it.
fn symbol(name: &str) -> Option<NonNull<c_void>> {
    let name = CString::new(name).ok()?;
    // SAFETY: dlsym with RTLD_DEFAULT and a valid C string.
    NonNull::new(unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) })
}

/// Screen Recording: granted? Mojave has no such permission, so always.
pub fn has_permission() -> bool {
    match symbol("CGPreflightScreenCaptureAccess") {
        // SAFETY: `bool CGPreflightScreenCaptureAccess(void)` (macOS 10.15+).
        Some(f) => unsafe { std::mem::transmute::<*mut c_void, extern "C" fn() -> bool>(f.as_ptr())() },
        None => true,
    }
}

/// Screen Recording: ask (the system prompt, the first time) and return whether it's granted now.
pub fn request_permission() -> bool {
    match symbol("CGRequestScreenCaptureAccess") {
        // SAFETY: `bool CGRequestScreenCaptureAccess(void)` (macOS 10.15+).
        Some(f) => unsafe { std::mem::transmute::<*mut c_void, extern "C" fn() -> bool>(f.as_ptr())() },
        None => true,
    }
}

/// Wraps a CGImage returned under the Create rule (we own one reference).
fn owned(image: *mut CGImage) -> Option<CFRetained<CGImage>> {
    // SAFETY: a CGImage* from a Create function, owned by the caller.
    NonNull::new(image).map(|p| unsafe { CFRetained::from_raw(p) })
}

/// One display's current picture, through CoreGraphics (`CGDisplayCreateImage`).
pub fn display_image(display: u32) -> Result<CFRetained<CGImage>, String> {
    let f = symbol("CGDisplayCreateImage").ok_or("this macOS has no CGDisplayCreateImage")?;
    // SAFETY: `CGImageRef CGDisplayCreateImage(CGDirectDisplayID)`.
    let f: extern "C" fn(u32) -> *mut CGImage = unsafe { std::mem::transmute(f.as_ptr()) };
    owned(f(display)).ok_or_else(|| format!("display {display} couldn't be read"))
}

/// One window on its own, through CoreGraphics (`CGWindowListCreateImage`), with or without its shadow, at the
/// display's full resolution: the call the Mac's own `screencapture -l` used before ScreenCaptureKit.
pub fn window_image(window: u32, shadow: bool) -> Result<CFRetained<CGImage>, String> {
    const INCLUDING_WINDOW: u32 = 1 << 3; // kCGWindowListOptionIncludingWindow
    const IGNORE_FRAMING: u32 = 1 << 0; // kCGWindowImageBoundsIgnoreFraming: no shadow
    const BEST_RESOLUTION: u32 = 1 << 3; // kCGWindowImageBestResolution
    let f = symbol("CGWindowListCreateImage").ok_or("this macOS has no CGWindowListCreateImage")?;
    // SAFETY: `CGImageRef CGWindowListCreateImage(CGRect, CGWindowListOption, CGWindowID, CGWindowImageOption)`.
    let f: extern "C" fn(CGRect, u32, u32, u32) -> *mut CGImage = unsafe { std::mem::transmute(f.as_ptr()) };
    let null_rect = CGRect::new(
        objc2_core_foundation::CGPoint::new(f64::INFINITY, f64::INFINITY),
        objc2_core_foundation::CGSize::new(0.0, 0.0),
    ); // CGRectNull: the window's own bounds
    let options = BEST_RESOLUTION | if shadow { 0 } else { IGNORE_FRAMING };
    owned(f(null_rect, INCLUDING_WINDOW, window, options)).ok_or_else(|| "that window couldn't be read".to_string())
}
