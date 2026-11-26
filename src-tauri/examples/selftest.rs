//! M0 self-test (PLAN.md §6, docs/M0.md). On every attached display:
//!
//! 1. the whole frame MingShot captures is compared pixel for pixel with the Mac's own full-display screenshot
//!    (`screencapture -D`, the ⌘⇧3 path): what is actually on screen;
//! 2. regions cropped by MingShot (ordinary, odd coordinates, far corner) are compared with the same regions cut
//!    from that screenshot, and their size with `screencapture -R` (the ⌘⇧4 path), whose pixels are reported too;
//! 3. capturing all displays at once is timed.
//!
//!   npm run selftest          (from the repo root; needs Screen Recording for the terminal running it)
//!
//! Screens move (clocks, cursors, pulsing status dots), so MingShot's capture is bracketed by two screenshots, one
//! just before and one just after: every pixel that held still between them must be identical in MingShot's frame,
//! and pixels that changed in between are reported as moving, not compared. Each display is tried up to five times and the best kept. The PNGs are left
//! in selftest-out/ (git-ignored: they're real screen contents).

#[cfg(target_os = "macos")]
fn main() {
    use mingshot_lib::capture::{self, Rect};
    use std::time::Instant;

    if !capture::has_permission() {
        eprintln!(
            "Screen Recording isn't allowed for this process. Allow it in System Settings ▸ Privacy & Security."
        );
        std::process::exit(2);
    }
    let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../selftest-out");
    std::fs::create_dir_all(&out).unwrap();
    let displays = capture::displays();
    let mut failed = false;

    println!("Displays:");
    for d in &displays {
        println!(
            "  {}  {}x{} pt at ({}, {})  {}x{} px  scale {}",
            d.id,
            d.bounds.w,
            d.bounds.h,
            d.bounds.x,
            d.bounds.y,
            d.px_w,
            d.px_h,
            d.scale()
        );
    }

    let regions = |d: &capture::Display| {
        vec![
            Rect {
                x: 120.0,
                y: 160.0,
                w: 300.0,
                h: 200.0,
            },
            Rect {
                x: 333.0,
                y: 241.0,
                w: 157.0,
                h: 89.0,
            },
            Rect {
                x: d.bounds.w - 260.0,
                y: d.bounds.h - 180.0,
                w: 260.0,
                h: 180.0,
            },
        ]
    };

    println!("\nWhole display vs screencapture -D, then regions:");
    for (index, d) in displays.iter().enumerate() {
        type Best = (usize, String, Vec<(usize, String)>);
        let mut best: Option<Best> = None;
        for _attempt in 0..5 {
            let theirs_path = out.join(format!("d{}-full-screencapture.png", d.id));
            let after_path = out.join(format!("d{}-full-screencapture-after.png", d.id));
            let dnum = (index + 1).to_string();
            if !screencapture(&["-D", &dnum], &theirs_path) {
                best = Some((usize::MAX, "screencapture failed".into(), vec![]));
                break;
            }
            let frames = capture::capture_all();
            if !screencapture(&["-D", &dnum], &after_path) {
                best = Some((usize::MAX, "screencapture failed".into(), vec![]));
                break;
            }
            let frames = match frames {
                Ok(f) => f,
                Err(e) => {
                    best = Some((usize::MAX, e, vec![]));
                    break;
                }
            };
            let frame = frames
                .iter()
                .find(|f| f.display.id == d.id)
                .expect("frame for display");
            let ours_png = capture::encode_png(&frame.image).unwrap();
            std::fs::write(out.join(format!("d{}-full-mingshot.png", d.id)), &ours_png).unwrap();
            let theirs = decode(&std::fs::read(&theirs_path).unwrap());
            let after = decode(&std::fs::read(&after_path).unwrap());
            let ours = decode(&ours_png);
            let (diff, note) = compare_either(&theirs, &after, &ours);

            let mut region_results = Vec::new();
            for (n, sel) in regions(d).into_iter().enumerate() {
                let (png, w, h) = capture::crop_png(frame, sel).expect("crop");
                std::fs::write(out.join(format!("d{}-r{}-mingshot.png", d.id, n)), &png).unwrap();
                let (x, y, _, _) = capture::to_pixels(sel, d.bounds, d.px_w, d.px_h);
                let cut = crop(&theirs, x, y, w, h);
                let cut_after = crop(&after, x, y, w, h);
                region_results.push(compare_either(&cut, &cut_after, &decode(&png)));
            }
            let total_diff = diff + region_results.iter().map(|r| r.0).sum::<usize>();
            if best.as_ref().is_none_or(|b| total_diff < b.0) {
                best = Some((total_diff, note, region_results));
            }
            if total_diff == 0 {
                break;
            }
        }
        let (diff, note, region_results) = best.unwrap();
        if diff != 0 {
            failed = true;
        }
        println!(
            "  display {}: whole frame {} {note}",
            d.id,
            verdict(diff == 0)
        );
        for (n, (sel, (rd, rnote))) in regions(d).into_iter().zip(region_results).enumerate() {
            // The ⌘⇧4 path, for information: its size must match; its pixels are reported, not required.
            let r_path = out.join(format!("d{}-r{}-screencapture-R.png", d.id, n));
            let arg = format!(
                "{},{},{},{}",
                d.bounds.x + sel.x,
                d.bounds.y + sel.y,
                sel.w,
                sel.h
            );
            let r_note = if screencapture(&["-R", &arg], &r_path) {
                let ours = decode(
                    &std::fs::read(out.join(format!("d{}-r{}-mingshot.png", d.id, n))).unwrap(),
                );
                let r = decode(&std::fs::read(&r_path).unwrap());
                if (r.1, r.2) != (ours.1, ours.2) {
                    failed = true;
                    format!("-R SIZE DIFFERS ({}x{} vs {}x{})", r.1, r.2, ours.1, ours.2)
                } else {
                    let (rdiff, rnote) = compare(&r, &ours);
                    if rdiff == 0 {
                        "-R identical".into()
                    } else {
                        format!("-R: {rdiff} px differ {rnote}")
                    }
                }
            } else {
                "-R failed".into()
            };
            if rd != 0 {
                failed = true;
            }
            println!(
                "    region {n} ({}x{} pt at {},{}): {} {rnote}; {r_note}",
                sel.w,
                sel.h,
                sel.x,
                sel.y,
                verdict(rd == 0)
            );
        }
    }

    println!("\nTiming, all {} displays at once:", displays.len());
    let mut times = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let frames = capture::capture_all().unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(frames.len(), displays.len());
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  capture_all over 10 runs: min {:.1} ms, median {:.1} ms, max {:.1} ms",
        times[0], times[5], times[9]
    );

    if failed {
        println!("\nFAILED. The PNGs are in {}", out.display());
        std::process::exit(1);
    }
    println!("\nPASSED: every display and region pixel-exact.");
}

