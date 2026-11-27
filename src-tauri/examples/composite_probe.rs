//! M2 check: compositing annotations over a capture must leave every pixel they don't cover exactly as captured.
//! Captures each display, encodes it as MingShot does, then composites (1) nothing and (2) a fully transparent
//! overlay the size of the image, and compares every pixel with the original. No window, nothing on screen.
fn main() {
    use mingshot_lib::{capture, editor};
    let frames = capture::capture_all().expect("capture");
    for f in &frames {
        let png = capture::encode_png(&f.image).unwrap();
        let original = decode_rgba(&png);
        let alone = editor::composite_png(&png, None).unwrap();
        // A transparent overlay: an empty 32-bit image of the same size, as the editor sends when nothing is drawn.
        let (w, h) = (original.1, original.2);
        let clear = {
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&vec![0u8; (w * h * 4) as usize]).unwrap();
            out
        };
        let over = editor::composite_png(&png, Some(&clear)).unwrap();
        for (name, result) in [("no overlay", alone), ("transparent overlay", over)] {
            let r = decode_rgba(&result);
            let diff = original.0.chunks(4).zip(r.0.chunks(4)).filter(|(a, b)| a[..3] != b[..3]).count();
            println!(
                "display {} ({}x{} px), {name}: {}",
                f.display.id,
                w,
                h,
                if diff == 0 { "EXACT".to_string() } else { format!("{diff} px differ") }
            );
        }
    }
}

fn decode_rgba(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => panic!("unexpected colour type"),
    };
    (rgba, info.width, info.height)
}
