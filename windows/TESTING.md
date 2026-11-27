# Testing

Status: layers 1–3 run on macOS (Phase 1 tasks A and B, 2026-11-27); layer 3 on Windows and layers
4–5 wait for the laptop and the Duo.

## Layers
1. **Conformance vectors (macOS + Windows, CI):** `windows/tools/make-vectors.sh` compiles upstream
   `StreamProtocol` and writes `windows/vectors/*.json`; `cd windows && cargo test --workspace` must
   match every one. What they cover:
   - `wire.json`: caps, kinds, header serialization (fractional, -0, denormal timestamps), parseHeader
     (unknown and reserved kinds, keyframe byte ≠ 1, short input), ParameterSets, thumbnail, icon and
     cursor blobs, encode and decode.
   - `json.json`: every payload as `JSONEncoder` writes it (plain and `.sortedKeys`) and 122 decode
     inputs with Swift's verdict (missing required, null optionals, unknown keys, wrong types, whole
     floats into integers, enum one-case objects, structs given as arrays, Data base64 edge cases);
     kind 18 signatures (valid, tampered, wrong key, and one made by the Rust code).
   - `pairing.json`: Damm digits and `check`, PBKDF2 code keys, proofs, recognition tags, door TXT,
     base64url, SPKI DER, fingerprints and Mac IDs, pairing links (4 urls, 48 parses), ALPNs.
   - `text.json`: 147 addresses (IPv4, Tailscale 100.x, IPv6 forms, MagicDNS names, ports, every
     refusal, inet_aton/inet_pton quirks), SillVersion parse/compare, SafeText labels, viewport bounds.
   Rust compares parsed JSON values (numbers by value, objects by key set), never bytes or key order,
   and checks that its own output decodes again.
2. **Unit tests** for ported policies, with the cases upstream's `Tests/checks/*` encode:
   `sill-policy/tests/policy.rs` ports door-policy (verify block over 2 doors × 7 ALPNs × 128 trust
   states, `.ready` × 0/7/8/9 sessions, kind 19 by door and `v`), the PairingWindow sequence from
   `Tests/checks/addresses` and all of `Tests/checks/device-gate`. Lock and clipboard policies come
   with their phases. The frame reader's caps are unit-tested in `sill-wire/src/header.rs`.
3. **Protocol integration (no device):**
   - `crates/sill-net/tests/host.rs` (CI, both OSes): a rustls client doing what the device does
     (TLS 1.3, its own P-256 certificate, one ALPN, the host pinned) over loopback: pairing by code
     with both proofs checked and the device saved, a wrong code (`code`, 4 tries left), a used
     window (remote door: TLS alert; home door: `closed`), the unpaired-key refusal asserted as
     `AlertReceived(BadCertificate)` on both ALPNs and both doors, an ask at home (`openOnMac`),
     window list → host settings order, kind 0 then a keyframe on join, ping/pong (payload and
     timestamp echoed), junk kinds 200/30/17 skipped, ticks, keyframe request, oversized header
     closes, the 9th remote session gets `busy`, unpair → goodbye `removed` then the alert, quit →
     goodbye `quit` with `reconnect:true`, a silent remote client dropped. Unit tests: certificate
     key and SPKI reader, bind classes, clip format, ≤2 frames in flight then wait for a keyframe,
     store round trips, admission deadlines.
   - Upstream `Scripts/sillclient.py` (Python `ssl`) against `sill-host` on loopback. On Windows run
     it through `windows/tools/sillclient-win.py`, which swaps upstream's `/usr/bin/openssl`,
     `/usr/sbin/lsof` and `/bin/ps` for local tools (`$SILL_OPENSSL`, PATH, then Git for Windows'
     `usr\bin` and `mingw64\bin` openssl.exe) without editing upstream; on macOS it runs the
     script unchanged.
4. **Hardware checkpoints (Kamal):** the Duo with the App Store build, LAN and Tailscale.
5. **Regression vs Mac host:** the same device build still works with Sill.app on the Mac after any
   client change.

## Results

