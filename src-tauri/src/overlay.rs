//! The capture overlay on the Mac (PLAN.md §4). For each display there are two windows above everything else:
//!
//! - a native "frozen frame" window whose layer shows the captured CGImage as is: no encoding, no copy into the
//!   webview, so it appears as fast as the window server can draw it, in the display's own colours;
//! - above it, a transparent Tauri webview window (`overlay-<display id>`) for the selection UI.
//!
//! Both are made ahead of time and only ordered in and out on New. The selection UI goes up the moment New is
//! clicked, before the screen is read; the frozen frame slides in under it when it arrives. That works because
//! every MingShot window is marked not shareable (`NSWindowSharingNone`) while the displays are being read, and
//! `captureImageInRect` leaves such windows out (bench mode checks both). Once every frame is in they're made
//! shareable again, so screen sharing and remote desktops (RustDesk) see the overlay. Main thread only.

use std::cell::RefCell;
use std::collections::HashMap;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSEvent, NSScreen, NSWindow,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask,
};
use objc2_core_foundation::CGRect;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSNumber, NSString};
use objc2_quartz_core::{CATransaction, kCAGravityResize};

/// Above the menu bar, the Dock and pop-up menus (CGShieldingWindowLevel territory starts higher still).
const FRAME_LEVEL: isize = 1000;
pub const UI_LEVEL: isize = 1001;

thread_local! {
    /// Frozen-frame windows by display id. Main thread only.
    static FRAMES: RefCell<HashMap<u32, Retained<NSWindow>>> = RefCell::new(HashMap::new());
}

/// The Cocoa frame (bottom-left origin) of each screen, by CoreGraphics display id.
pub fn screen_frames(mtm: MainThreadMarker) -> HashMap<u32, CGRect> {
    let mut out = HashMap::new();
    for screen in NSScreen::screens(mtm).iter() {
        let desc = screen.deviceDescription();
        let key = NSString::from_str("NSScreenNumber");
        if let Some(n) = desc.objectForKey(&key)
            && let Ok(n) = n.downcast::<NSNumber>()
        {
            out.insert(n.unsignedIntValue(), screen.frame());
        }
    }
    out
}

/// The pointer in CoreGraphics' global top-left-origin points.
pub fn mouse_cg(mtm: MainThreadMarker) -> (f64, f64) {
    let p = NSEvent::mouseLocation();
    let main_h = NSScreen::screens(mtm)
        .firstObject()
        .map_or(0.0, |s| s.frame().size.height);
    (p.x, main_h - p.y)
}

/// The display under the pointer, if any.
pub fn display_under_mouse(mtm: MainThreadMarker) -> Option<u32> {
    let p = NSEvent::mouseLocation();
    screen_frames(mtm).into_iter().find_map(|(id, f)| {
        let inside = p.x >= f.origin.x
            && p.x < f.origin.x + f.size.width
            && p.y >= f.origin.y
            && p.y < f.origin.y + f.size.height;
        inside.then_some(id)
    })
}

fn behaviour() -> NSWindowCollectionBehavior {
    NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle
}

/// Makes the frozen-frame window for a display if it doesn't exist yet, and places it over the display.
pub fn ensure_frame_window(mtm: MainThreadMarker, display: u32, frame: CGRect) {
    FRAMES.with_borrow_mut(|map| {
        let win = map.entry(display).or_insert_with(|| {
            // SAFETY: documented AppKit initialiser on the main thread.
            let w = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    mtm.alloc(),
                    frame,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe { w.setReleasedWhenClosed(false) };
            w.setLevel(FRAME_LEVEL);
            w.setSharingType(NSWindowSharingType::None);
            w.setAnimationBehavior(NSWindowAnimationBehavior::None);
            w.setCollectionBehavior(behaviour());
            w.setIgnoresMouseEvents(true);
            w.setHasShadow(false);
            w.setOpaque(true);
            w.setBackgroundColor(Some(&NSColor::blackColor()));
            if let Some(view) = w.contentView() {
                view.setWantsLayer(true);
            }
            w
        });
        win.setFrame_display(frame, false);
    });
}

/// Settings an overlay window keeps between captures, applied while it's hidden so New only has to put it on
/// screen: above everything, on every Space, no animation, not draggable by its background, and not shareable (so
/// it can be up before the displays are read). Changing the sharing type right as the capture requests go out
/// coincided with ~500 ms stalls in the bench (docs/M0.md), so it's never done on New's path.
pub fn prepare_ui(win: &NSWindow) {
    win.setSharingType(NSWindowSharingType::None);
    win.setLevel(UI_LEVEL);
    win.setCollectionBehavior(behaviour());
    win.setAnimationBehavior(NSWindowAnimationBehavior::None);
    win.setMovable(false);
    win.setMovableByWindowBackground(false);
}

