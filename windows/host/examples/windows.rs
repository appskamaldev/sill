//! Development probe: the window list a device would get.
#[cfg(windows)]
fn main() {
    let t = std::time::Instant::now();
    let w = sill_catalog::windows_list::windows();
    println!("{} windows in {:.1} ms", w.len(), t.elapsed().as_secs_f64() * 1e3);
    for x in &w {
        println!(
            "  {:08x} {:>5}x{:<5} min={} {:30.30} aumid={:?} exe={:?}",
            x.id(),
            x.width,
            x.height,
            x.minimized,
            x.title,
            x.aumid,
            x.exe.as_deref().map(|e| e.rsplit(std::path::MAIN_SEPARATOR).next().unwrap_or(e))
        );
    }
}
#[cfg(not(windows))]
fn main() {}
