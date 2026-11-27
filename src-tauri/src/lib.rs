//! MingShot M0 (PLAN.md §6): a small capture window with a New button (§7.6); New puts a selection overlay over
//! every display at once, freezes each display under it as its frame arrives, and the region you drag goes on the
//! clipboard as a PNG (image only, §7.8). ⌃⇧2 does the same as New. Mac only for now. Measurements: docs/M0.md.

#[cfg(target_os = "macos")]
pub mod capture;
#[cfg(target_os = "macos")]
pub mod compat;
pub mod log;
#[cfg(target_os = "macos")]
pub mod output;
#[cfg(target_os = "macos")]
mod overlay;
#[cfg(target_os = "macos")]
pub mod windows;

#[cfg(target_os = "macos")]
mod app {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use objc2::MainThreadMarker;
    use objc2_app_kit::NSWindow;
    use serde::Serialize;
    use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

    use crate::capture::{self, Frame, Rect};
    use crate::{log, overlay};

    #[derive(Serialize, Clone, Copy, Default)]
    pub struct Timings {
        /// New (or ⌃⇧2) reaching MingShot → selection overlay on every display handed to the window server.
        ui_ms: f64,
        /// New → the pointer's display frozen under the overlay: the M0 target (< 150 ms).
        frozen_ms: f64,
        /// New → every display frozen.
        all_ms: f64,
        /// Reading the displays (in parallel), until the last frame arrived.
        capture_ms: f64,
        displays: usize,
        delay_s: u64,
    }

    struct Session {
        id: u64,
        /// The frames' pixels, copied on the magnifier's first look at each display.
        pixels: std::collections::HashMap<u32, Arc<capture::Pixels>>,
        frames: Vec<Arc<Frame>>,
        timings: Timings,
        /// Where the capture window was before New hid it (global points), for bench mode's check.
        hidden: Option<Rect>,
    }

    static SESSION: Mutex<Option<Session>> = Mutex::new(None);

    /// What New captures (the capture window's mode picker, PLAN.md §2). ⌃⇧2 uses the same.
    #[derive(Clone, Copy, PartialEq, Serialize, serde::Deserialize, Debug)]
    #[serde(rename_all = "lowercase")]
    pub enum Mode {
        /// Drag a region.
        Region,
        /// Click the window under the pointer (or drag a region).
        Window,
        /// Click a display (or drag a region).
        Display,
    }

    #[derive(Clone, Copy)]
    struct Settings {
        mode: Mode,
        /// Window mode: include the window's shadow.
        shadow: bool,
        /// Clipboard, the MingShot folder, or both.
        after: crate::output::After,
    }

