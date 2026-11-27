//! Metadata recorded with every capture (M1, PLAN.md §2 "Metadata", §7.7): when, which display, where on it, and the
//! source window (app, bundle id, title), plus its web address or document path when MingShot may ask the app
//! through Accessibility. Saved beside a saved PNG as a `.mingshot` sidecar; the clipboard stays image only (§7.8).
//!
//! Web addresses and file paths need the Accessibility permission. MingShot never asks for it on its own: the
//! capture window offers it, and without it those two fields are simply empty.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::capture::Rect;
use crate::windows::WindowInfo;

/// Everything known about one capture.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Meta {
    /// Local time with its UTC offset, e.g. 2026-11-26T21:11:43-05:00.
    pub captured_at: String,
    /// "region", "window" or "display".
    pub kind: String,
    pub display: DisplayMeta,
    /// What was captured, in the display's points and pixels (for a window: its frame).
    pub area_points: Rect,
    pub area_pixels: Rect,
    /// The main source: the window that covers most of the capture (for a window capture, that window). None
    /// over the bare desktop.
    pub source: Option<Source>,
    /// Every window visible in the capture, most coverage first (Keith, 2026-11-26: "record every window in the
    /// selection"). A window hidden behind others inside the capture isn't listed.
    pub windows: Vec<WindowEntry>,
    pub app_version: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DisplayMeta {
    pub id: u32,
    pub name: Option<String>,
    pub scale: f64,
    pub width_pixels: usize,
    pub height_pixels: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Source {
    pub app: String,
    pub bundle_id: Option<String>,
    pub window_title: String,
    /// The page's address, when the window is a browser and Accessibility is allowed.
    pub url: Option<String>,
    /// The document's file, when the app says which and Accessibility is allowed.
    pub file_path: Option<String>,
    #[serde(skip)]
    pub pid: i32,
    #[serde(skip)]
    pub window_id: u32,
}

/// One window seen in a capture.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct WindowEntry {
    #[serde(flatten)]
    pub source: Source,
    /// Share of the capture where this window is what shows (0 to 1).
    pub coverage: f64,
    /// The window's frame, in points relative to the capture's display.
    pub frame_points: Rect,
}

/// The windows visible in `area` (global points), frontmost wins at each point, with the share of the area each
/// shows in, most first. Sampled on a grid of at most 96×96 points, so it's quick and exact enough for "which
/// windows, roughly how much of each".
pub fn visible_windows(windows: &[WindowInfo], area: Rect) -> Vec<(WindowInfo, f64)> {
    if area.w <= 0.0 || area.h <= 0.0 {
        return Vec::new();
    }
    let nx = ((area.w / 2.0).ceil() as usize).clamp(1, 96);
    let ny = ((area.h / 2.0).ceil() as usize).clamp(1, 96);
    let mut counts: Vec<usize> = vec![0; windows.len()];
    for j in 0..ny {
        let y = area.y + (j as f64 + 0.5) * area.h / ny as f64;
        for i in 0..nx {
            let x = area.x + (i as f64 + 0.5) * area.w / nx as f64;
            let front = windows.iter().position(|w| {
                x >= w.bounds.x
                    && x < w.bounds.x + w.bounds.w
                    && y >= w.bounds.y
                    && y < w.bounds.y + w.bounds.h
            });
            if let Some(k) = front {
                counts[k] += 1;
            }
        }
    }
    let total = (nx * ny) as f64;
    let mut out: Vec<(WindowInfo, f64)> = windows
        .iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(w, n)| (w.clone(), n as f64 / total))
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}

