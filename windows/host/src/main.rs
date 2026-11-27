//! `sill-host`: the Windows host: pairing, sessions, and the desktop as hardware HEVC (Phase 2), or
//! the synthetic test picture with `--test-clip`. Runs on macOS too for development (plain files,
//! no Bonjour, test picture only). See windows/BUILD.md.

#[cfg(windows)]
mod desktop;
#[cfg(windows)]
mod input;

use sill_net::{Clip, Config, Host};
use sill_store::Store;
use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

const USAGE: &str = "usage: sill-host [options]
  --name NAME              the name devices show (default: this computer's name)
  --data-dir DIR           where the identity and paired devices live
                           (Windows default %LOCALAPPDATA%\\Sill; required elsewhere: DEV ONLY, unencrypted)
  --pair                   open a 5-minute pairing window and print its code and sill://pair link
                           (a host already running for this data dir opens it and this prints it)
  --list-devices           print the paired devices and exit
  --unpair NAME|KEY        remove the paired device with this name, or whose key (as --list-devices
                           prints it and log lines begin it) starts with KEY, and exit
  --require-pairing on|off the home door's Require pairing (saved), then run
  --remote-port N          the remote door's port (default 7455)
  --home-port N            the home door's port (default: a free one)
  --bind IP                listen on this address (repeatable; default loopback, private LAN, Tailscale)
  --allow-internet         also listen on public addresses
  --no-bonjour             do not advertise the home door
  --log-level LEVEL        error, warn, info (default) or debug
  --test-clip              stream the bundled test picture instead of the desktop
  --capture-scale X        stream at most X (0.1-1) of the monitor's pixels (default 1; devices
                           choose Retina = all of them, or Standard = Windows' points)
  --bitrate MBPS           starting megabits per second at 60 fps (default 15; devices can change
                           it; the stream gets bitrate x fps / 60)
  --probe-encoder          list the hardware HEVC encoders and exit
  --help";

struct Args {
    name: Option<String>,
    data_dir: Option<PathBuf>,
    pair: bool,
    list: bool,
    unpair: Option<String>,
    require_pairing: Option<bool>,
    remote_port: u16,
    home_port: u16,
    bind: Vec<IpAddr>,
    allow_internet: bool,
    bonjour: bool,
    log_level: String,
    test_clip: bool,
    capture_scale: f64,
    bitrate_mbps: f64,
    probe_encoder: bool,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        name: None,
        data_dir: None,
        pair: false,
        list: false,
        unpair: None,
        require_pairing: None,
        remote_port: 7455,
        home_port: 0,
        bind: Vec::new(),
        allow_internet: false,
        bonjour: true,
        log_level: "info".into(),
        test_clip: !cfg!(windows),
        capture_scale: 1.0,
        bitrate_mbps: 15.0,
        probe_encoder: false,
    };
    while let Some(flag) = it.next() {
        let mut value = |f: &str| it.next().ok_or_else(|| format!("{f} needs a value"));
        match flag.as_str() {
            "--name" => a.name = Some(value("--name")?),
            "--data-dir" => a.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--pair" => a.pair = true,
            "--list-devices" => a.list = true,
            "--unpair" => a.unpair = Some(value("--unpair")?),
            "--require-pairing" => {
                a.require_pairing = Some(match value("--require-pairing")?.as_str() {
                    "on" => true,
                    "off" => false,
                    v => return Err(format!("--require-pairing: on or off, not {v}")),
                })
            }
            "--remote-port" => {
                a.remote_port = value("--remote-port")?.parse().map_err(|_| "--remote-port: a port number")?
            }
            "--home-port" => a.home_port = value("--home-port")?.parse().map_err(|_| "--home-port: a port number")?,
            "--bind" => a.bind.push(value("--bind")?.parse().map_err(|_| "--bind: an IP address")?),
            "--allow-internet" => a.allow_internet = true,
            "--no-bonjour" => a.bonjour = false,
            "--log-level" => a.log_level = value("--log-level")?,
            "--test-clip" => a.test_clip = true,
            "--probe-encoder" => a.probe_encoder = true,
            "--capture-scale" => {
                a.capture_scale = value("--capture-scale")?
                    .parse()
                    .ok()
                    .filter(|x: &f64| (0.1..=1.0).contains(x))
                    .ok_or("--capture-scale: a number from 0.1 to 1")?
            }
            "--bitrate" => {
                a.bitrate_mbps = value("--bitrate")?
                    .parse()
                    .ok()
                    .filter(|x: &f64| (0.5..=200.0).contains(x))
                    .ok_or("--bitrate: megabits per second, 0.5 to 200")?
            }
            "--help" | "-h" => return Err(String::new()),
            f => return Err(format!("unknown option {f}")),
        }
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_times() {
        assert_eq!(super::utc(0.0), "1970-01-01 00:00 UTC");
        assert_eq!(super::utc(1_798_382_700.0), "2026-12-27 14:45 UTC");
        assert_eq!(super::utc(951_782_400.0), "2000-02-29 00:00 UTC");
        assert_eq!(super::utc(f64::NAN), "-");
    }
}

/// Writes each log line to the console and to the log file.
#[derive(Clone)]
struct Tee(Option<Arc<Mutex<std::fs::File>>>);

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stdout().write_all(buf);
        if let Some(f) = &self.0 {
            let _ = f.lock().unwrap().write_all(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

fn default_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "Windows PC".into())
}

fn main() {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("sill-host: {e}");
            }
            eprintln!("{USAGE}");
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };
    #[cfg(windows)]
    sill_capture::set_dpi_awareness();
    if args.probe_encoder {
        std::process::exit(probe_encoder());
    }
    let Some(dir) = args.data_dir.clone().or_else(Store::default_dir) else {
        eprintln!("sill-host: --data-dir DIR is required on this platform (DEV ONLY: files are not encrypted here).");
        std::process::exit(2);
    };
    let store = match Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sill-host: {}: {e}", dir.display());
            std::process::exit(1);
        }
    };

    if args.list || args.unpair.is_some() {
        std::process::exit(manage(&store, &args));
    }

    // One host per data directory: it holds this lock for as long as it runs.
    let lock = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("host.lock")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("sill-host: {}: {e}", dir.join("host.lock").display());
            std::process::exit(1);
        }
    };
    if lock.try_lock().is_err() {
        if args.pair {
            std::process::exit(ask_running_host_to_pair(&dir));
        }
        eprintln!(
            "sill-host: a host is already running for {}. `sill-host --pair` opens a pairing window in it.",
            dir.display()
        );
        std::process::exit(1);
    }

    let log_file = std::fs::create_dir_all(dir.join("logs"))
        .and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(dir.join("logs").join("host.log")))
        .ok()
        .map(|f| Arc::new(Mutex::new(f)));
    let tee = Tee(log_file);
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(format!("warn,sill_net={0},sill_host={0}", args.log_level)))
        .with_ansi(false)
        .with_target(false)
        .with_writer(move || tee.clone())
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    let code = rt.block_on(run(store, args));
    drop(lock);
    std::process::exit(code);
}