#[cfg(target_os = "macos")]
fn verdict(ok: bool) -> &'static str {
    if ok { "EXACT" } else { "DIFFERENT" }
}

#[cfg(target_os = "macos")]
fn screencapture(args: &[&str], path: &std::path::Path) -> bool {
    std::process::Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .args(args)
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

type Rgba = (Vec<u8>, u32, u32);

/// Decodes a PNG to 8-bit RGBA, plus its width and height.
#[cfg(target_os = "macos")]
fn decode(bytes: &[u8]) -> Rgba {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => unreachable!("EXPAND removes palettes"),
    };
    (rgba, info.width, info.height)
}

#[cfg(target_os = "macos")]
fn crop(img: &Rgba, x: usize, y: usize, w: usize, h: usize) -> Rgba {
    let stride = img.1 as usize * 4;
    let mut out = Vec::with_capacity(w * h * 4);
    for row in y..y + h {
        out.extend_from_slice(&img.0[row * stride + x * 4..row * stride + (x + w) * 4]);
    }
    (out, w as u32, h as u32)
}

/// (pixels that differ, note). Alpha is ignored: screenshots are opaque.
#[cfg(target_os = "macos")]
fn compare(a: &Rgba, b: &Rgba) -> (usize, String) {
    if (a.1, a.2) != (b.1, b.2) {
        return (
            usize::MAX / 8,
            format!("(size {}x{} vs {}x{})", a.1, a.2, b.1, b.2),
        );
    }
    let mut diff = 0;
    let mut worst = 0u8;
    for (p, q) in a.0.chunks(4).zip(b.0.chunks(4)) {
        let d = (0..3).map(|i| p[i].abs_diff(q[i])).max().unwrap();
        if d > 0 {
            diff += 1;
            worst = worst.max(d);
        }
    }
    let note = if diff > 0 {
        format!(
            "({diff} of {} px, largest channel difference {worst})",
            a.1 * a.2
        )
    } else {
        format!("({}x{} px)", a.1, a.2)
    };
    (diff, note)
}

/// Like `compare`, for a frame taken between two screenshots: every pixel that held still between `before` and
/// `after` must be identical in `ours`; pixels that changed in between (animation) are counted, not compared.
#[cfg(target_os = "macos")]
fn compare_either(before: &Rgba, after: &Rgba, ours: &Rgba) -> (usize, String) {
    if (before.1, before.2) != (ours.1, ours.2) || (after.1, after.2) != (ours.1, ours.2) {
        return compare(before, ours);
    }
    let same = |p: &[u8], q: &[u8]| p[..3] == q[..3];
    let mut diff = 0;
    let mut moving = 0;
    let mut worst = 0u8;
    for ((b, a), o) in before
        .0
        .chunks(4)
        .zip(after.0.chunks(4))
        .zip(ours.0.chunks(4))
    {
        if !same(b, a) {
            // Animating between the two screenshots: MingShot may have caught any frame of it.
            moving += 1;
            continue;
        }
        if !same(b, o) {
            diff += 1;
            worst = worst.max(
                (0..3)
                    .map(|i| b[i].abs_diff(o[i]).min(a[i].abs_diff(o[i])))
                    .max()
                    .unwrap(),
            );
        }
    }
    let moving_note = if moving > 0 {
        format!(", {moving} px moved on screen meanwhile")
    } else {
        String::new()
    };
    let note = if diff > 0 {
        format!(
            "({diff} of {} px, largest channel difference {worst}{moving_note})",
            ours.1 * ours.2
        )
    } else {
        format!("({}x{} px{moving_note})", ours.1, ours.2)
    };
    (diff, note)
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The M0 self-test is Mac only for now.");
}
