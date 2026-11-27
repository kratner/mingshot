//! Prints the metadata MingShot would record for the frontmost windows (docs/M1.md): no capture, nothing on screen.
fn main() {
    use mingshot_lib::{metadata, windows};
    println!(
        "Accessibility allowed for this process: {}",
        metadata::ax_trusted()
    );
    let list = windows::on_screen();
    for w in list.iter().filter(|w| w.layer == 0).take(8) {
        let t = std::time::Instant::now();
        let s = metadata::source(w);
        let title: String = s.window_title.chars().take(40).collect();
        println!(
            "{:<16} {:<34} title {:<42} url {:?} file {:?} ({} ms)",
            s.app,
            s.bundle_id.clone().unwrap_or_default(),
            format!("{title:?}"),
            s.url,
            s.file_path,
            t.elapsed().as_millis()
        );
    }
    per_display();
}

/// Every window visible on each display, with its share, as a display capture would record it.
fn per_display() {
    use mingshot_lib::{capture, metadata, windows};
    let list = windows::on_screen();
    for d in capture::displays() {
        let seen = metadata::visible_windows(&list, d.bounds);
        let t = std::time::Instant::now();
        let entries = metadata::sources(&seen, (d.bounds.x, d.bounds.y));
        println!(
            "\ndisplay {}: {} windows visible (details looked up in {} ms)",
            d.id,
            entries.len(),
            t.elapsed().as_millis()
        );
        for e in entries.iter().take(6) {
            let title: String = e.source.window_title.chars().take(36).collect();
            println!(
                "  {:>5.1}%  {:<16} {:<38} {}",
                e.coverage * 100.0,
                e.source.app,
                format!("{title:?}"),
                e.source
                    .url
                    .clone()
                    .or(e.source.file_path.clone())
                    .unwrap_or_default()
            );
        }
    }
}
