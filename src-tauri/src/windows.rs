//! Window capture on the Mac (PLAN.md §2, M1): the list of windows on screen, front to back, for the overlay's
//! hover highlight; and one window captured on its own with ScreenCaptureKit, with or without its shadow, whatever
//! is covering it. The same picture as the Mac's own window screenshot (`screencapture -l<id>`; `-o` for no shadow),
//! which the self-test compares against.

use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFArray, CFRetained};
use objc2_core_graphics::{CGImage, CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{SCContentFilter, SCScreenshotManager, SCShareableContent};

use crate::capture::Rect;

/// A window on screen, as the overlay needs it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowInfo {
    pub id: u32,
    /// Global points, top-left origin.
    pub bounds: Rect,
    pub pid: i32,
    pub app: String,
    /// Empty when the app doesn't name the window.
    pub title: String,
    pub layer: i32,
}

fn number(dict: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<f64> {
    let v = dict.objectForKey(&NSString::from_str(key))?;
    v.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

fn string(dict: &NSDictionary<NSString, AnyObject>, key: &str) -> String {
    dict.objectForKey(&NSString::from_str(key))
        .and_then(|v| v.downcast::<NSString>().ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Windows on screen, frontmost first: ordinary app windows plus menus, pop-ups and panels (anything an app draws
/// above the desktop), but not the desktop, the menu bar, the Dock, invisible or tiny windows, or MingShot's own.
pub fn on_screen() -> Vec<WindowInfo> {
    let option =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(option, 0) else {
        return Vec::new();
    };
    // SAFETY: a CFArray of CFDictionaries is toll-free bridged to NSArray<NSDictionary>; `list` outlives the borrow.
    let array: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const CFArray as *const _) };
    let own = std::process::id() as i32;
    let mut out = Vec::new();
    for dict in array.iter() {
        let pid = number(&dict, "kCGWindowOwnerPID").unwrap_or(0.0) as i32;
        let layer = number(&dict, "kCGWindowLayer").unwrap_or(0.0) as i32;
        let alpha = number(&dict, "kCGWindowAlpha").unwrap_or(1.0);
        let app = string(&dict, "kCGWindowOwnerName");
        // Layer 0 is ordinary windows; 1-24 floating panels and the like; 24/25 the menu bar and its status items;
        // the Dock is 20. Menus and pop-ups sit at 101 and up, below the screen saver (1000).
        let wanted = (0..20).contains(&layer) || (101..1000).contains(&layer);
        if pid == own || alpha <= 0.0 || !wanted || app == "Window Server" || app == "Dock" {
            continue;
        }
        let Some(b) = dict
            .objectForKey(&NSString::from_str("kCGWindowBounds"))
            .and_then(|v| v.downcast::<NSDictionary>().ok())
        else {
            continue;
        };
        // SAFETY: the bounds dictionary has NSString keys and NSNumber values.
        let b: &NSDictionary<NSString, AnyObject> = unsafe { &*(Retained::as_ptr(&b) as *const _) };
        let bounds = Rect {
            x: number(b, "X").unwrap_or(0.0),
            y: number(b, "Y").unwrap_or(0.0),
            w: number(b, "Width").unwrap_or(0.0),
            h: number(b, "Height").unwrap_or(0.0),
        };
        if bounds.w < 24.0 || bounds.h < 24.0 {
            continue;
        }
        out.push(WindowInfo {
            id: number(&dict, "kCGWindowNumber").unwrap_or(0.0) as u32,
            bounds,
            pid,
            app,
            title: string(&dict, "kCGWindowName"),
            layer,
        });
    }
    out
}

struct Content(Retained<SCShareableContent>);
unsafe impl Send for Content {}

fn shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel::<Result<Content, String>>();
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result = match NonNull::new(content) {
                // SAFETY: a live SCShareableContent handed to the completion handler; retain it to keep it.
                Some(c) => Ok(Content(unsafe { Retained::retain(c.as_ptr()) }.unwrap())),
                None => Err(error_text(error, "no shareable content")),
            };
            let _ = tx.send(result);
        },
    );
    // SAFETY: documented ScreenCaptureKit API; the block is called once.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "ScreenCaptureKit didn't answer".to_string())?
        .map(|c| c.0)
}

fn error_text(error: *mut NSError, fallback: &str) -> String {
    match NonNull::new(error) {
        // SAFETY: a live NSError passed to a completion handler.
        Some(e) => unsafe { e.as_ref() }.localizedDescription().to_string(),
        None => fallback.to_string(),
    }
}

struct Image(CFRetained<CGImage>);
unsafe impl Send for Image {}

/// One window on its own, at full pixel resolution, with or without its shadow (outside the window the image is
/// transparent). Covered parts come out as the window drew them. Uses macOS 26's `captureScreenshotWithFilter`, whose
/// `ignoreShadows` gives exactly the Mac's own window screenshot both ways (`screencapture -l`, `-o -l`); the older
/// `captureImageWithFilter` can't size the shadow (docs/M1.md).
pub fn capture(window_id: u32, shadow: bool) -> Result<CFRetained<CGImage>, String> {
    use objc2_screen_capture_kit::{SCScreenshotConfiguration, SCScreenshotOutput};
    let content = shareable_content()?;
    // SAFETY: property reads on a retained SCShareableContent.
    let window = unsafe { content.windows() }
        .iter()
        .find(|w| unsafe { w.windowID() } == window_id)
        .ok_or("that window has closed")?;
    // SAFETY: documented ScreenCaptureKit initialisers and setters on fresh objects.
    let (filter, config) = unsafe {
        let filter =
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window);
        let config = SCScreenshotConfiguration::new();
        config.setShowsCursor(false);
        config.setIgnoreShadows(!shadow);
        (filter, config)
    };
    let (tx, rx) = mpsc::channel::<Result<Image, String>>();
    let block = RcBlock::new(move |out: *mut SCScreenshotOutput, error: *mut NSError| {
        // SAFETY: a live SCScreenshotOutput handed to the completion handler.
        let image = NonNull::new(out).and_then(|o| unsafe { o.as_ref().sdrImage() });
        let result = match image {
            Some(img) => {
                // The binding hands the CGImage back as a Retained; it's a CF object, so re-wrap the same +1.
                let raw = Retained::into_raw(img);
                // SAFETY: `raw` is a non-null CGImage we own one reference to.
                Ok(Image(unsafe {
                    CFRetained::from_raw(NonNull::new(raw).unwrap())
                }))
            }
            None => Err(error_text(error, "window capture failed")),
        };
        let _ = tx.send(result);
    });
    // SAFETY: documented API (macOS 26+); the block is called once.
    unsafe {
        SCScreenshotManager::captureScreenshotWithFilter_configuration_completionHandler(
            &filter,
            &config,
            Some(&block),
        )
    };
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "the window didn't return a picture in 5 s".to_string())?
        .map(|i| i.0)
}