const PAIR_REQUEST: &str = "pair.request";
const PAIR_RESPONSE: &str = "pair.response";

/// `--pair` while a host runs for `dir`: asks it through `pair.request`, prints what it leaves in
/// `pair.response` (the code and link, read once and deleted).
fn ask_running_host_to_pair(dir: &std::path::Path) -> i32 {
    let mut nonce = [0u8; 12];
    if !sill_store::random(&mut nonce) {
        eprintln!("sill-host: the random generator failed.");
        return 1;
    }
    let nonce = sill_pairing::base64url::encode(&nonce);
    let _ = std::fs::remove_file(dir.join(PAIR_RESPONSE));
    if let Err(e) = std::fs::write(dir.join(PAIR_REQUEST), &nonce) {
        eprintln!("sill-host: could not ask the running host: {e}");
        return 1;
    }
    println!("A host is already running for {}: asking it to open a pairing window…", dir.display());
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let Ok(text) = std::fs::read_to_string(dir.join(PAIR_RESPONSE)) else { continue };
        let Some(rest) = text.strip_prefix(&format!("{nonce}\n")) else { continue };
        let _ = std::fs::remove_file(dir.join(PAIR_RESPONSE));
        print!("{rest}");
        return 0;
    }
    let _ = std::fs::remove_file(dir.join(PAIR_REQUEST));
    eprintln!("sill-host: the running host did not answer within 5 s (is it an older sill-host?).");
    1
}