/// Looks up every window's details (in parallel: each app may take up to ⅓ s), keeping the order.
pub fn sources(windows: &[(WindowInfo, f64)], origin: (f64, f64)) -> Vec<WindowEntry> {
    let found: Vec<Source> = std::thread::scope(|scope| {
        let jobs: Vec<_> = windows
            .iter()
            .take(16)
            .map(|(w, _)| scope.spawn(move || source(w)))
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    found
        .into_iter()
        .zip(windows)
        .map(|(source, (w, coverage))| WindowEntry {
            source,
            coverage: (coverage * 1000.0).round() / 1000.0,
            frame_points: Rect {
                x: w.bounds.x - origin.0,
                y: w.bounds.y - origin.1,
                w: w.bounds.w,
                h: w.bounds.h,
            },
        })
        .collect()
}

/// The current local time as ISO 8601 with offset.
pub fn now_iso() -> String {
    use objc2_foundation::{NSDate, NSDateFormatter, NSLocale, NSString};
    let f = NSDateFormatter::new();
    f.setLocale(Some(&NSLocale::localeWithLocaleIdentifier(
        &NSString::from_str("en_US_POSIX"),
    )));
    f.setDateFormat(Some(&NSString::from_str("yyyy-MM-dd'T'HH:mm:ssXXXXX")));
    f.stringFromDate(&NSDate::date()).to_string()
}

/// The frontmost ordinary window (layer 0) containing `p` (global points), from a front-to-back list.
pub fn window_at(windows: &[WindowInfo], p: (f64, f64)) -> Option<&WindowInfo> {
    windows.iter().find(|w| {
        w.layer == 0
            && p.0 >= w.bounds.x
            && p.0 < w.bounds.x + w.bounds.w
            && p.1 >= w.bounds.y
            && p.1 < w.bounds.y + w.bounds.h
    })
}

/// A source from a window: app and title from the window list, bundle id from the running app, and (when allowed)
/// the address or file from Accessibility.
pub fn source(w: &WindowInfo) -> Source {
    let (url, file_path) = if ax_trusted() {
        ax_document(w)
    } else {
        (None, None)
    };
    Source {
        app: w.app.clone(),
        bundle_id: bundle_id(w.pid),
        window_title: w.title.clone(),
        url,
        file_path,
        pid: w.pid,
        window_id: w.id,
    }
}

fn bundle_id(pid: i32) -> Option<String> {
    use objc2_app_kit::NSRunningApplication;
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app.bundleIdentifier().map(|s| s.to_string())
}

/// What an `AXDocument` / `AXURL` value means: a web address, or a local file's path.
pub fn classify(value: &str) -> (Option<String>, Option<String>) {
    if value.starts_with("http://") || value.starts_with("https://") {
        (Some(value.to_string()), None)
    } else if let Some(rest) = value.strip_prefix("file://") {
        let path = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
        // Electron apps (Claude, Orca, VS Code…) report their own page inside the app bundle: not a document.
        if path.contains(".app/Contents/") || path.contains(".asar/") {
            return (None, None);
        }
        (None, Some(path))
    } else {
        (None, None)
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- Accessibility -------------------------------------------------------------------------------------------

/// Whether MingShot may read other apps' windows (Accessibility).
pub fn ax_trusted() -> bool {
    // SAFETY: documented call, no arguments.
    unsafe { objc2_application_services::AXIsProcessTrusted() }
}

/// Shows macOS's Accessibility prompt (it opens System Settings). Only from a click in the capture window.
pub fn ax_request() -> bool {
    use objc2_application_services::kAXTrustedCheckOptionPrompt;
    use objc2_core_foundation::{CFBoolean, CFDictionary};
    // SAFETY: the documented options dictionary: { kAXTrustedCheckOptionPrompt: true }.
    unsafe {
        let key: &objc2_core_foundation::CFString = kAXTrustedCheckOptionPrompt;
        let dict = CFDictionary::from_slices(&[key], &[CFBoolean::new(true)]);
        objc2_application_services::AXIsProcessTrustedWithOptions(Some(dict.as_opaque()))
    }
}

/// The window's document: its `AXDocument` (file or address) or, for a browser, the web area's `AXURL`. At most a
/// third of a second per app, so a hung app can't hold up a capture.
fn ax_document(w: &WindowInfo) -> (Option<String>, Option<String>) {
    use objc2_application_services::AXUIElement;
    let deadline = Instant::now() + Duration::from_millis(330);
    // SAFETY: documented calls on an element we own.
    let app = unsafe { AXUIElement::new_application(w.pid) };
    unsafe { app.set_messaging_timeout(0.25) };
    let Some(windows) = ax_children(&app, "AXWindows") else {
        return (None, None);
    };
    // The AX window that is this window: same title, else same position and size.
    let window = windows
        .iter()
        .find(|el| {
            ax_string(el, "AXTitle").as_deref() == Some(w.title.as_str()) && !w.title.is_empty()
        })
        .or_else(|| {
            windows
                .iter()
                .find(|el| ax_frame(el).is_some_and(|f| near(f, w.bounds)))
        })
        .or(windows.first());
    let Some(window) = window else {
        return (None, None);
    };
    if let Some(doc) = ax_string(window, "AXDocument") {
        let found = classify(&doc);
        if found != (None, None) {
            return found;
        }
    }
    // Browsers: the first web area under the window carries the page's address.
    let mut queue: std::collections::VecDeque<(
        objc2_core_foundation::CFRetained<AXUIElement>,
        usize,
    )> = std::collections::VecDeque::new();
    queue.push_back((window.clone(), 0));
    let mut seen = 0;
    while let Some((el, depth)) = queue.pop_front() {
        seen += 1;
        if seen > 3000 || Instant::now() > deadline {
            break;
        }
        if ax_string(&el, "AXRole").as_deref() == Some("AXWebArea")
            && let Some(url) = ax_url(&el, "AXURL")
        {
            return classify(&url);
        }
        if depth < 30 {
            for child in ax_children(&el, "AXChildren").unwrap_or_default() {
                queue.push_back((child, depth + 1));
            }
        }
    }
    (None, None)
}

fn near(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 2.0
        && (a.y - b.y).abs() < 2.0
        && (a.w - b.w).abs() < 2.0
        && (a.h - b.h).abs() < 2.0
}

fn ax_value(
    el: &objc2_application_services::AXUIElement,
    attr: &str,
) -> Option<objc2_core_foundation::CFRetained<objc2_core_foundation::CFType>> {
    use objc2_core_foundation::{CFRetained, CFString, CFType};
    let name = CFString::from_str(attr);
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: copy rule: on success `value` is a retained CF object we take over.
    let err = unsafe { el.copy_attribute_value(&name, std::ptr::NonNull::from(&mut value)) };
    if err.0 != 0 || value.is_null() {
        return None;
    }
    // SAFETY: non-null and owned (copy rule).
    Some(unsafe { CFRetained::from_raw(std::ptr::NonNull::new(value as *mut CFType)?) })
}

fn ax_string(el: &objc2_application_services::AXUIElement, attr: &str) -> Option<String> {
    let v = ax_value(el, attr)?;
    v.downcast_ref::<objc2_core_foundation::CFString>()
        .map(|s| s.to_string())
}

fn ax_url(el: &objc2_application_services::AXUIElement, attr: &str) -> Option<String> {
    let v = ax_value(el, attr)?;
    if let Some(s) = v.downcast_ref::<objc2_core_foundation::CFString>() {
        return Some(s.to_string());
    }
    let url = v.downcast_ref::<objc2_core_foundation::CFURL>()?;
    url.string().to_string().into()
}

fn ax_children(
    el: &objc2_application_services::AXUIElement,
    attr: &str,
) -> Option<Vec<objc2_core_foundation::CFRetained<objc2_application_services::AXUIElement>>> {
    use objc2_application_services::AXUIElement;
    use objc2_core_foundation::{CFArray, CFRetained};
    let v = ax_value(el, attr)?;
    let array = v.downcast_ref::<CFArray>()?;
    let mut out = Vec::new();
    for i in 0..array.count() {
        // SAFETY: AX arrays of elements hold AXUIElementRefs; retain each to keep it.
        let item = unsafe { array.value_at_index(i) } as *const AXUIElement;
        if let Some(p) = std::ptr::NonNull::new(item as *mut AXUIElement) {
            out.push(unsafe { CFRetained::retain(p) });
        }
    }
    Some(out)
}

fn ax_frame(el: &objc2_application_services::AXUIElement) -> Option<Rect> {
    use objc2_application_services::{AXValue, AXValueType};
    use objc2_core_foundation::{CGPoint, CGSize};
    let get = |attr: &str, kind: AXValueType, out: *mut std::ffi::c_void| -> bool {
        let Some(v) = ax_value(el, attr) else {
            return false;
        };
        let Some(v) = v.downcast_ref::<AXValue>() else {
            return false;
        };
        // SAFETY: `out` points at a value of the type `kind` names.
        unsafe { v.value(kind, std::ptr::NonNull::new(out).unwrap()) }
    };
    let mut p = CGPoint::new(0.0, 0.0);
    let mut s = CGSize::new(0.0, 0.0);
    let ok = get(
        "AXPosition",
        AXValueType::CGPoint,
        &mut p as *mut _ as *mut _,
    ) && get("AXSize", AXValueType::CGSize, &mut s as *mut _ as *mut _);
    ok.then_some(Rect {
        x: p.x,
        y: p.y,
        w: s.width,
        h: s.height,
    })
}

// ---- Sidecar ---------------------------------------------------------------------------------------------------

/// The `.mingshot` file beside a saved PNG (PLAN.md §2 "Library"): the image's name, its metadata, the editable
/// annotations (M2), and, once there are annotations, the untouched original (base64 PNG), so the PNG beside it can
/// show the markup in any app while MingShot can always reopen the capture clean underneath.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub format: String,
    pub version: u32,
    pub image: String,
    pub metadata: Option<Meta>,
    #[serde(default)]
    pub annotations: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_png: Option<String>,
}

/// Writes `<name>.mingshot` beside `png_path`; returns its path.
pub fn write_sidecar(
    png_path: &Path,
    meta: Option<&Meta>,
    annotations: &serde_json::Value,
    original: Option<&[u8]>,
) -> Result<std::path::PathBuf, String> {
    use base64::Engine;
    let path = png_path.with_extension("mingshot");
    let doc = Document {
        format: "mingshot".into(),
        version: 1,
        image: png_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        metadata: meta.cloned(),
        annotations: if annotations.is_null() {
            serde_json::json!([])
        } else {
            annotations.clone()
        },
        original_png: original.map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
    };
    let json = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(path)
}

/// A saved capture to edit again: the clean original (from the sidecar if it has one, else the PNG beside it),
/// the annotations and the metadata. `path` may be the `.mingshot` or its `.png`.
pub struct Opened {
    pub png_path: std::path::PathBuf,
    pub original: Vec<u8>,
    pub annotations: serde_json::Value,
    pub meta: Option<Meta>,
}

pub fn read_document(path: &Path) -> Result<Opened, String> {
    use base64::Engine;
    let sidecar = path.with_extension("mingshot");
    let png_path = path.with_extension("png");
    let doc: Option<Document> = std::fs::read_to_string(&sidecar)
        .ok()
        .and_then(|j| serde_json::from_str(&j).ok());
    let original = match doc.as_ref().and_then(|d| d.original_png.as_ref()) {
        Some(b64) => base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| e.to_string())?,
        None => std::fs::read(&png_path)
            .map_err(|e| format!("couldn't read {}: {e}", png_path.display()))?,
    };
    Ok(Opened {
        png_path,
        original,
        annotations: doc
            .as_ref()
            .map(|d| d.annotations.clone())
            .unwrap_or_else(|| serde_json::json!([])),
        meta: doc.and_then(|d| d.metadata),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, layer: i32, x: f64, y: f64, w: f64, h: f64) -> WindowInfo {
        WindowInfo {
            id,
            bounds: Rect { x, y, w, h },
            pid: 1,
            app: format!("App{id}"),
            title: String::new(),
            layer,
        }
    }

    #[test]
    fn picks_the_frontmost_ordinary_window_under_a_point() {
        let list = vec![
            win(1, 101, 0.0, 0.0, 500.0, 500.0), // a menu: skipped
            win(2, 0, 100.0, 100.0, 100.0, 100.0),
            win(3, 0, 0.0, 0.0, 400.0, 400.0),
        ];
        assert_eq!(window_at(&list, (150.0, 150.0)).map(|w| w.id), Some(2));
        assert_eq!(window_at(&list, (300.0, 300.0)).map(|w| w.id), Some(3));
        assert_eq!(window_at(&list, (450.0, 450.0)).map(|w| w.id), None);
    }

    #[test]
    fn every_visible_window_in_the_area_with_its_share() {
        // Two windows side by side, a third entirely behind the first (front to back), a fourth outside.
        let list = vec![
            win(1, 0, 0.0, 0.0, 100.0, 100.0),
            win(2, 0, 100.0, 0.0, 100.0, 100.0),
            win(3, 0, 10.0, 10.0, 50.0, 50.0),
            win(4, 0, 500.0, 500.0, 50.0, 50.0),
        ];
        let seen = visible_windows(
            &list,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 100.0,
            },
        );
        let ids: Vec<u32> = seen.iter().map(|(w, _)| w.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1) && ids.contains(&2), "{ids:?}");
        for (_, share) in &seen {
            assert!((share - 0.5).abs() < 0.02, "{share}");
        }
        // Partly covered: window 2 behind window 1 shows only where 1 isn't.
        let list = vec![
            win(1, 0, 0.0, 0.0, 150.0, 100.0),
            win(2, 0, 0.0, 0.0, 200.0, 100.0),
        ];
        let seen = visible_windows(
            &list,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 100.0,
            },
        );
        assert_eq!(seen[0].0.id, 1);
        assert!(
            (seen[0].1 - 0.75).abs() < 0.02 && (seen[1].1 - 0.25).abs() < 0.02,
            "{seen:?}"
        );
        // Bare desktop: nothing.
        assert!(
            visible_windows(
                &list,
                Rect {
                    x: 300.0,
                    y: 300.0,
                    w: 10.0,
                    h: 10.0
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn documents_are_addresses_or_paths() {
        assert_eq!(
            classify("https://example.com/a?b=1"),
            (Some("https://example.com/a?b=1".into()), None)
        );
        assert_eq!(
            classify("file:///Users/k/My%20Notes.txt"),
            (None, Some("/Users/k/My Notes.txt".into()))
        );
        assert_eq!(
            classify("file://localhost/tmp/x.png"),
            (None, Some("/tmp/x.png".into()))
        );
        assert_eq!(classify("about:blank"), (None, None));
        // An Electron app's own page is not a document.
        assert_eq!(
            classify(
                "file:///Applications/Claude.app/Contents/Resources/app.asar/.vite/renderer/main_window/index.html"
            ),
            (None, None)
        );
    }

    #[test]
    fn time_is_iso_with_offset() {
        let t = now_iso();
        assert_eq!(t.len(), "2026-11-26T21:11:43-05:00".len(), "{t}");
        assert_eq!(&t[10..11], "T", "{t}");
        assert!(
            t.ends_with('Z') || t[19..].starts_with('+') || t[19..].starts_with('-'),
            "{t}"
        );
    }

    #[test]
    fn sidecar_sits_beside_the_png() {
        let dir = std::env::temp_dir().join(format!("mingshot-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("MingShot 2026-11-26 at 21.11.43.png");
        let meta = Meta {
            captured_at: "2026-11-26T21:11:43-05:00".into(),
            kind: "region".into(),
            display: DisplayMeta {
                id: 3,
                name: None,
                scale: 2.0,
                width_pixels: 3840,
                height_pixels: 2160,
            },
            area_points: Rect {
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            },
            area_pixels: Rect {
                x: 2.0,
                y: 4.0,
                w: 6.0,
                h: 8.0,
            },
            source: None,
            windows: Vec::new(),
            app_version: "0.0.1".into(),
        };
        let path = write_sidecar(&png, Some(&meta), &serde_json::json!([]), None).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "MingShot 2026-11-26 at 21.11.43.mingshot"
        );
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["format"], "mingshot");
        assert_eq!(v["image"], "MingShot 2026-11-26 at 21.11.43.png");
        assert_eq!(v["metadata"]["kind"], "region");
        assert_eq!(v["metadata"]["display"]["scale"], 2.0);
        assert!(v.get("original_png").is_none());
        // With annotations and the original kept: reading it back gives the clean original and the shapes.
        std::fs::write(&png, b"flattened").unwrap();
        let shapes = serde_json::json!([{"id": "s1", "kind": "arrow"}]);
        write_sidecar(&png, Some(&meta), &shapes, Some(b"original")).unwrap();
        let back = read_document(&png).unwrap();
        assert_eq!(back.original, b"original");
        assert_eq!(back.annotations, shapes);
        assert_eq!(back.meta.unwrap().kind, "region");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
