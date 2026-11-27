//! The editor's output (M2, PLAN.md §4): annotations are drawn by the editor's canvas into a transparent overlay at
//! the capture's full pixel size, and composited here, with CoreGraphics, over the original image in the original's
//! own colour space. Drawing the screenshot itself through the web view would colour-manage every pixel (the
//! canvas is sRGB, screens are often Display P3); this way every pixel no annotation covers comes out exactly as
//! captured (checked by `cargo run --example composite_probe`).

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{CFData, CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage};
use objc2_image_io::CGImageSource;

unsafe extern "C" {
    // Missing from objc2-core-graphics' generated bindings; the documented CoreGraphics function.
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *const CGColorSpace,
        bitmap_info: u32,
    ) -> *mut CGContext;
}

/// kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little: BGRA, the layout screen captures come in.
const BGRA_PREMULTIPLIED: u32 = 2 | (2 << 12);

/// A PNG (or any ImageIO format) decoded to a CGImage, colour profile kept.
pub fn decode(bytes: &[u8]) -> Result<CFRetained<CGImage>, String> {
    let data = CFData::from_bytes(bytes);
    // SAFETY: documented ImageIO calls; no options.
    unsafe {
        let source = CGImageSource::with_data(&data, None).ok_or("not an image")?;
        source
            .image_at_index(0, None)
            .ok_or_else(|| "couldn't read the image".to_string())
    }
}

/// The original with the overlay drawn over it, at the original's size and in its colour space. Where the overlay
/// is transparent, the original's pixels are untouched.
pub fn composite(
    original: &CGImage,
    overlay: Option<&CGImage>,
) -> Result<CFRetained<CGImage>, String> {
    let (w, h) = (
        CGImage::width(Some(original)),
        CGImage::height(Some(original)),
    );
    let space = CGImage::color_space(Some(original)).ok_or("the image has no colour space")?;
    // SAFETY: a context we own (create rule), drawn into and read on this thread only.
    let ctx = unsafe {
        let raw = CGBitmapContextCreate(
            std::ptr::null_mut(),
            w,
            h,
            8,
            0,
            CFRetained::as_ptr(&space).as_ptr(),
            BGRA_PREMULTIPLIED,
        );
        CFRetained::from_raw(NonNull::new(raw).ok_or("couldn't make a drawing context")?)
    };
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
    CGContext::draw_image(Some(&ctx), rect, Some(original));
    if let Some(over) = overlay {
        if (CGImage::width(Some(over)), CGImage::height(Some(over))) != (w, h) {
            return Err(format!(
                "the drawing is {}x{} px, the image {w}x{h}",
                CGImage::width(Some(over)),
                CGImage::height(Some(over))
            ));
        }
        CGContext::draw_image(Some(&ctx), rect, Some(over));
    }
    CGBitmapContextCreateImage(Some(&ctx)).ok_or_else(|| "couldn't finish the image".to_string())
}

/// The editor's canvas draws in sRGB but its PNGs carry no profile, which CoreGraphics would take as the screen's own
/// colours (more saturated on a P3 display). Tag it sRGB so annotations look exactly as they did in the editor.
fn as_srgb(image: CFRetained<CGImage>) -> CFRetained<CGImage> {
    use objc2_core_graphics::kCGColorSpaceSRGB;
    // SAFETY: a static CoreGraphics name constant.
    let Some(srgb) = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })) else {
        return image;
    };
    CGImage::new_copy_with_color_space(Some(&image), Some(&srgb)).unwrap_or(image)
}

/// Original PNG + overlay PNG → finished PNG.
pub fn composite_png(original_png: &[u8], overlay_png: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let original = decode(original_png)?;
    let overlay = overlay_png.map(decode).transpose()?.map(as_srgb);
    let done = composite(&original, overlay.as_deref())?;
    crate::capture::encode_png(&done)
}