/// Opens a pairing window and returns the lines `--pair` prints.
fn open_pairing(host: &Arc<Host>, running: &sill_net::Running) -> Result<Vec<String>, String> {
    let offer = host.open_pairing().map_err(|e| e.to_string())?;
    let mut lines = vec![
        format!(
            "Pairing open for {:.0} minutes. On the device: Add a Mac, Type address and code.",
            offer.expires_in / 60.0
        ),
        format!("  Code: {}", sill_pairing::code::grouped(&offer.code)),
        format!("  Link: {}", offer.link),
    ];
    for ip in running.addresses().iter().filter(|ip| !ip.is_loopback()) {
        lines.push(format!(
            "  Address: {ip} (port {} is the default; type it only if you changed it)",
            running.remote_port
        ));
    }
    Ok(lines)
}

/// Twice a second: a `sill-host --pair` from another process (its `pair.request`) opens a window
/// here, logged as at launch, and the lines go back in `pair.response`.
async fn answer_pair_requests(host: Arc<Host>, running: Arc<sill_net::Running>, dir: PathBuf) {
    let mut every = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        every.tick().await;
        let Ok(nonce) = std::fs::read_to_string(dir.join(PAIR_REQUEST)) else { continue };
        let _ = std::fs::remove_file(dir.join(PAIR_REQUEST));
        let nonce = nonce.trim();
        if nonce.is_empty()
            || nonce.len() > 64
            || !nonce.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            continue;
        }
        info!("Another sill-host asked for a pairing window.");
        let text = match open_pairing(&host, &running) {
            Ok(lines) => {
                for l in &lines {
                    info!("{l}");
                }
                lines.join("\n") + "\n"
            }
            Err(e) => {
                error!("Could not open a pairing window: {e}");
                format!("Could not open a pairing window: {e}\n")
            }
        };
        if let Err(e) = write_private(&dir.join(PAIR_RESPONSE), &format!("{nonce}\n{text}")) {
            warn!("Could not answer the pairing request: {e}");
        }
    }
}

/// Writes `path` whole (a temporary file renamed into place), readable by this user only outside
/// Windows (on Windows the data directory's own permissions keep it private).
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.create(true).truncate(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&tmp)?.write_all(text.as_bytes())?;
    std::fs::rename(tmp, path)
}

fn manage(store: &Store, args: &Args) -> i32 {
    let mut devices = match store.paired() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("sill-host: the paired devices could not be read: {e}");
            return 1;
        }
    };
    if let Some(which) = &args.unpair {
        let removed = match sill_store::remove_matching(&mut devices, which) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("sill-host: {e}");
                return 1;
            }
        };
        if removed.is_empty() {
            eprintln!("sill-host: no paired device matches {which:?}");
            return 1;
        }
        if let Err(e) = store.save_paired(&devices) {
            eprintln!("sill-host: could not save: {e}");
            return 1;
        }
        for d in removed {
            println!(
                "Unpaired {} ({}). A running sill-host drops its sessions within a second.",
                d.name, d.fingerprint
            );
        }
    }
    if args.list {
        if devices.is_empty() {
            println!("No paired devices.");
        }
        for d in &devices {
            println!(
                "{}  {}  model {}  app {}  paired {} by {}",
                d.fingerprint,
                d.name,
                d.model.as_deref().unwrap_or("-"),
                d.app.as_deref().unwrap_or("-"),
                utc(d.paired_at),
                d.method
            );
        }
    }
    0
}