/// Puts the selection UI over each display (already prepared: `prepare_ui`), then makes MingShot the active app and
/// the overlay under the pointer (`key`) the key window, so Esc reaches it. The overlay is handed to the window
/// server (CATransaction flush) before the focus change; the screen shows it on its next refresh.
pub fn show_ui(mtm: MainThreadMarker, ui: &[(u32, &NSWindow)], key: Option<u32>) {
    let t = std::time::Instant::now();
    let screens = screen_frames(mtm);
    let mut key_window = None;
    for (display, win) in ui {
        if let Some(rect) = screens.get(display)
            && win.frame() != *rect
        {
            win.setFrame_display(*rect, false);
        }
        win.setIgnoresMouseEvents(false);
        win.setAlphaValue(1.0);
        win.orderFrontRegardless();
        if Some(*display) == key {
            key_window = Some(*win);
        }
    }
    // On screen first: the flush hands the overlay to the window server, and nothing else is waited on.
    CATransaction::flush();
    let t_shown = t.elapsed();
    // Then focus, so Esc reaches the overlay under the pointer. Activating is a handshake with the app that was
    // active, which can take ~500 ms; done before the flush, it held the overlay back by that much in a few runs in
    // ten (docs/M0.md). Activate before making the window key, or AppKit hands key status back to whichever
    // MingShot window had it.
    let app = NSApplication::sharedApplication(mtm);
    if !app.isActive() {
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    if let Some(win) = key_window.or(ui.first().map(|(_, w)| *w)) {
        win.makeKeyAndOrderFront(None);
    }
    if t_shown.as_millis() > 40 {
        crate::log::event(&format!("slow show_ui: on screen after {t_shown:?}"));
    }
}

/// Puts a display's frozen frame on screen under its selection UI.
pub fn show_frame(mtm: MainThreadMarker, display: u32, image: &CGImage) {
    let Some(rect) = screen_frames(mtm).get(&display).copied() else {
        return;
    };
    ensure_frame_window(mtm, display, rect);
    FRAMES.with_borrow(|map| {
        let win = &map[&display];
        if let Some(layer) = win.contentView().and_then(|v| v.layer()) {
            // SAFETY: a CGImage is a valid layer `contents` object; the layer retains it.
            unsafe {
                let obj = &*(image as *const CGImage as *const AnyObject);
                layer.setContents(Some(obj));
                layer.setContentsGravity(kCAGravityResize);
            }
        }
        win.orderFrontRegardless();
    });
    let t = std::time::Instant::now();
    CATransaction::flush();
    if t.elapsed().as_millis() > 40 {
        crate::log::event(&format!("slow show_frame: flush {:?}", t.elapsed()));
    }
}

/// Every display has been read: let screen sharing see the overlay again.
pub fn make_shareable(ui: &[&NSWindow]) {
    for win in ui {
        win.setSharingType(NSWindowSharingType::ReadOnly);
    }
    FRAMES.with_borrow(|map| {
        for win in map.values() {
            win.setSharingType(NSWindowSharingType::ReadOnly);
        }
    });
}

/// Puts the overlays away, lets go of the frozen frames (a 4K frame is 33 MB) and gets the windows ready for the
/// next New (not shareable again, among the rest: `prepare_ui`).
///
/// The selection-UI windows are not ordered out: they stay on screen, fully transparent and letting every click
/// through. A webview that has been off screen makes AppKit wait for its web process when it comes back, and that
/// wait timed out at ~500 ms in a few runs in ten (docs/M0.md). Kept on screen, it never goes hidden.
pub fn hide(ui: &[&NSWindow]) {
    for win in ui {
        win.setAlphaValue(0.0);
        win.setIgnoresMouseEvents(true);
        prepare_ui(win);
        win.orderFrontRegardless();
    }
    FRAMES.with_borrow(|map| {
        for win in map.values() {
            win.orderOut(None);
            win.setSharingType(NSWindowSharingType::None);
            if let Some(layer) = win.contentView().and_then(|v| v.layer()) {
                unsafe { layer.setContents(None) };
            }
        }
    });
    CATransaction::flush();
}

/// A window's frame in CoreGraphics' global top-left-origin points (Cocoa's origin is bottom-left of the main
/// screen).
pub fn cg_frame(mtm: MainThreadMarker, win: &NSWindow) -> crate::capture::Rect {
    let f = win.frame();
    let main_h = NSScreen::screens(mtm)
        .firstObject()
        .map_or(0.0, |s| s.frame().size.height);
    crate::capture::Rect {
        x: f.origin.x,
        y: main_h - (f.origin.y + f.size.height),
        w: f.size.width,
        h: f.size.height,
    }
}

/// Takes the capture window off screen now and out of the capture. Not shareable first, so a capture started this
/// instant leaves it out; animation off, because macOS otherwise shrinks and fades it over several frames (bench
/// mode caught that in 5 of 30 runs before either fix).
pub fn order_out_now(win: &NSWindow) {
    win.setSharingType(NSWindowSharingType::None);
    win.setAnimationBehavior(NSWindowAnimationBehavior::None);
    win.orderOut(None);
    CATransaction::flush();
}

/// The capture window is back: visible to screen sharing and other screenshot tools again.
pub fn make_window_shareable(win: &NSWindow) {
    win.setSharingType(NSWindowSharingType::ReadOnly);
}
