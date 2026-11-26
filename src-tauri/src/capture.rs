//! The capture core on the Mac (PLAN.md §4). Each display is taken whole with ScreenCaptureKit's
//! `captureImageInRect` (macOS 15.2+), the same composited picture the Mac's own full-screen screenshot reads, so
//! window edges and glass effects come out exactly as on screen. (ScreenCaptureKit's content-filter captures leave
//! those edges out: see docs/M0.md.) A region is cropped from that frozen frame at full pixel resolution and encoded
//! as PNG by ImageIO, so the bytes on the clipboard are the display's pixels, untouched.
//!
//! `captureImageInRect` can't leave MingShot's own windows out, so the caller hides them first
//! (`overlay::hide_and_wait`).

use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2_core_foundation::{CFMutableData, CFRetained, CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGDisplayBounds, CGDisplayCopyDisplayMode, CGDisplayMode, CGGetActiveDisplayList, CGImage,
};
use objc2_foundation::{NSData, NSError};
use objc2_image_io::CGImageDestination;
use objc2_screen_capture_kit::SCScreenshotManager;

/// A rectangle in points. Display bounds are in the global top-left-origin space CoreGraphics uses; a selection
/// is relative to its display's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One attached display: its id, where it sits in points, and its size in pixels.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Display {
    pub id: u32,
    pub bounds: Rect,
    pub px_w: usize,
    pub px_h: usize,
}

impl Display {
    /// Pixels per point (1 on a standard display, 2 on Retina). Taken from the current display mode, never
    /// assumed, because a "more space" mode on a Retina panel can be 1.
    pub fn scale(&self) -> f64 {
        self.px_w as f64 / self.bounds.w
    }
}

/// The displays attached right now, main display first.
pub fn displays() -> Vec<Display> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: the buffer holds 16 ids and CoreGraphics writes at most that many.
    let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if err.0 != 0 {
        return Vec::new();
    }
    ids[..count as usize]
        .iter()
        .map(|&id| {
            let b = CGDisplayBounds(id);
            let mode = CGDisplayCopyDisplayMode(id);
            Display {
                id,
                bounds: Rect {
                    x: b.origin.x,
                    y: b.origin.y,
                    w: b.size.width,
                    h: b.size.height,
                },
                px_w: CGDisplayMode::pixel_width(mode.as_deref()),
                px_h: CGDisplayMode::pixel_height(mode.as_deref()),
            }
        })
        .collect()
}

pub fn has_permission() -> bool {
    crate::compat::has_permission()
}

/// Shows the system's Screen Recording prompt (only the first time macOS has heard of the app) and returns
/// whether access is granted now.
pub fn request_permission() -> bool {
    crate::compat::request_permission()
}

/// A CGImage of one display, frozen at the moment of capture. CGImages are immutable, so sharing one across
/// threads is safe.
pub struct Frame {
    pub display: Display,
    pub image: CFRetained<CGImage>,
}
unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

impl Frame {
    pub fn px_w(&self) -> usize {
        CGImage::width(Some(&self.image))
    }
    pub fn px_h(&self) -> usize {
        CGImage::height(Some(&self.image))
    }
}

fn error_text(error: *mut NSError, fallback: &str) -> String {
    match NonNull::new(error) {
        // SAFETY: a live NSError passed to a completion handler.
        Some(e) => unsafe { e.as_ref() }.localizedDescription().to_string(),
        None => fallback.to_string(),
    }
}

/// Frames as they arrive from `capture_each`.
pub type Frames = mpsc::Receiver<Result<Frame, String>>;