async fn run(store: Store, args: Args) -> i32 {
    let store_dir = store.dir().to_path_buf();
    let mut config = Config::new(args.name.clone().unwrap_or_else(default_name));
    config.remote_port = args.remote_port;
    config.home_port = args.home_port;
    config.bind = args.bind.clone();
    config.allow_internet = args.allow_internet;
    config.bonjour = args.bonjour;
    if !sill_platform_protected() {
        warn!("DEV ONLY: {} holds the private key and paired devices UNENCRYPTED.", store.dir().display());
    }
    let clip = if args.test_clip { Some(Clip::bundled()) } else { None };
    let host = match Host::new(config, store, clip) {
        Ok(h) => h,
        Err(e) => {
            error!("Could not load the identity: {e}");
            return 1;
        }
    };
    let bitrate = (args.bitrate_mbps * 1e6) as i64;
    host.hub.settings.send_modify(|v| v.bitrate = bitrate);
    if !args.test_clip {
        host.hub.set_picture("the desktop starts when a device views it");
    }
    if let Some(on) = args.require_pairing {
        if let Err(e) = host.set_require_pairing(on) {
            error!("Could not save Require pairing: {e}");
            return 1;
        }
    }
    info!(
        "sill-host {} \"{}\": Mac ID {}, launch {}, Require pairing {}, {} paired device(s).",
        env!("CARGO_PKG_VERSION"),
        host.config.name,
        host.mac_id,
        host.launch_id,
        if host.require_pairing() { "on" } else { "off" },
        host.paired().len()
    );
    let dir = store_dir.clone();
    let running = match sill_net::start(host.clone()).await {
        Ok(r) => Arc::new(r),
        Err(e) => {
            error!("Could not start the doors: {e}");
            return 1;
        }
    };
    let _ = std::fs::remove_file(dir.join(PAIR_REQUEST));
    tokio::spawn(answer_pair_requests(host.clone(), running.clone(), dir));
    if args.pair {
        match open_pairing(&host, &running) {
            Ok(lines) => lines.iter().for_each(|l| info!("{l}")),
            Err(e) => error!("Could not open a pairing window: {e}"),
        }
    }
    let stop_desktop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(windows)]
    if !args.test_clip {
        let _ = host.input.set(Arc::new(input::DesktopInput::default()));
        let h = host.clone();
        desktop::spawn(
            host.hub.clone(),
            host.stats.clone(),
            stop_desktop.clone(),
            move || h.now_us(),
            desktop::Options { capture_scale: args.capture_scale },
        );
    }
    let _ = tokio::signal::ctrl_c().await;
    stop_desktop.store(true, std::sync::atomic::Ordering::Relaxed);
    info!("Quitting: goodbye to every device.");
    running.quit().await;
    0
}

/// "2026-11-27 16:25 UTC" for seconds since 1970.
fn utc(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "-".into();
    }
    let s = secs as i64;
    let (days, rest) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", rest / 3600, rest % 3600 / 60)
}

/// `--probe-encoder`: the hardware HEVC encoders Media Foundation lists, or why there are none.
fn probe_encoder() -> i32 {
    #[cfg(windows)]
    {
        match sill_encode::mf::probe() {
            Ok(names) if names.is_empty() => {
                println!(
                    "No hardware HEVC encoder: this PC cannot stream its desktop (sill-host --test-clip still works)."
                );
                1
            }
            Ok(names) => {
                for n in names {
                    println!("Hardware HEVC encoder: {n}");
                }
                0
            }
            Err(e) => {
                println!("Media Foundation failed: {e}");
                1
            }
        }
    }
    #[cfg(not(windows))]
    {
        println!("Hardware HEVC encoding is Windows only here.");
        1
    }
}

fn sill_platform_protected() -> bool {
    cfg!(windows)
}
