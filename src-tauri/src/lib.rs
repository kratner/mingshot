//! MingShot M0 (PLAN.md §6): a small capture window with a New button (§7.6); New puts a selection overlay over
//! every display at once, freezes each display under it as its frame arrives, and the region you drag goes on the
//! clipboard as a PNG (image only, §7.8). ⌃⇧2 does the same as New. Mac only for now. Measurements: docs/M0.md.

#[cfg(target_os = "macos")]
pub mod capture;
#[cfg(target_os = "macos")]
pub mod compat;
#[cfg(target_os = "macos")]
pub mod editor;
pub mod log;
#[cfg(target_os = "macos")]
pub mod metadata;
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
        /// When New (or ⌃⇧2) was pressed, for the metadata.
        started_at: String,
        /// The windows on screen at that moment, front to back (metadata's source window; window mode's targets).
        windows: Vec<crate::windows::WindowInfo>,
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

    /// What's known about a capture when it's delivered; the source window's details are added afterwards.
    struct MetaCtx {
        captured_at: String,
        kind: &'static str,
        display: capture::Display,
        /// In the display's points (relative to its top-left corner) and pixels.
        area_points: Rect,
        area_pixels: Rect,
        /// The windows visible in the capture with their share of it, most first.
        windows: Vec<(crate::windows::WindowInfo, f64)>,
    }

    impl MetaCtx {
        fn finish(self) -> crate::metadata::Meta {
            let d = self.display;
            crate::metadata::Meta {
                captured_at: self.captured_at,
                kind: self.kind.to_string(),
                display: crate::metadata::DisplayMeta {
                    id: d.id,
                    name: None,
                    scale: d.scale(),
                    width_pixels: d.px_w,
                    height_pixels: d.px_h,
                },
                area_points: self.area_points,
                area_pixels: self.area_pixels,
                source: None,
                windows: crate::metadata::sources(&self.windows, (d.bounds.x, d.bounds.y)),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }
            .with_main_source()
        }
    }

    /// The window the capture is captioned "from": the one covering most of it.
    trait MainSource {
        fn with_main_source(self) -> Self;
    }
    impl MainSource for crate::metadata::Meta {
        fn with_main_source(mut self) -> Self {
            self.source = self.windows.first().map(|w| w.source.clone());
            self
        }
    }

    /// The capture's area in global points.
    fn global(d: &capture::Display, r: Rect) -> Rect {
        Rect {
            x: d.bounds.x + r.x,
            y: d.bounds.y + r.y,
            w: r.w,
            h: r.h,
        }
    }

    /// The session's start time and window list (for metadata), read before the session ends.
    fn session_context() -> (String, Vec<crate::windows::WindowInfo>) {
        SESSION
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| (s.started_at.clone(), s.windows.clone()))
            .unwrap_or_else(|| (crate::metadata::now_iso(), crate::windows::on_screen()))
    }

    fn pixel_rect(d: &capture::Display, r: Rect) -> Rect {
        let (x, y, w, h) = capture::to_pixels(r, d.bounds, d.px_w, d.px_h);
        Rect {
            x: x as f64,
            y: y as f64,
            w: w as f64,
            h: h as f64,
        }
    }

    /// Counts captures, so a slow metadata lookup can't attach itself to a newer capture.
    static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

    /// The last capture's PNG and where it went, for the status line's Copy / Save / Show in Finder / Open.
    struct Last {
        seq: u64,
        png: Vec<u8>,
        copied: bool,
        saved: Option<std::path::PathBuf>,
        meta: Option<crate::metadata::Meta>,
        /// Drawings made on it in the editor, kept when the editor closes so Edit brings them back.
        annotations: serde_json::Value,
    }

    static LAST: Mutex<Option<Last>> = Mutex::new(None);

    /// Where the last capture is now (sent back after each action).
    #[derive(Serialize, Clone)]
    pub struct Delivered {
        copied: bool,
        /// The saved file's full path, if it's been saved.
        saved: Option<String>,
        /// Opened in the editor (After capture → Edit, or Open).
        editing: bool,
    }

    fn delivered(l: &Last) -> Delivered {
        Delivered {
            copied: l.copied,
            saved: l.saved.as_ref().map(|p| p.display().to_string()),
            editing: false,
        }
    }

    // ---- The editor (M2) --------------------------------------------------------------------------------------

    /// A capture open in an editor window: the clean original, the annotations so far, and where it's saved.
    struct EditDoc {
        original: Vec<u8>,
        width: usize,
        height: usize,
        /// Pixels per point of the capture (stroke widths are chosen in points).
        scale: f64,
        meta: Option<crate::metadata::Meta>,
        saved: Option<std::path::PathBuf>,
        annotations: serde_json::Value,
        title: String,
        /// The capture it came from, so its metadata can arrive after the window opens.
        seq: Option<u64>,
    }

    static EDITS: std::sync::LazyLock<Mutex<std::collections::HashMap<u64, EditDoc>>> =
        std::sync::LazyLock::new(Default::default);
    static NEXT_EDIT: AtomicU64 = AtomicU64::new(1);

    /// Whether the capture window has been shown for the first time (by main_ready or the fallback).
    static MAIN_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// The capture window's page has drawn: show it the first time.
    #[tauri::command]
    fn main_ready(app: AppHandle) {
        if !MAIN_SHOWN.swap(true, Ordering::SeqCst)
            && let Some(main) = app.get_webview_window("main")
        {
            log::event("capture window shown (page drawn)");
            let _ = main.show();
            let _ = main.set_focus();
        }
    }

    /// Opens an editor window on a capture; returns its id.
    fn open_editor(app: &AppHandle, doc: EditDoc) -> Result<u64, String> {
        // One editor per capture: Edit while it's open brings that window forward instead of opening another
        // (Keith, 2026-11-28: each Edit opened a new window).
        let open = EDITS.lock().unwrap().iter().find_map(|(id, d)| {
            let same = (doc.seq.is_some() && d.seq == doc.seq) || (doc.saved.is_some() && d.saved == doc.saved);
            same.then_some(*id)
        });
        if let Some(id) = open
            && let Some(w) = app.get_webview_window(&format!("editor-{id}"))
        {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            log::event(&format!("editor {id} already open: focused"));
            return Ok(id);
        }
        let id = NEXT_EDIT.fetch_add(1, Ordering::SeqCst);
        // The image at its size in points, plus the toolbar and status bar, within reason.
        let (pw, ph) = (doc.width as f64 / doc.scale, doc.height as f64 / doc.scale);
        let w = (pw + 40.0).clamp(820.0, 1280.0);
        let h = (ph + 40.0 + 56.0 + 30.0).clamp(420.0, 880.0);
        let title = doc.title.clone();
        EDITS.lock().unwrap().insert(id, doc);
        let label = format!("editor-{id}");
        let window = WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App(format!("index.html?editor={id}").into()),
        )
        .title(title)
        .inner_size(w, h)
        .min_inner_size(700.0, 400.0)
        .center()
        .build()
        .map_err(|e| {
            EDITS.lock().unwrap().remove(&id);
            e.to_string()
        })?;
        let app_for_close = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Destroyed = event {
                // The editor's drawings (synced as they change) and save location go back to the capture they came
                // from, so Edit reopens them (Keith, 2026-11-27: closing lost them). EDITS is released before LAST
                // is taken: the Edit action takes them the other way round.
                let doc = EDITS.lock().unwrap().remove(&id);
                if let Some(doc) = doc
                    && let Some(seq) = doc.seq
                {
                    let saved = doc.saved.as_ref().map(|p| p.display().to_string());
                    let count = doc.annotations.as_array().map_or(0, |a| a.len());
                    if let Some(last) = LAST.lock().unwrap().as_mut().filter(|l| l.seq == seq) {
                        last.annotations = doc.annotations;
                        if doc.saved.is_some() {
                            last.saved = doc.saved;
                        }
                    }
                    // The capture window stops saying "Opened in the editor" (Keith, 2026-11-27).
                    let _ = app_for_close.emit_to(
                        "main",
                        "editor-closed",
                        serde_json::json!({ "seq": seq, "saved": saved, "annotations": count }),
                    );
                }
            }
        });
        let _ = window.set_focus();
        log::event(&format!("editor {id} opened"));
        Ok(id)
    }

    fn edit_doc_for(
        png: Vec<u8>,
        meta: Option<crate::metadata::Meta>,
        saved: Option<std::path::PathBuf>,
        annotations: serde_json::Value,
        seq: Option<u64>,
    ) -> Result<EditDoc, String> {
        let image = crate::editor::decode(&png)?;
        let (width, height) = (
            objc2_core_graphics::CGImage::width(Some(&image)),
            objc2_core_graphics::CGImage::height(Some(&image)),
        );
        let scale = meta.as_ref().map_or(2.0, |m| m.display.scale).max(1.0);
        let title = match &saved {
            Some(p) => p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => format!("MingShot · {width} × {height}"),
        };
        Ok(EditDoc {
            original: png,
            width,
            height,
            scale,
            meta,
            saved,
            annotations,
            title,
            seq,
        })
    }

    #[derive(Serialize)]
    struct EditorInfo {
        width: usize,
        height: usize,
        scale: f64,
        title: String,
        annotations: serde_json::Value,
        saved: Option<String>,
    }

    #[tauri::command]
    fn editor_doc(id: u64) -> Result<EditorInfo, String> {
        let edits = EDITS.lock().unwrap();
        let d = edits.get(&id).ok_or("that editor has closed")?;
        Ok(EditorInfo {
            width: d.width,
            height: d.height,
            scale: d.scale,
            title: d.title.clone(),
            annotations: d.annotations.clone(),
            saved: d.saved.as_ref().map(|p| p.display().to_string()),
        })
    }

    /// The editor's background: the original capture, as PNG bytes.
    #[tauri::command]
    fn editor_image(id: u64) -> tauri::ipc::Response {
        let png = EDITS
            .lock()
            .unwrap()
            .get(&id)
            .map(|d| d.original.clone())
            .unwrap_or_default();
        tauri::ipc::Response::new(png)
    }

    /// Copy and/or save the edited capture: the editor's overlay (a transparent PNG at full size, base64) composited
    /// over the original (editor.rs), annotations kept in the sidecar with the clean original.
    #[tauri::command]
    async fn editor_export(
        app: AppHandle,
        id: u64,
        overlay: Option<String>,
        annotations: serde_json::Value,
        action: String,
    ) -> Result<Delivered, String> {
        tauri::async_runtime::spawn_blocking(move || {
            use base64::Engine;
            let overlay = overlay
                .map(|b| {
                    base64::engine::general_purpose::STANDARD
                        .decode(b.trim_start_matches("data:image/png;base64,"))
                })
                .transpose()
                .map_err(|e| e.to_string())?;
            let (original, meta, saved) = {
                let edits = EDITS.lock().unwrap();
                let d = edits.get(&id).ok_or("that editor has closed")?;
                (d.original.clone(), d.meta.clone(), d.saved.clone())
            };
            let png = crate::editor::composite_png(&original, overlay.as_deref())?;
            let (copy, save) = (
                action == "copy" || action == "both",
                action == "save" || action == "both",
            );
            if copy {
                let p = png.clone();
                on_main(&app, move |_| capture::copy_png(&p))?;
            }
            let mut path = saved;
            if save {
                let target = match &path {
                    Some(p) => {
                        std::fs::write(p, &png)
                            .map_err(|e| format!("couldn't save {}: {e}", p.display()))?;
                        p.clone()
                    }
                    None => crate::output::save(&png)?,
                };
                let has_shapes = annotations.as_array().is_some_and(|a| !a.is_empty());
                crate::metadata::write_sidecar(
                    &target,
                    meta.as_ref(),
                    &annotations,
                    has_shapes.then_some(original.as_slice()),
                )?;
                path = Some(target);
            }
            if let Some(d) = EDITS.lock().unwrap().get_mut(&id) {
                d.annotations = annotations;
                if save {
                    d.saved = path.clone();
                }
            }
            log::event(&format!("editor {id}: {action}"));
            Ok(Delivered {
                copied: copy,
                saved: path.map(|p| p.display().to_string()),
                editing: true,
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// The editor's drawings as they change, so nothing is lost when the window closes.
    #[tauri::command]
    fn editor_sync(id: u64, annotations: serde_json::Value) {
        if let Some(d) = EDITS.lock().unwrap().get_mut(&id) {
            d.annotations = annotations;
        }
    }

    /// Where captures are saved (~/Pictures/MingShot): where Open… starts.
    #[tauri::command]
    fn captures_folder() -> Result<String, String> {
        crate::output::folder().map(|p| p.display().to_string())
    }

    /// Shows a saved capture in Finder (from the editor's status line).
    #[tauri::command]
    fn reveal_file(path: String) -> Result<(), String> {
        crate::output::reveal(std::path::Path::new(&path))
    }

    /// Reopens a saved capture (its .png or .mingshot) in the editor, annotations editable.
    #[tauri::command]
    async fn open_document(app: AppHandle, path: String) -> Result<u64, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let opened = crate::metadata::read_document(std::path::Path::new(&path))?;
            let doc = edit_doc_for(
                opened.original,
                opened.meta,
                Some(opened.png_path),
                opened.annotations,
                None,
            )?;
            open_editor(&app, doc)
        })
        .await
        .map_err(|e| e.to_string())?
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
    fn send_targets(app: &AppHandle, id: u64, displays: &[capture::Display]) {
        let mode = SETTINGS.lock().unwrap().mode;
        // Read once: the window list as it is now, for window mode's highlight and every capture's metadata.
        let snapshot = crate::windows::on_screen();
        if let Some(s) = SESSION.lock().unwrap().as_mut().filter(|s| s.id == id) {
            s.windows = snapshot.clone();
        }
        let all = if mode == Mode::Window {
            snapshot
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
                started_at: crate::metadata::now_iso(),
                windows: Vec::new(),
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
        send_targets(app, id, &displays);
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
                .filter_map(|w| {
                    w.label()
                        .trim_start_matches("overlay-")
                        .parse()
                        .ok()
                        .map(|d| (d, w))
                })
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
        /// Which capture this is (an editor closing reports it).
        seq: u64,
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
    // Every caller passes all of these and they read clearly at the call sites.
    #[allow(clippy::too_many_arguments)]
    fn deliver(
        app: &AppHandle,
        result: Result<(Vec<u8>, usize, usize), String>,
        t: Instant,
        timings: Timings,
        what: &str,
        region: Option<LastRegion>,
        repeated: bool,
        ctx: MetaCtx,
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
        let seq = CAPTURE_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        let saved_path = saved.clone();
        let editing = after == crate::output::After::Edit;
        let editor = if editing {
            let doc = edit_doc_for(png.clone(), None, None, serde_json::json!([]), Some(seq))?;
            Some(open_editor(app, doc)?)
        } else {
            None
        };
        let last = Last {
            seq,
            png,
            copied: after.copies(),
            saved,
            meta: None,
            annotations: serde_json::json!([]),
        };
        let captured = Captured {
            width,
            height,
            bytes,
            copy_ms: ms(t),
            timings,
            region,
            repeated,
            delivered: Delivered {
                editing,
                ..delivered(&last)
            },
            seq,
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
        // The editor comes last, so it's in front of the capture window (on Mojave it opened behind it: Keith,
        // 2026-11-26).
        if let Some(id) = editor
            && let Some(w) = app.get_webview_window(&format!("editor-{id}"))
        {
            let _ = w.show();
            let _ = w.set_focus();
        }
        let _ = app.emit_to("main", "captured", captured.clone());
        // The source window's details can take a moment (Accessibility), so they follow the picture.
        let app = app.clone();
        std::thread::spawn(move || {
            let meta = ctx.finish();
            if let Some(path) = &saved_path
                && let Err(e) =
                    crate::metadata::write_sidecar(path, Some(&meta), &serde_json::json!([]), None)
            {
                log::event(&format!("sidecar: {e}"));
            }
            if let Some(l) = LAST.lock().unwrap().as_mut().filter(|l| l.seq == seq) {
                l.meta = Some(meta.clone());
            }
            for d in EDITS
                .lock()
                .unwrap()
                .values_mut()
                .filter(|d| d.seq == Some(seq))
            {
                d.meta = Some(meta.clone());
            }
            let _ = app.emit_to("main", "metadata", meta);
        });
        Ok(captured)
    }

    /// A region was dragged: crop it from the frozen frame.
    pub fn finish(app: &AppHandle, display: u32, sel: Rect) -> Result<Captured, String> {
        let t = Instant::now();
        let frame = frame_for(app, display)?;
        let (captured_at, windows) = session_context();
        let timings = end_session(app);
        let d = frame.display;
        let ctx = MetaCtx {
            captured_at,
            kind: "region",
            display: d,
            area_points: sel,
            area_pixels: pixel_rect(&d, sel),
            windows: crate::metadata::visible_windows(&windows, global(&d, sel)),
        };
        deliver(
            app,
            capture::crop_png(&frame, sel),
            t,
            timings,
            "region",
            Some(LastRegion { display, rect: sel }),
            false,
            ctx,
        )
    }

    /// A display was clicked: the whole frozen frame.
    pub fn finish_display(app: &AppHandle, display: u32) -> Result<Captured, String> {
        let t = Instant::now();
        let frame = frame_for(app, display)?;
        let (captured_at, windows) = session_context();
        let timings = end_session(app);
        let d = frame.display;
        let whole = Rect {
            x: 0.0,
            y: 0.0,
            w: d.bounds.w,
            h: d.bounds.h,
        };
        // Every window visible on the display.
        let ctx = MetaCtx {
            captured_at,
            kind: "display",
            display: d,
            area_points: whole,
            area_pixels: pixel_rect(&d, whole),
            windows: crate::metadata::visible_windows(&windows, d.bounds),
        };
        deliver(
            app,
            capture::crop_png(&frame, whole),
            t,
            timings,
            "display",
            None,
            false,
            ctx,
        )
    }

    /// A window was clicked: that window on its own, with or without its shadow (the capture window's setting).
    pub fn finish_window(app: &AppHandle, window_id: u32) -> Result<Captured, String> {
        let t = Instant::now();
        if SESSION.lock().unwrap().is_none() {
            return Err("no capture in progress".into());
        }
        let shadow = SETTINGS.lock().unwrap().shadow;
        let (captured_at, windows) = session_context();
        let timings = end_session(app);
        let window = windows.iter().find(|w| w.id == window_id).cloned();
        // The display the window is mostly on (its centre's), and the window's frame on it.
        let displays = capture::displays();
        let display = window
            .as_ref()
            .and_then(|w| {
                let c = (w.bounds.x + w.bounds.w / 2.0, w.bounds.y + w.bounds.h / 2.0);
                displays.iter().find(|d| {
                    let b = d.bounds;
                    c.0 >= b.x && c.0 < b.x + b.w && c.1 >= b.y && c.1 < b.y + b.h
                })
            })
            .or(displays.first())
            .copied()
            .ok_or("no displays")?;
        let frame_on_display = window.as_ref().map_or(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            |w| Rect {
                x: w.bounds.x - display.bounds.x,
                y: w.bounds.y - display.bounds.y,
                w: w.bounds.w,
                h: w.bounds.h,
            },
        );
        let result = crate::windows::capture(window_id, shadow).and_then(|img| {
            let png = capture::encode_png(&img)?;
            let (w, h) = (
                objc2_core_graphics::CGImage::width(Some(&img)),
                objc2_core_graphics::CGImage::height(Some(&img)),
            );
            Ok((png, w, h))
        });
        let area_pixels = match &result {
            Ok((_, w, h)) => Rect {
                x: 0.0,
                y: 0.0,
                w: *w as f64,
                h: *h as f64,
            },
            Err(_) => pixel_rect(&display, frame_on_display),
        };
        let ctx = MetaCtx {
            captured_at,
            kind: "window",
            display,
            area_points: frame_on_display,
            area_pixels,
            // The window captured is the whole picture.
            windows: window.map(|w| vec![(w, 1.0)]).unwrap_or_default(),
        };
        deliver(app, result, t, timings, "window", None, false, ctx)
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
                started_at: crate::metadata::now_iso(),
                windows: Vec::new(),
            });
        }
        let t = Instant::now();
        let captured_at = crate::metadata::now_iso();
        let windows = crate::windows::on_screen();
        let mut display = None;
        let result = hide_main_for_capture(app)
            .and_then(|_| capture::capture_all())
            .and_then(|frames| {
                let frame = frames
                    .iter()
                    .find(|f| f.display.id == last.display)
                    .ok_or("The display that region was on isn't attached now.")?;
                display = Some(frame.display);
                capture::crop_png(frame, last.rect)
            });
        SESSION.lock().unwrap().take();
        let d = display
            .or_else(|| capture::displays().first().copied())
            .ok_or("no displays")?;
        let r = last.rect;
        let ctx = MetaCtx {
            captured_at,
            kind: "region",
            display: d,
            area_points: r,
            area_pixels: pixel_rect(&d, r),
            windows: crate::metadata::visible_windows(&windows, global(&d, r)),
        };
        deliver(
            app,
            result,
            t,
            Timings::default(),
            "repeated region",
            Some(last),
            true,
            ctx,
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

    /// Accessibility, for web addresses and file paths in the metadata: allowed? (`request`: show macOS's prompt.)
    #[tauri::command]
    fn accessibility(request: bool) -> bool {
        if request {
            crate::metadata::ax_request()
        } else {
            crate::metadata::ax_trusted()
        }
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
                "edit" => {
                    let doc = edit_doc_for(
                        last.png.clone(),
                        last.meta.clone(),
                        last.saved.clone(),
                        last.annotations.clone(),
                        Some(last.seq),
                    )?;
                    open_editor(&app, doc)?;
                }
                "save" | "open" | "reveal" => {
                    if last.saved.is_none() {
                        let path = crate::output::save(&last.png)?;
                        if let Some(meta) = &last.meta
                            && let Err(e) = crate::metadata::write_sidecar(
                                &path,
                                Some(meta),
                                &serde_json::json!([]),
                                None,
                            )
                        {
                            log::event(&format!("sidecar: {e}"));
                        }
                        last.saved = Some(path);
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

    /// A test run (e2e or bench): the capture window doesn't save settings or the last region, so tests never leave
    /// anything behind in the real app's storage.
    #[tauri::command]
    fn test_run() -> bool {
        [
            "MINGSHOT_BENCH",
            "MINGSHOT_E2E_MODE",
            "MINGSHOT_E2E_SELECT",
            "MINGSHOT_E2E_AFTER",
        ]
        .iter()
        .any(|k| std::env::var(k).is_ok())
    }

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
        // If MingShot ever goes down (on an older macOS, a missing method is the usual cause), the reason is the last
        // line of ~/Library/Logs/MingShot/mingshot.log.
        std::panic::set_hook(Box::new(|info| {
            log::event(&format!("crash: {info}"));
        }));
        let hotkey = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit2);
        // Repeat the last region (M1).
        let repeat_key = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit3);

        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
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
                main_ready,
                start_capture,
                finish_capture,
                finish_window_capture,
                finish_display_capture,
                set_mode,
                repeat_region,
                last_capture,
                editor_doc,
                editor_image,
                editor_export,
                editor_sync,
                captures_folder,
                reveal_file,
                open_document,
                accessibility,
                set_last_region,
                loupe,
                cancel_capture,
                e2e_selection,
                permission,
                request_permission,
                bench_runs,
                test_run,
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
                // The capture window shows once its page has drawn (main_ready), so the first launch doesn't flash a
                // blank white window while WebKit warms up (Keith, 2026-11-28, Mojave). If the page never says so (a
                // script failure), it shows after 3 s anyway.
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if !MAIN_SHOWN.swap(true, Ordering::SeqCst) {
                        log::event("capture window shown by the 3 s fallback");
                        if let Some(main) = handle.get_webview_window("main") {
                            let _ = main.show();
                        }
                    }
                });
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
