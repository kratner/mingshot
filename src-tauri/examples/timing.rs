//! Times captureImageInRect per display, alone and all together (docs/M0.md).
fn main() {
    use mingshot_lib::capture;
    use std::time::Instant;
    let ds = capture::displays();
    for _ in 0..2 {
        let _ = capture::capture_all();
    }
    for d in &ds {
        let mut v = Vec::new();
        for _ in 0..8 {
            let t = Instant::now();
            capture::capture_one(d).unwrap();
            v.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "display {} {}x{} px alone: median {:.1} ms (min {:.1}, max {:.1})",
            d.id, d.px_w, d.px_h, v[4], v[0], v[7]
        );
    }
    let mut v = Vec::new();
    for _ in 0..8 {
        let t = Instant::now();
        capture::capture_all().unwrap();
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "all {} together: median {:.1} ms (min {:.1}, max {:.1})",
        ds.len(),
        v[4],
        v[0],
        v[7]
    );
}
