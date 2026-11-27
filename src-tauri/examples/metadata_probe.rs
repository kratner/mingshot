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
}