/// Starts capturing every display at once and returns them with a channel that delivers each frame as it arrives
/// (in whatever order the displays finish), so a caller can show each display's overlay without waiting for the
/// slowest one. Whatever is on screen is in the frames, MingShot's windows included, so hide them first.
pub fn capture_each() -> Result<(Vec<Display>, Frames), String> {
    let displays = displays();
    if displays.is_empty() {
        return Err("no displays".into());
    }
    let (tx, rx) = mpsc::channel::<Result<Frame, String>>();
    if !crate::compat::modern_displays() {
        // Before macOS 15.2: CoreGraphics, one thread per display so they're read together (compat.rs).
        for d in &displays {
            let (tx, display) = (tx.clone(), *d);
            std::thread::spawn(move || {
                let result = crate::compat::display_image(display.id)
                    .map(|image| Frame { display, image })
                    .and_then(check_size);
                let _ = tx.send(result);
            });
        }
        return Ok((displays, rx));
    }
    for d in &displays {
        let tx = tx.clone();
        let display = *d;
        let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            let result = match NonNull::new(image) {
                // SAFETY: the handler's CGImage is only borrowed; retain it to keep it.
                Some(img) => Ok(Frame {
                    display,
                    image: unsafe { CFRetained::retain(img) },
                }),
                None => Err(error_text(error, "capture failed")),
            };
            let result = result.and_then(check_size);
            let _ = tx.send(result);
        });
        let b = d.bounds;
        let rect = CGRect::new(CGPoint::new(b.x, b.y), CGSize::new(b.w, b.h));
        // SAFETY: documented API (macOS 15.2+); the block is called once.
        unsafe { SCScreenshotManager::captureImageInRect_completionHandler(rect, Some(&block)) };
    }
    Ok((displays, rx))
}

/// A frame must come back at the display's full pixel size.
fn check_size(f: Frame) -> Result<Frame, String> {
    if (f.px_w(), f.px_h()) == (f.display.px_w, f.display.px_h) {
        Ok(f)
    } else {
        Err(format!(
            "display {} came back {}x{} px, expected {}x{}",
            f.display.id,
            f.px_w(),
            f.px_h(),
            f.display.px_w,
            f.display.px_h
        ))
    }
}

/// Captures every display, all at once, and waits for all of them. Frames come back in display order.
pub fn capture_all() -> Result<Vec<Frame>, String> {
    let (displays, rx) = capture_each()?;
    let mut frames = Vec::with_capacity(displays.len());
    for _ in 0..displays.len() {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(f)) => frames.push(f),
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("a display didn't return a frame in 5 s".into()),
        }
    }
    frames.sort_by_key(|f| displays.iter().position(|d| d.id == f.display.id));
    Ok(frames)
}

/// Captures one display on its own (timing experiments: examples/timing.rs).
pub fn capture_one(d: &Display) -> Result<Frame, String> {
    let (tx, rx) = mpsc::channel::<Result<Frame, String>>();
    let display = *d;
    let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = match NonNull::new(image) {
            // SAFETY: the handler's CGImage is only borrowed; retain it to keep it.
            Some(img) => Ok(Frame {
                display,
                image: unsafe { CFRetained::retain(img) },
            }),
            None => Err(error_text(error, "capture failed")),
        };
        let _ = tx.send(result);
    });
    let b = d.bounds;
    let rect = CGRect::new(CGPoint::new(b.x, b.y), CGSize::new(b.w, b.h));
    // SAFETY: documented API (macOS 15.2+); the block is called once.
    unsafe { SCScreenshotManager::captureImageInRect_completionHandler(rect, Some(&block)) };
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "no frame in 5 s".to_string())?
}