| Date | Where | Command | Result |
|---|---|---|---|
| 2026-11-27 | macOS (arm64) | `cargo test --workspace` | 33 tests passed, 0 failed |
| 2026-11-27 | CI windows-latest, macos-latest | `.github/workflows/windows.yml` (test; clippy and fmt on Windows) | 33 passed, 0 failed on each; clippy and fmt clean: https://github.com/appskamaldev/sill/actions/runs/46020622253 |
| 2026-11-27 | macOS (arm64) | `cargo test --workspace --locked` (task B) | 52 passed, 0 failed; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean; `cargo clippy -p sill-platform -p sill-store --all-targets --target x86_64-pc-windows-msvc -- -D warnings` clean |
| 2026-11-27 | macOS, sillclient.py | see "sillclient.py runs" below | all as expected |

### sillclient.py runs (macOS loopback, 2026-11-27)

Host: `target/debug/sill-host --data-dir $S/hd --name "Test PC" --remote-port 17455 --home-port 17456 --bind 127.0.0.1 --pair --log-level debug`
(ports off 7455: Sill.app runs on this Mac). `$S` is a scratch directory; `CODE`/`LINK` are what `--pair` printed.

| # | Command | Result |
|---|---|---|
| 1 | `python3 Scripts/sillclient.py 17455 4 desktop --identity=$S/id --pair-code=$CODE --hello=1.0,1 --device=sillclient --junk --fps=60 --ping=0.25 --keyframe@2` | `PAIR ok: Test PC (…), proof_M checked, pin saved`; session: `first kinds: 2 16`, 120 frames (8 key) in 4 s = 30.0 fps, 16 pongs (rtt p50 0.3 ms), 17 ticks; host log: kinds 200 and 201 skipped, viewport fps 60, keyframe request |
| 2 | same `--pair-code` again from a new identity (window used) | `PAIR FAIL: [SSL: TLS_ALERT_BAD_CERTIFICATE]` (remote door takes no pairing without a window) |
| 3 | `… 17455 3 --identity=$S/id --tls --big-payload=2000000` | `EOF from host at 0.00s` after kinds 2, 16; host: `kind 200 announces 2000000 bytes, over the cap of 1048576; closed.` |
| 4 | `… 17455 2 --identity=$S/id3 --tls --pin=none --expect-tls-fail` (unpaired key) | `read failed at 0.00s: [SSL: TLS_ALERT_BAD_CERTIFICATE] tls alert bad certificate`, `EXPECT-TLS-FAIL ok: refused` (the handshake completed first, as on Apple's stack) |
| 5 | `… 17456 2 --identity=$S/id3 --pair-ask --pin=none` (home door, no window) | `pairResult: {"ok": false, "reason": "openOnMac"}` |
| 6 | `… 17456 2 --identity=$S/id3 --pair-cancel` | `{"ok": false, "reason": "closed"}` |
| 7 | host restarted with `--pair`; `python3 windows/tools/sillclient-win.py 17455 2 --identity=$S/id4 --pair-url="$LINK" --hello=1.0` | QR path `PAIR ok`, 60 frames in 2 s, `first kinds: 2 16` |
| 8 | `SILL_OPENSSL=$(which openssl) python3 windows/tools/sillclient-win.py 17455 2 --identity=$S/id5 --tls --pin=none --expect-tls-fail` | new identity made with the Homebrew openssl; `EXPECT-TLS-FAIL ok` |
| 9 | `… 17456 1 --identity=$S/id3 --pair-code=$CODE --pair-v=2 --pin=none` | `{"ok": false, "reason": "closed"}` |
| 10 | session running, then Ctrl+C (SIGINT) to the host | client: `goodbye at 1.973s: quit; reconnect: true`, `EOF from host`; host exited |

## Checkpoint 1 procedure (Phase 1, on the laptop and the Duo)

### 0. Prerequisite check on the laptop
```
Get-NetConnectionProfile          # expect Wi-Fi: Private, Tailscale: Private
```
Both must be **Private** (Kamal set the Wi-Fi to Private on 2026-11-27; if it shows Public again,
the LAN/Bonjour test in step 7 will fail at the firewall). Windows Firewall's prompt on the first
run of `sill-host.exe`: allow **Private networks only**. The default rule stays Private + Tailscale;
never allow Public.

### 1. Get the binary
From the CI run of the pushed commit (Actions → run → Artifacts → `sill-host-windows-x64`), or build:
```
cd windows
cargo build -p sill-host --release
```

### 2. Pre-flight without the phone (PowerShell, repository root)
```
$env:SILL_OPENSSL = 'C:\Program Files\Git\usr\bin\openssl.exe'
.\windows\target\release\sill-host.exe --data-dir $env:TEMP\sill-preflight --remote-port 17455 --bind 127.0.0.1 --no-bonjour --pair
# in a second PowerShell, with the 12 digits it printed:
python windows\tools\sillclient-win.py 17455 4 --identity=$env:TEMP\sillclient-id --pair-code=123456789012 --hello=1.0 --junk
python windows\tools\sillclient-win.py 17455 2 --identity=$env:TEMP\sillclient-other --tls --pin=none --expect-tls-fail
```
Expected: `PAIR ok: … proof_M checked`, `first kinds: 2 16`, about 30 fps; then
`[SSL: TLS_ALERT_BAD_CERTIFICATE]` and `EXPECT-TLS-FAIL ok: refused`. Ctrl+C the host.

### 3. Pair the Duo over Tailscale
```
tailscale ip -4                                   # note 100.x.y.z
.\windows\target\release\sill-host.exe --pair
```
Host prints `Listening on 100.x.y.z (Tailscale): remote door 100.x.y.z:7455, …`,
`Bonjour: registered.`, `Pairing open for 5 minutes…` and `Code: dddd dddd dddd`.
On the Duo (App Store build): Sill → Add a Mac → Type address and code → `100.x.y.z` → the code → Pair.
Expected host log, in order:
- `Paired <device> (key XXXXXX…) from 100.a.b.c, with the code.`
- `Client connected: <device> at 100.a.b.c:port (remote door).`
- `Client hello: <device>, Sill <version> (<build>), protocol 1 (…)`
- `Viewport from <device>: W×H points, fps N (the test picture plays at 30).`
- once a second: `Device stats <device>: fps … frame age … ms … rtt … ms …` and
  `stats: sessions 1 (home 0, remote 1) | frames sent 30 dropped 0 …`
Expected on the Duo: "Paired", then the test picture (colour bars, grey ramp, a moving white bar,
"Sill test picture frame NN/30") within 2 s. Touch input does nothing yet (Phase 2); with
`--log-level debug` the host logs each input event.

### 4. Reconnect
Phone in flight mode 10 s, then off: it reconnects by itself within 5 s (`Client left: … silent
for 8 s; dropped` may appear first, then a new `Client connected`).

### 5. Unpair → "Not paired", no redial
In a second PowerShell: `.\windows\target\release\sill-host.exe --unpair "<device name>"`.
Expected host log: `Paired devices changed on disk: 0 now.`, `<device> at …: no longer paired;
goodbye removed.`, then at most a few `Refused 100.a.b.c:port at the remote door: key XXXXXX… not
trusted for sill/1 (TLS alert bad_certificate).` and then **no further lines** from that phone.
The Duo shows "Not paired" for this host (risk R1 closed only if it does).

### 6. Quit
Ctrl+C on the host: the phone shows the host went away and reconnects when it is back.

### 7. LAN via Bonjour (Wi-Fi Private, see 0)
Phone on the same Wi-Fi, Tailscale off on the phone: the host appears under its name without
typing; pair or connect as above (home door, `Client connected: … (home door)`).

### Diagnostics
```
Get-NetTCPConnection -LocalPort 7455 -State Listen
Get-NetConnectionProfile                          # both Private
Get-NetFirewallRule -DisplayName "*sill*" | Format-Table DisplayName,Enabled,Profile,Action
Test-NetConnection 100.x.y.z -Port 7455           # from another machine on the tailnet
Get-Content $env:LOCALAPPDATA\Sill\logs\host.log -Tail 50
tailscale status
.\windows\target\release\sill-host.exe --log-level debug   # handshake failures, skipped kinds, input
```

## Performance measurements
From the host stats line and kind 12: frame age median/max, RTT median/max, fps, drops, encode ms,
CPU %, working set, GPU utilisation. Targets in WINDOWS_PORT.md §8.
