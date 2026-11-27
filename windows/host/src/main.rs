//! `sill-host`: the Windows host, Phase 1 (pairing, sessions, a synthetic test picture). Runs on
//! macOS too for development (plain files, no Bonjour). See windows/BUILD.md.

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
  --list-devices           print the paired devices and exit
  --unpair NAME|FP         remove paired devices by name or fingerprint (6+ leading characters) and exit
  --require-pairing on|off the home door's Require pairing (saved), then run
  --remote-port N          the remote door's port (default 7455)
  --home-port N            the home door's port (default: a free one)
  --bind IP                listen on this address (repeatable; default loopback, private LAN, Tailscale)
  --allow-internet         also listen on public addresses
  --no-bonjour             do not advertise the home door
  --log-level LEVEL        error, warn, info (default) or debug
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
            "--help" | "-h" => return Err(String::new()),
            f => return Err(format!("unknown option {f}")),
        }
    }
    Ok(a)
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
    std::process::exit(rt.block_on(run(store, args)));
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
        let removed = sill_store::remove_matching(&mut devices, which);
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
                "{}  {}  model {}  paired {:.0} by {}",
                d.fingerprint,
                d.name,
                d.model.as_deref().unwrap_or("-"),
                d.paired_at,
                d.method
            );
        }
    }
    0
}

async fn run(store: Store, args: Args) -> i32 {
    let mut config = Config::new(args.name.clone().unwrap_or_else(default_name));
    config.remote_port = args.remote_port;
    config.home_port = args.home_port;
    config.bind = args.bind.clone();
    config.allow_internet = args.allow_internet;
    config.bonjour = args.bonjour;
    if !sill_platform_protected() {
        warn!("DEV ONLY: {} holds the private key and paired devices UNENCRYPTED.", store.dir().display());
    }
    let host = match Host::new(config, store, Clip::bundled()) {
        Ok(h) => h,
        Err(e) => {
            error!("Could not load the identity: {e}");
            return 1;
        }
    };
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
    let running = match sill_net::start(host.clone()).await {
        Ok(r) => r,
        Err(e) => {
            error!("Could not start the doors: {e}");
            return 1;
        }
    };
    if args.pair {
        match host.open_pairing() {
            Ok(offer) => {
                info!(
                    "Pairing open for {:.0} minutes. On the device: Add a Mac, Type address and code.",
                    offer.expires_in / 60.0
                );
                info!("  Code: {}", sill_pairing::code::grouped(&offer.code));
                info!("  Link: {}", offer.link);
                for ip in running.addresses().iter().filter(|ip| !ip.is_loopback()) {
                    info!(
                        "  Address: {ip} (port {} is the default; type it only if you changed it)",
                        running.remote_port
                    );
                }
            }
            Err(e) => error!("Could not open a pairing window: {e}"),
        }
    }
    let _ = tokio::signal::ctrl_c().await;
    info!("Quitting: goodbye to every device.");
    running.quit().await;
    0
}

fn sill_platform_protected() -> bool {
    cfg!(windows)
}