    static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
        mode: Mode::Region,
        shadow: true,
        after: crate::output::After::Copy,
    });

    /// The last capture's PNG and where it went, for the status line's Copy / Save / Show in Finder / Open.
    struct Last {
        png: Vec<u8>,
        copied: bool,
        saved: Option<std::path::PathBuf>,
    }

    static LAST: Mutex<Option<Last>> = Mutex::new(None);

    /// Where the last capture is now (sent back after each action).
    #[derive(Serialize, Clone)]
    pub struct Delivered {
        copied: bool,
        /// The saved file's full path, if it's been saved.
        saved: Option<String>,
    }

    fn delivered(l: &Last) -> Delivered {
        Delivered {
            copied: l.copied,
            saved: l.saved.as_ref().map(|p| p.display().to_string()),
        }
    }

    /// A window the overlay can highlight, in points relative to the overlay's display.
    #[derive(Serialize, Clone)]
    struct Target {
        id: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        app: String,
        title: String,
    }

    #[derive(Serialize, Clone)]
    struct Targets {
        mode: Mode,
        /// Front to back.
        windows: Vec<Target>,
    }

    /// Tells each display's overlay the mode and, in window mode, the windows on it as they are now.
    fn send_targets(app: &AppHandle, displays: &[capture::Display]) {
        let mode = SETTINGS.lock().unwrap().mode;
        let all = if mode == Mode::Window {
            crate::windows::on_screen()
        } else {
            Vec::new()
        };
        for d in displays {
            let b = d.bounds;
            let windows = all
                .iter()
                .filter(|w| {
                    let r = w.bounds;
                    r.x < b.x + b.w && r.x + r.w > b.x && r.y < b.y + b.h && r.y + r.h > b.y
                })
                .map(|w| Target {
                    id: w.id,
                    x: w.bounds.x - b.x,
                    y: w.bounds.y - b.y,
                    w: w.bounds.w,
                    h: w.bounds.h,
                    app: w.app.clone(),
                    title: w.title.clone(),
                })
                .collect();
            let _ = app.emit_to(
                overlay_label(d.id).as_str(),
                "targets",
                Targets { mode, windows },
            );
        }
    }
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn ms(t: Instant) -> f64 {
        (t.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
    }

    /// Runs `f` on the main thread and waits for its result (directly when already there).
    fn on_main<R: Send + 'static>(
        app: &AppHandle,
        f: impl FnOnce(MainThreadMarker) -> R + Send + 'static,
    ) -> Result<R, String> {
        if let Some(mtm) = MainThreadMarker::new() {
            return Ok(f(mtm));
        }
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(f(MainThreadMarker::new().expect("main thread")));
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "main thread didn't answer".to_string())
    }

    fn ns_window(w: &tauri::WebviewWindow) -> Option<&NSWindow> {
        // SAFETY: Tauri's window pointer is a live NSWindow for as long as the Tauri window exists; only used on
        // the main thread.
        w.ns_window()
            .ok()
            .map(|p| unsafe { &*(p as *const NSWindow) })
    }

    fn overlay_label(display: u32) -> String {
        format!("overlay-{display}")
    }

    fn overlay_windows(app: &AppHandle) -> Vec<tauri::WebviewWindow> {
        app.webview_windows()
            .into_iter()
            .filter(|(l, _)| l.starts_with("overlay-"))
            .map(|(_, w)| w)
            .collect()
    }

    /// Makes the selection-UI window for every attached display that doesn't have one. Done at launch, so New only
    /// pays for it when a display has been plugged in since. Returns whether it made any.
    fn ensure_overlays(app: &AppHandle) -> bool {
        let mut made = false;
        for d in capture::displays() {
            let label = overlay_label(d.id);
            if app.get_webview_window(&label).is_some() {
                continue;
            }
            let url = WebviewUrl::App(format!("index.html?overlay={}", d.id).into());
            let built = WebviewWindowBuilder::new(app, &label, url)
                .title("MingShot overlay")
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .visible(false)
                .focused(false)
                .accept_first_mouse(true)
                .disable_drag_drop_handler()
                .visible_on_all_workspaces(true)
                .position(d.bounds.x, d.bounds.y)
                .inner_size(d.bounds.w, d.bounds.h)
                .build();
            match built {
                Ok(_) => made = true,
                Err(e) => log::event(&format!("overlay window for display {} failed: {e}", d.id)),
            }
        }
        made
    }

    fn current(id: u64) -> bool {
        SESSION.lock().unwrap().as_ref().is_some_and(|s| s.id == id)
    }

    /// New: hide the capture window, wait out the delay, put the overlay up, then freeze each display under it as
    /// its frame arrives. Returns when every display is frozen (or the selection was already made).
    pub fn begin(app: &AppHandle, delay_s: u64) -> Result<Timings, String> {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        {
            let mut session = SESSION.lock().unwrap();
            if session.is_some() {
                return Err("a capture is already in progress".into());
            }
            *session = Some(Session {
                id,
                pixels: Default::default(),
                frames: Vec::new(),
                timings: Timings::default(),
                hidden: None,
            });
        }
        let result = begin_inner(app, id, delay_s);
        if let Err(e) = &result {
            log::event(&format!("capture failed: {e}"));
            if current(id) {
                SESSION.lock().unwrap().take();
                hide_overlays(app);
                show_main(app);
            }
        }
        result
    }

    fn begin_inner(app: &AppHandle, id: u64, delay_s: u64) -> Result<Timings, String> {
        let mut t0 = Instant::now();
        if ensure_overlays(app) {
            hide_overlays(app); // a display was plugged in: prepare its new overlay
        }

        // Hide the capture window (and leave it out of captures) before anything else.
        let hidden = hide_main_for_capture(app)?;
        let legacy = !crate::compat::modern_displays();
        if delay_s > 0 {
            std::thread::sleep(Duration::from_secs(delay_s));
            t0 = Instant::now();
        }

        // Each display freezes under the overlay as soon as its frame lands, so the one under the pointer never
        // waits for a slower display. All were read at the same moment.
        let t1 = Instant::now();
        // Ask for the frames first, then put the overlay up: a little quicker to freeze than the other way round
        // (docs/M0.md), and the overlay is left out of the frames either way.
        let (displays, rx) = if legacy {
            // Before macOS 15.2 (CoreGraphics): read every display first, then put the overlay up. Whether
            // CoreGraphics leaves out a not-shareable window isn't proven on those systems, so the overlay waits.
            let frames = capture::capture_all()?;
            let (tx, rx) = std::sync::mpsc::channel();
            let displays: Vec<capture::Display> = frames.iter().map(|f| f.display).collect();
            for f in frames {
                let _ = tx.send(Ok(f));
            }
            (displays, rx)
        } else {
            capture::capture_each()?
        };
        // The overlay goes up at once; it's not shareable, so the capture can't see it.
        let a = app.clone();
        let queued = Instant::now();
        let key = on_main(app, move |mtm| {
            let waited = queued.elapsed();
            if waited.as_millis() > 40 {
                log::event(&format!(
                    "slow: main thread took {waited:?} to pick up show_ui"
                ));
            }
            let key = overlay::display_under_mouse(mtm);
            let windows: Vec<(u32, tauri::WebviewWindow)> = capture::displays()
                .iter()
                .filter_map(|d| {
                    a.get_webview_window(&overlay_label(d.id))
                        .map(|w| (d.id, w))
                })
                .collect();
            let ui: Vec<(u32, &NSWindow)> = windows
                .iter()
                .filter_map(|(id, w)| ns_window(w).map(|n| (*id, n)))
                .collect();
            overlay::show_ui(mtm, &ui);
            key
        })?;
        let ui_ms = ms(t0);
        send_targets(app, &displays);
        if let Some(display) = key {
            let rect = if std::env::var("MINGSHOT_E2E_SELECT").as_deref() == Ok("pointer") {
                // A click where the pointer is (window and display modes).
                let b = displays.iter().find(|d| d.id == display).map(|d| d.bounds);
                let p = on_main(app, overlay::mouse_cg)?;
                b.map(|b| Rect {
                    x: p.0 - b.x,
                    y: p.1 - b.y,
                    w: 0.0,
                    h: 0.0,
                })
            } else {
                e2e_selection()
            };
            if let Some(rect) = rect {
                let _ = app.emit_to(overlay_label(display).as_str(), "e2e-select", rect);
            }
        }
        if let Some(s) = SESSION.lock().unwrap().as_mut().filter(|s| s.id == id) {
            s.hidden = hidden;
        }

        let (mut frozen_ms, mut capture_ms) = (None, 0.0);
        for _ in 0..displays.len() {
            let frame = match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(f)) => Arc::new(f),
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("a display didn't return a frame in 5 s".into()),
            };
            capture_ms = ms(t1);
            let display = frame.display.id;
            let shown = frame.clone();
            // Checked on the main thread, where finish/cancel also hide the overlays, so a frame that lands just
            // after the selection was taken can't come back on screen.
            let still_on = on_main(app, move |mtm| {
                if !current(id) {
                    return false;
                }
                overlay::show_frame(mtm, display, &shown.image);
                true
            })?;
            if !still_on {
                // Cancelled, or a region already taken from a display that was ready: stop here.
                return Ok(Timings {
                    ui_ms,
                    displays: displays.len(),
                    delay_s,
                    ..Default::default()
                });
            }
            if Some(display) == key || (key.is_none() && frozen_ms.is_none()) {
                frozen_ms = Some(ms(t0));
            }
            if let Some(s) = SESSION.lock().unwrap().as_mut().filter(|s| s.id == id) {
                s.frames.push(frame);
            }
        }
        let a = app.clone();
        on_main(app, move |_| {
            let windows = overlay_windows(&a);
            let ui: Vec<&NSWindow> = windows.iter().filter_map(ns_window).collect();
            overlay::make_shareable(&ui);
        })?;
        // Every display is read: now MingShot may take focus (see overlay::focus).
        let a = app.clone();
        on_main(app, move |mtm| {
            if !current(id) {
                return;
            }
            let windows: Vec<(u32, tauri::WebviewWindow)> = overlay_windows(&a)
                .into_iter()
                .filter_map(|w| w.label().trim_start_matches("overlay-").parse().ok().map(|d| (d, w)))
                .collect();
            let ui: Vec<(u32, &NSWindow)> = windows
                .iter()
                .filter_map(|(id, w)| ns_window(w).map(|n| (*id, n)))
                .collect();
            overlay::focus(mtm, &ui, key);
        })?;
        let all_ms = ms(t0);
        let timings = Timings {
            ui_ms,
            frozen_ms: frozen_ms.unwrap_or(all_ms),
            all_ms,
            capture_ms,
            displays: displays.len(),
            delay_s,
        };
        log::event(&format!(
            "overlay up {} ms, pointer's display frozen {} ms, all {} ms (capture {} ms), {} displays, delay {} s",
            ui_ms, timings.frozen_ms, all_ms, capture_ms, timings.displays, delay_s
        ));
        if let Some(s) = SESSION.lock().unwrap().as_mut().filter(|s| s.id == id) {
            s.timings = timings;
        }
        Ok(timings)
    }

    /// Takes the capture window off screen and out of captures; returns where it was (global points) if it was up.
    fn hide_main_for_capture(app: &AppHandle) -> Result<Option<Rect>, String> {
        let a = app.clone();
        let hidden = on_main(app, move |mtm| {
            let main = a.get_webview_window("main")?;
            let win = ns_window(&main)?;
            if !win.isVisible() {
                return None;
            }
            let rect = overlay::cg_frame(mtm, win);
            overlay::order_out_now(win);
            Some(rect)
        })?;
        if !crate::compat::modern_displays() && hidden.is_some() {
            // CoreGraphics reads whatever is on screen now: give the window server a frame or two to drop the
            // capture window (ScreenCaptureKit leaves it out because it's marked not shareable; compat.rs).
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(hidden)
    }

    fn hide_overlays(app: &AppHandle) {
        for w in overlay_windows(app) {
            let _ = w.emit_to(w.label(), "thawed", ());
        }
        let a = app.clone();
        let _ = on_main(app, move |_| {
            let windows = overlay_windows(&a);
            let ui: Vec<&NSWindow> = windows.iter().filter_map(ns_window).collect();
            overlay::hide(&ui);
        });
    }

    fn show_main(app: &AppHandle) {
        let a = app.clone();
        let _ = on_main(app, move |_| {
            if let Some(main) = a.get_webview_window("main") {
                if let Some(win) = ns_window(&main) {
                    overlay::make_window_shareable(win);
                }
                let _ = main.show();
                let _ = main.set_focus();
            }
        });
    }

    #[derive(Serialize, Clone)]
    pub struct Captured {
        width: usize,
        height: usize,
        bytes: usize,
        copy_ms: f64,
        timings: Timings,
        /// A dragged region (for Repeat); none for a window or display.
        region: Option<LastRegion>,
        /// This was Repeat: the last region again, no overlay.
        repeated: bool,
        /// Where it went: the clipboard and/or a file.
        delivered: Delivered,
    }

    /// The last dragged region: its display and the rectangle in points on it (M1, "repeat last region").
    #[derive(Serialize, serde::Deserialize, Clone, Copy, Debug)]
    pub struct LastRegion {
        display: u32,
        rect: Rect,
    }

    static LAST_REGION: Mutex<Option<LastRegion>> = Mutex::new(None);

    /// Waits for a display's frozen frame (a quick drag can beat the capture), up to 5 s.
    fn frame_for(app: &AppHandle, display: u32) -> Result<Arc<Frame>, String> {
        let t = Instant::now();
        loop {
            {
                let session = SESSION.lock().unwrap();
                let s = session.as_ref().ok_or("no capture in progress")?;
                if let Some(f) = s.frames.iter().find(|f| f.display.id == display) {
                    return Ok(f.clone());
                }
            }
            if t.elapsed() > Duration::from_secs(5) {
                SESSION.lock().unwrap().take();
                hide_overlays(app);
                show_main(app);
                return Err(format!("display {display} wasn't captured"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Ends the session and puts the overlays away; returns the session's timings.
    fn end_session(app: &AppHandle) -> Timings {
        let timings = SESSION
            .lock()
            .unwrap()
            .take()
            .map(|s| s.timings)
            .unwrap_or_default();
        hide_overlays(app);
        timings
    }

    /// The picture is made: on the clipboard, capture window back, and tell it what happened.
    fn deliver(
        app: &AppHandle,
        result: Result<(Vec<u8>, usize, usize), String>,
        t: Instant,
        timings: Timings,
        what: &str,
        region: Option<LastRegion>,
        repeated: bool,
    ) -> Result<Captured, String> {
        let (png, width, height) = match result {
            Ok(r) => r,
            Err(e) => {
                log::event(&format!("{what} failed: {e}"));
                show_main(app);
                return Err(e);
            }
        };
        let bytes = png.len();
        let after = SETTINGS.lock().unwrap().after;
        let saved = if after.saves() {
            match crate::output::save(&png) {
                Ok(p) => Some(p),
                Err(e) => {
                    log::event(&format!("save failed: {e}"));
                    show_main(app);
                    return Err(e);
                }
            }
        } else {
            None
        };
        if after.copies() {
            let copy = png.clone();
            on_main(app, move |_| capture::copy_png(&copy))?;
        }
        let last = Last {
            png,
            copied: after.copies(),
            saved,
        };
        let captured = Captured {
            width,
            height,
            bytes,
            copy_ms: ms(t),
            timings,
            region,
            repeated,
            delivered: delivered(&last),
        };
        *LAST.lock().unwrap() = Some(last);
        if let Some(r) = region {
            *LAST_REGION.lock().unwrap() = Some(r);
        }
        log::event(&format!(
            "{what} {width}x{height} px ({bytes} bytes) in {} ms: copied {}, saved {}",
            captured.copy_ms,
            captured.delivered.copied,
            captured.delivered.saved.is_some()
        ));
        show_main(app);
        let _ = app.emit_to("main", "captured", captured.clone());
        Ok(captured)
    }

    /// A region was dragged: crop it from the frozen frame.
    pub fn finish(app: &AppHandle, display: u32, sel: Rect) -> Result<Captured, String> {
        let t = Instant::now();
        let frame = frame_for(app, display)?;
        let timings = end_session(app);
        deliver(
            app,
            capture::crop_png(&frame, sel),
            t,
            timings,
            "region",
            Some(LastRegion { display, rect: sel }),
            false,
        )
    }

    /// A display was clicked: the whole frozen frame.
    pub fn finish_display(app: &AppHandle, display: u32) -> Result<Captured, String> {
        let t = Instant::now();
        let frame = frame_for(app, display)?;
        let timings = end_session(app);
        let whole = Rect {
            x: 0.0,
            y: 0.0,
            w: frame.display.bounds.w,
            h: frame.display.bounds.h,
        };
        deliver(
            app,
            capture::crop_png(&frame, whole),
            t,
            timings,
            "display",
            None,
            false,
        )
    }

    /// A window was clicked: that window on its own, with or without its shadow (the capture window's setting).
    pub fn finish_window(app: &AppHandle, window_id: u32) -> Result<Captured, String> {
        let t = Instant::now();
        if SESSION.lock().unwrap().is_none() {
            return Err("no capture in progress".into());
        }
        let shadow = SETTINGS.lock().unwrap().shadow;
        let timings = end_session(app);
        let result = crate::windows::capture(window_id, shadow).and_then(|img| {
            let png = capture::encode_png(&img)?;
            let (w, h) = (
                objc2_core_graphics::CGImage::width(Some(&img)),
                objc2_core_graphics::CGImage::height(Some(&img)),
            );
            Ok((png, w, h))
        });
        deliver(app, result, t, timings, "window", None, false)
    }

    /// Repeat: the last dragged region again, straight to the clipboard, no overlay.
    pub fn repeat(app: &AppHandle) -> Result<Captured, String> {
        let last = (*LAST_REGION.lock().unwrap()).ok_or("No region yet: drag one first.")?;
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        {
            let mut session = SESSION.lock().unwrap();
            if session.is_some() {
                return Err("a capture is already in progress".into());
            }
            *session = Some(Session {
                id,
                pixels: Default::default(),
                frames: Vec::new(),
                timings: Timings::default(),
                hidden: None,
            });
        }
        let t = Instant::now();
        let result = hide_main_for_capture(app)
            .and_then(|_| capture::capture_all())
            .and_then(|frames| {
                let frame = frames
                    .iter()
                    .find(|f| f.display.id == last.display)
                    .ok_or("The display that region was on isn't attached now.")?;
                capture::crop_png(frame, last.rect)
            });
        SESSION.lock().unwrap().take();
        deliver(
            app,
            result,
            t,
            Timings::default(),
            "repeated region",
            Some(last),
            true,
        )
    }

    pub fn cancel(app: &AppHandle) {
        if SESSION.lock().unwrap().take().is_some() {
            log::event("capture cancelled");
        }
        hide_overlays(app);
        show_main(app);
        let _ = app.emit_to("main", "cancelled", ());
    }

    // ---- Commands -------------------------------------------------------------------------------------------

    #[tauri::command]
    async fn start_capture(app: AppHandle, delay: Option<u64>) -> Result<Timings, String> {
        tauri::async_runtime::spawn_blocking(move || begin(&app, delay.unwrap_or(0)))
            .await
            .map_err(|e| e.to_string())?
    }

    #[tauri::command]
    async fn finish_capture(app: AppHandle, display: u32, rect: Rect) -> Result<Captured, String> {
        tauri::async_runtime::spawn_blocking(move || finish(&app, display, rect))
            .await
            .map_err(|e| e.to_string())?
    }

    #[tauri::command]
    async fn finish_window_capture(app: AppHandle, window: u32) -> Result<Captured, String> {
        tauri::async_runtime::spawn_blocking(move || finish_window(&app, window))
            .await
            .map_err(|e| e.to_string())?
    }

    #[tauri::command]
    async fn finish_display_capture(app: AppHandle, display: u32) -> Result<Captured, String> {
        tauri::async_runtime::spawn_blocking(move || finish_display(&app, display))
            .await
            .map_err(|e| e.to_string())?
    }

    #[tauri::command]
    async fn repeat_region(app: AppHandle) -> Result<Captured, String> {
        tauri::async_runtime::spawn_blocking(move || repeat(&app))
            .await
            .map_err(|e| e.to_string())?
    }

    /// The capture window hands back the region remembered from the last launch.
    #[tauri::command]
    fn set_last_region(region: Option<LastRegion>) {
        let mut last = LAST_REGION.lock().unwrap();
        if last.is_none() {
            *last = region;
        }
    }

    /// Status-line actions on the last capture: copy it, save it, show it in Finder, open it (saving first).
    #[tauri::command]
    async fn last_capture(app: AppHandle, action: String) -> Result<Delivered, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let mut guard = LAST.lock().unwrap();
            let last = guard.as_mut().ok_or("nothing captured yet")?;
            match action.as_str() {
                "copy" => {
                    let png = last.png.clone();
                    on_main(&app, move |_| capture::copy_png(&png))?;
                    last.copied = true;
                }
                "save" | "open" | "reveal" => {
                    if last.saved.is_none() {
                        last.saved = Some(crate::output::save(&last.png)?);
                    }
                    let path = last.saved.clone().unwrap();
                    match action.as_str() {
                        "open" => crate::output::open(&path)?,
                        "reveal" => crate::output::reveal(&path)?,
                        _ => {}
                    }
                }
                other => return Err(format!("unknown action {other}")),
            }
            log::event(&format!("last capture: {action}"));
            Ok(delivered(last))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// The capture window's mode picker and shadow setting.
    #[tauri::command]
    fn set_mode(mode: Mode, shadow: bool, after: Option<crate::output::After>) {
        // End-to-end test runs fix the mode (scripts/e2e.sh); the capture window mustn't change it.
        let mode = std::env::var("MINGSHOT_E2E_MODE")
            .ok()
            .and_then(|m| serde_json::from_value(serde_json::Value::String(m)).ok())
            .unwrap_or(mode);
        let after = std::env::var("MINGSHOT_E2E_AFTER")
            .ok()
            .and_then(|a| serde_json::from_value(serde_json::Value::String(a)).ok())
            .or(after)
            .unwrap_or(crate::output::After::Copy);
        *SETTINGS.lock().unwrap() = Settings {
            mode,
            shadow,
            after,
        };
    }

    #[tauri::command]
    async fn cancel_capture(app: AppHandle) {
        let _ = tauri::async_runtime::spawn_blocking(move || cancel(&app)).await;
    }

    /// End-to-end test mode: MINGSHOT_E2E_SELECT="x,y,w,h" (points, relative to the display under the pointer) is
    /// selected automatically when the overlay comes up (scripts/e2e.swift).
    #[tauri::command]
    fn e2e_selection() -> Option<Rect> {
        let v: Vec<f64> = std::env::var("MINGSHOT_E2E_SELECT")
            .ok()?
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        (v.len() == 4).then(|| Rect {
            x: v[0],
            y: v[1],
            w: v[2],
            h: v[3],
        })
    }

    /// Side of the magnifier's patch, in device pixels (odd, so one pixel is the centre).
    const LOUPE: usize = 15;

    /// The magnifier: a LOUPE×LOUPE patch of the frozen frame (RGBA) centred on device pixel (x, y) of a display.
    /// Empty until that display's frame has arrived.
    #[tauri::command]
    fn loupe(display: u32, x: i64, y: i64) -> tauri::ipc::Response {
        let pixels = {
            let mut session = SESSION.lock().unwrap();
            let Some(s) = session.as_mut() else {
                return tauri::ipc::Response::new(Vec::new());
            };
            match s.pixels.get(&display) {
                Some(p) => p.clone(),
                None => {
                    let Some(frame) = s.frames.iter().find(|f| f.display.id == display) else {
                        return tauri::ipc::Response::new(Vec::new());
                    };
                    let Some(p) = frame.pixels() else {
                        return tauri::ipc::Response::new(Vec::new());
                    };
                    let p = Arc::new(p);
                    s.pixels.insert(display, p.clone());
                    p
                }
            }
        };
        tauri::ipc::Response::new(pixels.patch(x, y, LOUPE))
    }

    #[tauri::command]
    fn permission() -> bool {
        capture::has_permission()
    }

    /// Asks macOS for Screen Recording and opens its Settings pane, where the switch is.
    #[tauri::command]
    fn request_permission() -> bool {
        let granted = capture::request_permission();
        if !granted {
            let _ = std::process::Command::new("open")
                .arg(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
                )
                .status();
        }
        granted
    }

    // ---- Bench mode (MINGSHOT_BENCH=<runs>): docs/M0.md ------------------------------------------------------

    #[tauri::command]
    fn bench_runs() -> u32 {
        std::env::var("MINGSHOT_BENCH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    /// Bench mode paints the overlay opaque magenta (to prove captures never contain it) unless
    /// MINGSHOT_BENCH_PLAIN is set, which times the ordinary see-through overlay instead.
    #[tauri::command]
    fn bench_opaque() -> bool {
        bench_runs() > 0 && std::env::var("MINGSHOT_BENCH_PLAIN").is_err()
    }

    #[derive(Serialize)]
    struct BenchCheck {
        /// Magenta pixels where the (magenta) capture window was. Any at all means MingShot photographed it.
        window: usize,
        /// The largest share of any display that's magenta: the bench overlay is opaque magenta, so a frame that
        /// caught it would be ~1.0.
        overlay: f64,
    }

    #[tauri::command]
    fn bench_check() -> BenchCheck {
        let session = SESSION.lock().unwrap();
        let Some(s) = session.as_ref() else {
            return BenchCheck {
                window: 0,
                overlay: 0.0,
            };
        };
        let mut window = 0;
        let mut overlay: f64 = 0.0;
        for f in s.frames.iter() {
            let n = s.hidden.map_or(0, |area| capture::count_magenta(f, area));
            window += n;
            let whole =
                capture::count_magenta(f, f.display.bounds) as f64 / (f.px_w() * f.px_h()) as f64;
            overlay = overlay.max(whole);
            // MINGSHOT_BENCH_DUMP=<dir>: keep the frames that caught MingShot, to see where.
            if let (true, Ok(dir)) = (n > 0 || whole > 0.01, std::env::var("MINGSHOT_BENCH_DUMP"))
                && let Ok(png) = capture::encode_png(&f.image)
            {
                let _ = std::fs::write(format!("{dir}/leak-d{}-{n}.png", f.display.id), png);
            }
        }
        BenchCheck { window, overlay }
    }

    /// Positive control for the bench: a capture with the window left up must find it.
    #[tauri::command]
    async fn bench_control(app: AppHandle) -> Result<usize, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let a = app.clone();
            let area = on_main(&app, move |mtm| {
                let main = a.get_webview_window("main")?;
                ns_window(&main).map(|w| overlay::cg_frame(mtm, w))
            })?
            .ok_or("no capture window")?;
            capture::capture_all()
                .map(|frames| frames.iter().map(|f| capture::count_magenta(f, area)).sum())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    #[tauri::command]
    fn bench_done(app: AppHandle, report: String) {
        println!("{report}");
        log::event(&format!("bench: {}", report.replace('\n', " | ")));
        app.exit(0);
    }

    pub fn run() {
        use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
        let hotkey = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit2);
        // Repeat the last region (M1).
        let repeat_key = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit3);

        tauri::Builder::default()
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        if event.state() != ShortcutState::Pressed
                            || (*shortcut != hotkey && *shortcut != repeat_key)
                        {
                            return;
                        }
                        // One press can arrive twice (seen in the e2e runs): ignore a repeat within 300 ms.
                        static LAST: Mutex<Option<Instant>> = Mutex::new(None);
                        {
                            let mut last = LAST.lock().unwrap();
                            if last.is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
                                return;
                            }
                            *last = Some(Instant::now());
                        }
                        let app = app.clone();
                        let repeat_it = *shortcut == repeat_key;
                        std::thread::spawn(move || {
                            if repeat_it {
                                if let Err(e) = repeat(&app) {
                                    log::event(&format!("⌃⇧3: {e}"));
                                    let _ = app.emit_to("main", "capture-error", e);
                                }
                            } else if let Err(e) = begin(&app, 0) {
                                log::event(&format!("⌃⇧2: {e}"));
                            }
                        });
                    })
                    .build(),
            )
            .invoke_handler(tauri::generate_handler![
                start_capture,
                finish_capture,
                finish_window_capture,
                finish_display_capture,
                set_mode,
                repeat_region,
                last_capture,
                set_last_region,
                loupe,
                cancel_capture,
                e2e_selection,
                permission,
                request_permission,
                bench_runs,
                bench_opaque,
                bench_check,
                bench_control,
                bench_done
            ])
            .setup(move |app| {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                log::event("started");
                if let Err(e) = app.global_shortcut().register(hotkey) {
                    log::event(&format!("⌃⇧2 not registered: {e}"));
                }
                if let Err(e) = app.global_shortcut().register(repeat_key) {
                    log::event(&format!("⌃⇧3 not registered: {e}"));
                }
                ensure_overlays(app.handle());
                hide_overlays(app.handle()); // prepares them for the first New
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                }
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("error while running MingShot");
    }
}

#[cfg(target_os = "macos")]
pub fn run() {
    app::run();
}

#[cfg(not(target_os = "macos"))]
pub fn run() {
    eprintln!("MingShot M0 is Mac only; Windows is M6 (PLAN.md §6).");
}