/// Converts a selection in points (relative to its display's top-left corner) to whole pixels of the frame,
/// clamped to the frame. Points are scaled by the frame's own pixels-per-point, so a 2× display gets twice the
/// pixels and a 1× display exactly the points.
pub fn to_pixels(
    sel: Rect,
    display_points: Rect,
    px_w: usize,
    px_h: usize,
) -> (usize, usize, usize, usize) {
    let sx = px_w as f64 / display_points.w;
    let sy = px_h as f64 / display_points.h;
    let x0 = (sel.x * sx).round().clamp(0.0, px_w as f64) as usize;
    let y0 = (sel.y * sy).round().clamp(0.0, px_h as f64) as usize;
    let x1 = ((sel.x + sel.w) * sx).round().clamp(0.0, px_w as f64) as usize;
    let y1 = ((sel.y + sel.h) * sy).round().clamp(0.0, px_h as f64) as usize;
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

/// The region of `frame` as PNG bytes, the frame's pixels unchanged and its colour profile attached.
pub fn crop_png(frame: &Frame, sel: Rect) -> Result<(Vec<u8>, usize, usize), String> {
    let (x, y, w, h) = to_pixels(sel, frame.display.bounds, frame.px_w(), frame.px_h());
    if w == 0 || h == 0 {
        return Err("empty selection".into());
    }
    let rect = CGRect::new(
        CGPoint::new(x as f64, y as f64),
        CGSize::new(w as f64, h as f64),
    );
    let cropped = CGImage::with_image_in_rect(Some(&frame.image), rect).ok_or("crop failed")?;
    Ok((encode_png(&cropped)?, w, h))
}

pub fn encode_png(image: &CGImage) -> Result<Vec<u8>, String> {
    let data = CFMutableData::new(None, 0).ok_or("no buffer")?;
    let png = CFString::from_static_str("public.png");
    // SAFETY: documented ImageIO calls; no options dictionaries.
    unsafe {
        let dest = CGImageDestination::with_data(&data, &png, 1, None).ok_or("no PNG encoder")?;
        dest.add_image(image, None);
        if !dest.finalize() {
            return Err("PNG encode failed".into());
        }
    }
    let bytes: &objc2_core_foundation::CFData = &data;
    let len = objc2_core_foundation::CFData::length(bytes) as usize;
    let ptr = objc2_core_foundation::CFData::byte_ptr(bytes);
    // SAFETY: CFData's bytes are valid for `len` bytes while `data` lives.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}

/// Pixels of a frame, inside `area` (global points), that are strongly magenta (red and blue high, green low).
/// Bench mode paints the capture window magenta and counts these where the window was, to prove it never ends up
/// in a frame.
pub fn count_magenta(frame: &Frame, area: Rect) -> usize {
    let b = frame.display.bounds;
    let local = Rect {
        x: area.x - b.x,
        y: area.y - b.y,
        w: area.w,
        h: area.h,
    };
    let (x0, y0, w, h) = to_pixels(local, b, frame.px_w(), frame.px_h());
    if w == 0 || h == 0 {
        return 0;
    }
    use objc2_core_graphics::CGDataProvider;
    let image = &frame.image;
    let Some(provider) = CGImage::data_provider(Some(image)) else {
        return 0;
    };
    let Some(data) = CGDataProvider::data(Some(&provider)) else {
        return 0;
    };
    let bytes: &objc2_core_foundation::CFData = &data;
    let len = objc2_core_foundation::CFData::length(bytes) as usize;
    // SAFETY: CFData's bytes are valid for `len` bytes while `data` lives.
    let buf =
        unsafe { std::slice::from_raw_parts(objc2_core_foundation::CFData::byte_ptr(bytes), len) };
    let row = CGImage::bytes_per_row(Some(image));
    let mut n = 0;
    for y in y0..y0 + h {
        for px in buf[y * row + x0 * 4..y * row + (x0 + w) * 4]
            .as_chunks::<4>()
            .0
        {
            // ScreenCaptureKit's SDR frames are BGRA.
            let (b, g, r) = (px[0], px[1], px[2]);
            if r > 200 && b > 200 && g < 90 {
                n += 1;
            }
        }
    }
    n
}

/// A frame's pixels as bytes (BGRA, rows `row` bytes apart), copied once so the magnifier can read small patches
/// quickly.
pub struct Pixels {
    pub bytes: Vec<u8>,
    pub row: usize,
    pub w: usize,
    pub h: usize,
}

impl Frame {
    pub fn pixels(&self) -> Option<Pixels> {
        use objc2_core_graphics::CGDataProvider;
        let provider = CGImage::data_provider(Some(&self.image))?;
        let data = CGDataProvider::data(Some(&provider))?;
        let cf: &objc2_core_foundation::CFData = &data;
        let len = objc2_core_foundation::CFData::length(cf) as usize;
        // SAFETY: CFData's bytes are valid for `len` bytes while `data` lives; copied out here.
        let bytes = unsafe { std::slice::from_raw_parts(objc2_core_foundation::CFData::byte_ptr(cf), len) }.to_vec();
        Some(Pixels { bytes, row: CGImage::bytes_per_row(Some(&self.image)), w: self.px_w(), h: self.px_h() })
    }
}

impl Pixels {
    /// An n×n patch (RGBA, row by row) centred on pixel (cx, cy); outside the frame is transparent.
    pub fn patch(&self, cx: i64, cy: i64, n: usize) -> Vec<u8> {
        let mut out = vec![0u8; n * n * 4];
        let half = (n / 2) as i64;
        for j in 0..n {
            let y = cy - half + j as i64;
            if y < 0 || y >= self.h as i64 {
                continue;
            }
            for i in 0..n {
                let x = cx - half + i as i64;
                if x < 0 || x >= self.w as i64 {
                    continue;
                }
                let src = y as usize * self.row + x as usize * 4;
                let dst = (j * n + i) * 4;
                // BGRA → RGBA, opaque.
                out[dst] = self.bytes[src + 2];
                out[dst + 1] = self.bytes[src + 1];
                out[dst + 2] = self.bytes[src];
                out[dst + 3] = 255;
            }
        }
        out
    }
}

/// Puts PNG bytes on the clipboard as the only item (image only: PLAN.md §7.8).
pub fn copy_png(png: &[u8]) {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let data = NSData::with_bytes(png);
    // SAFETY: NSPasteboardTypePNG is a static NSString constant.
    pb.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG });
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_X: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    #[test]
    fn patch_centres_and_pads() {
        // A 3×2 frame, BGRA, pixel (x, y) has blue = x, green = y.
        let mut bytes = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                bytes.extend_from_slice(&[x, y, 9, 255]);
            }
        }
        let p = Pixels { bytes, row: 12, w: 3, h: 2 };
        let patch = p.patch(0, 0, 3); // centred on the top-left corner
        let px = |i: usize, j: usize| &patch[(j * 3 + i) * 4..(j * 3 + i) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 0, 0, 0]); // outside: transparent
        assert_eq!(px(1, 1), &[9, 0, 0, 255]); // (0,0) as RGBA
        assert_eq!(px(2, 2), &[9, 1, 1, 255]); // (1,1)
    }

    #[test]
    fn one_x_is_points() {
        let sel = Rect {
            x: 100.0,
            y: 50.0,
            w: 300.0,
            h: 200.0,
        };
        assert_eq!(to_pixels(sel, ONE_X, 1920, 1080), (100, 50, 300, 200));
    }

    #[test]
    fn two_x_doubles() {
        let sel = Rect {
            x: 100.0,
            y: 50.0,
            w: 300.0,
            h: 200.0,
        };
        assert_eq!(to_pixels(sel, ONE_X, 3840, 2160), (200, 100, 600, 400));
    }

    #[test]
    fn half_points_land_on_pixels_at_2x() {
        let sel = Rect {
            x: 10.5,
            y: 0.5,
            w: 1.0,
            h: 1.5,
        };
        assert_eq!(to_pixels(sel, ONE_X, 3840, 2160), (21, 1, 2, 3));
    }

    #[test]
    fn clamped_to_the_frame() {
        let sel = Rect {
            x: 1900.0,
            y: 1000.0,
            w: 100.0,
            h: 100.0,
        };
        assert_eq!(to_pixels(sel, ONE_X, 1920, 1080), (1900, 1000, 20, 80));
        let neg = Rect {
            x: -10.0,
            y: -10.0,
            w: 20.0,
            h: 20.0,
        };
        assert_eq!(to_pixels(neg, ONE_X, 1920, 1080), (0, 0, 10, 10));
    }
}
