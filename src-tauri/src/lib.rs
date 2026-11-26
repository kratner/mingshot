//! MingShot M0 (PLAN.md §6): a small capture window with a New button (§7.6); New puts a selection overlay over
//! every display at once, freezes each display under it as its frame arrives, and the region you drag goes on the
//! clipboard as a PNG (image only, §7.8). ⌃⇧2 does the same as New. Mac only for now. Measurements: docs/M0.md.

#[cfg(target_os = "macos")]
pub mod capture;
pub mod log;
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
        frames: Vec<Arc<Frame>>,
        timings: Timings,
        /// Where the capture window was before New hid it (global points), for bench mode's check.
        hidden: Option<Rect>,
    }

    static SESSION: Mutex<Option<Session>> = Mutex::new(None);
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
        if delay_s > 0 {
            std::thread::sleep(Duration::from_secs(delay_s));
            t0 = Instant::now();
        }

        // Each display freezes under the overlay as soon as its frame lands, so the one under the pointer never
        // waits for a slower display. All were read at the same moment.
        let t1 = Instant::now();
        // Ask for the frames first, then put the overlay up: a little quicker to freeze than the other way round
        // (docs/M0.md), and the overlay is left out of the frames either way.
        let (displays, rx) = capture::capture_each()?;
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
            overlay::show_ui(mtm, &ui, key);
            key
        })?;
        let ui_ms = ms(t0);
        if let (Some(rect), Some(display)) = (e2e_selection(), key) {
            let _ = app.emit_to(overlay_label(display).as_str(), "e2e-select", rect);
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
    }

    /// The selection is done: crop it from the frozen frame (waiting for it if the drag beat the capture), put it
    /// on the clipboard, put the overlays away.
    pub fn finish(app: &AppHandle, display: u32, sel: Rect) -> Result<Captured, String> {
        let t = Instant::now();
        let frame = loop {
            {
                let session = SESSION.lock().unwrap();
                let s = session.as_ref().ok_or("no capture in progress")?;
                if let Some(f) = s.frames.iter().find(|f| f.display.id == display) {
                    break f.clone();
                }
            }
            if t.elapsed() > Duration::from_secs(5) {
                SESSION.lock().unwrap().take();
                hide_overlays(app);
                show_main(app);
                return Err(format!("display {display} wasn't captured"));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let session = SESSION.lock().unwrap().take();
        let timings = session.map(|s| s.timings).unwrap_or_default();
        let result = capture::crop_png(&frame, sel);
        hide_overlays(app);
        let (png, width, height) = match result {
            Ok(r) => r,
            Err(e) => {
                show_main(app);
                return Err(e);
            }
        };
        let bytes = png.len();
        on_main(app, move |_| capture::copy_png(&png))?;
        let captured = Captured {
            width,
            height,
            bytes,
            copy_ms: ms(t),
            timings,
        };
        log::event(&format!(
            "copied {width}x{height} px ({bytes} bytes) in {} ms",
            captured.copy_ms
        ));
        show_main(app);
        let _ = app.emit_to("main", "captured", captured.clone());
        Ok(captured)
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

        tauri::Builder::default()
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, shortcut, event| {
                        if *shortcut == hotkey && event.state() == ShortcutState::Pressed {
                            let app = app.clone();
                            std::thread::spawn(move || {
                                if let Err(e) = begin(&app, 0) {
                                    log::event(&format!("⌃⇧2: {e}"));
                                }
                            });
                        }
                    })
                    .build(),
            )
            .invoke_handler(tauri::generate_handler![
                start_capture,
                finish_capture,
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
