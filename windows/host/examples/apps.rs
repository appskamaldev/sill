//! Development probe: the Apps drawer's list and icon sizes on this machine.
#[cfg(windows)]
fn main() {
    let t = std::time::Instant::now();
    let apps = sill_catalog::scan().unwrap();
    println!("{} apps in {:.0} ms", apps.len(), t.elapsed().as_secs_f64() * 1e3);
    for a in apps.iter().take(12) {
        println!("  {:40} {}", a.name, a.id);
    }
    let t = std::time::Instant::now();
    let (mut ok, mut bytes, mut failed) = (0, 0, Vec::new());
    for a in &apps {
        match sill_catalog::icon_png(&a.id, 64) {
            Ok(png) => {
                ok += 1;
                bytes += png.len();
            }
            Err(e) => failed.push(format!("{} ({e})", a.name)),
        }
    }
    println!(
        "{ok} icons, {} kB, {:.0} ms; {} failed: {:?}",
        bytes / 1000,
        t.elapsed().as_secs_f64() * 1e3,
        failed.len(),
        failed.iter().take(3).collect::<Vec<_>>()
    );
    if let Some(a) = apps.iter().find(|a| a.name.contains("Notepad")) {
        std::fs::write(std::env::temp_dir().join("sill-notepad-icon.png"), sill_catalog::icon_png(&a.id, 64).unwrap())
            .unwrap();
        println!("wrote sill-notepad-icon.png in %TEMP%");
    }
}
#[cfg(not(windows))]
fn main() {}
