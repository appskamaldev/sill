# Build

Status: Phase 1 task B. `sill-host` pairs and runs sessions with a synthetic test picture; it
builds and runs on Windows and (for development) macOS. Real capture, encoding and input are Phase 2.

## Layout

```
windows/
├── Cargo.toml            workspace (edition 2021); Cargo.lock is committed, CI builds --locked
├── crates/sill-wire      header, kinds, caps, frame reader, blobs, JSON payloads, SillVersion
├── crates/sill-pairing   code, proofs, tag, door TXT, SPKI/MacID, link, addresses, SafeText, kind 18
├── crates/sill-policy    door trust, pairing window, device gate, viewport bounds, admission deadlines
├── crates/sill-platform  DPAPI and DnsServiceRegister (cfg(windows)); stand-ins elsewhere
├── crates/sill-store     identity (P-256 key, recognition key), paired devices, Require pairing
├── crates/sill-net       TLS doors, pairing, sessions, synthetic stream (tokio + rustls)
├── host/                 the sill-host binary (tray icon, control pipe; CLI flags for development)
├── app/Sill/             the Sill app (C#, WinUI 3): main window and tray flyout, talks to the host
├── testdata/clip.bin     the synthetic picture: 30 HEVC frames, 1280×800 (format: sill-net/src/video.rs)
├── vectors/*.json        golden vectors from upstream Swift (generated, committed)
└── tools/                make-vectors.sh, the vector generator (tools/vectors), testclip (tools/testclip),
                          sillclient-win.py (upstream Scripts/sillclient.py on Windows)
```

## Portable crates (macOS or Windows)

Prerequisite: Rust stable via rustup (on Windows the `x86_64-pc-windows-msvc` toolchain, which
needs the Visual Studio 2022 Build Tools for the linker). No other system packages.

```
cd windows
cargo test --workspace                                   # all crates, all vectors
cargo test -p sill-wire                                  # one crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

Dependencies build optimised even in debug (`[profile.dev.package."*"]`), because the PBKDF2
vectors run 600,000 rounds.

On this Mac Rust is not on `PATH`: `. ~/.cargo/env` first, or call `~/.cargo/bin/cargo`.

## sill-host

Build: `cd windows && cargo build -p sill-host --release` → `target/release/sill-host(.exe)`.
CI also builds `sill-host.exe` on `windows-latest` and keeps it 14 days as the artifact
`sill-host-windows-x64` of each run (Actions → the run → Artifacts).

Windows (PowerShell, from `windows\`):
```
.\target\release\sill-host.exe --pair                # open a 5-minute window: prints the code and link
.\target\release\sill-host.exe                       # run (Ctrl+C: goodbye "quit" to every device)
.\target\release\sill-host.exe --list-devices
.\target\release\sill-host.exe --unpair "Kamal's iPhone"   # or 6+ leading characters of the fingerprint
```
Data: `%LOCALAPPDATA%\Sill\` (`identity.bin`, `recognition.bin`, `devices.json.dpapi` sealed with
DPAPI for the current user; `settings.json`; `logs\host.log`). Doors: remote 7455, home a free
port advertised by Bonjour `_sill._tcp`. Default bind: loopback, private LAN and Tailscale
addresses (new ones picked up every 10 s); never a public address without `--allow-internet`.
Windows Firewall asks on first run: allow **Private** networks only.

macOS (development only; no Bonjour, no DPAPI: files are plain and marked `DEV-ONLY-UNENCRYPTED.txt`):
```
./target/release/sill-host --data-dir /tmp/sill-dev --remote-port 17455 --bind 127.0.0.1 --pair
```
Use a port other than 7455 if Sill.app runs on the same Mac.

Flags: `--name`, `--data-dir`, `--pair`, `--list-devices`, `--unpair NAME|FP`, `--require-pairing on|off`
(saved, then runs), `--remote-port`, `--home-port`, `--bind IP` (repeatable), `--allow-internet`,
`--no-bonjour`, `--log-level error|warn|info|debug` (input events and skipped kinds are debug).
`--list-devices` and `--unpair` act on the files and exit; a running host notices within a second
and sends the unpaired device goodbye `removed`.

## The Sill app (WinUI 3)

Needs the .NET 8 SDK (8.0.4xx); NuGet fetches the Windows App SDK 1.6. No Visual Studio needed.
```
cd windows\app\Sill
dotnet build -c Release                                  # bin\Release\net8.0-windows10.0.22621.0\win-x64\Sill.exe
dotnet publish -c Release -r win-x64 --self-contained -o <install dir>\app   # trimmed + ReadyToRun, about 84 MB
```
Install layout: `<dir>\sill-host.exe` and `<dir>\app\Sill.exe` (+ its files). The host opens the app
from the tray (`--flyout`; `--pair` for Pair a device) and lets onto its control pipe
(`\\.\pipe\sill-host-<user SID>`, this user and SYSTEM only, no remote clients) only processes whose
exe is inside its own folder; `sill-host.exe --control-any-client` lifts that for development.
The app starts the host if it is not running, keeps one instance (a second launch is handed to the
first), and exits when its last window closes; the host and the tray icon stay.
Development flags: `--page status|pair|devices|streaming|network|logs|about`, `--theme light|dark`,
`--stay` (the flyout stays open without focus), `--timing` (launch times in app.log; every flyout open logs its draw time anyway). App errors: `%LOCALAPPDATA%\Sill\logs\app.log`.

## Synthetic clip (macOS, no Xcode project, no simulator: no xcode-lock)

```
swiftc -O windows/tools/testclip/main.swift -o /tmp/testclip && /tmp/testclip windows/testdata/clip.bin
```
VideoToolbox, HEVC Main 8-bit, hardware encoder, real time, no reordering, IDR at frames 0 and 15.
Commit the new `clip.bin` (≈32 KB).

## Build directory layout (note from the Windows test session)

Test executables appear under `target\debug\build\<crate>\<hash>\out\`. That is expected: this
Cargo (1.100) uses the new build-directory layout, which keeps every compiled unit, test binaries
included, in `build/<package>/<hash>/out` instead of `deps/`. No crate in `windows/` has a build
script; `ring`'s build script (C and assembly) writes only its own objects there.

## Golden vectors (macOS only, Swift 5.9+; no Xcode project, no simulator)

```
windows/tools/make-vectors.sh
```

Builds `windows/tools/vectors` (a SwiftPM executable depending on the repository root by path,
product `StreamProtocol`), runs it with `SWIFT_DETERMINISTIC_HASHING=1` so `JSONEncoder`'s key order
is the same each run, and rewrites `windows/vectors/{wire,json,pairing,text}.json`. Run it after every
upstream merge, then `cargo test --workspace`; commit the vectors with any fix.

Kind 18's signatures are pinned in the generator (ECDSA is randomized in CryptoKit): if the fixed
info bytes ever change, the tool stops and prints the new signature to paste in.

## CI

`.github/workflows/windows.yml` (the fork's only workflow file): on pushes to `task/**` and on pull
requests touching `windows/**`. `windows-latest`: test, clippy, fmt. `macos-latest`: test.

## iOS client

Unmodified upstream for MVP (App Store build). Building from source follows upstream
`docs/DEVELOPMENT.md`.
