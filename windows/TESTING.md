# Testing

Status: layers 1–3 run on macOS (Phase 1 tasks A and B, 2026-11-27); Checkpoint 1 (layer 4) ran on
the laptop and the Duo on 2026-11-27 (results below); its fixes wait for a re-run of steps 4 and 5.

**Alert spellings.** An unpaired key is refused with the TLS alert `certificate_unknown` (46), as
the Mac host does (R1, below). Python's `ssl` prints it as `TLS_ALERT_CERTIFICATE_UNKNOWN`
(OpenSSL 3 and later) or `SSLV3_ALERT_CERTIFICATE_UNKNOWN` (older OpenSSL, e.g. some Windows
Pythons); both are a pass. Before 2026-11-27 the host sent `bad_certificate`
(`TLS_ALERT_BAD_CERTIFICATE` / `SSLV3_ALERT_BAD_CERTIFICATE`), which is now a failure: Apple's
client reads it as "isn't Sill".

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
     `AlertReceived(CertificateUnknown)` on both ALPNs and both doors, an ask at home (`openOnMac`),
     window list → host settings order, kind 0 then a keyframe on join, ping/pong (payload and
     timestamp echoed), junk kinds 200/30/17 skipped, ticks, keyframe request, oversized header
     closes, the 9th remote session gets `busy`, unpair → goodbye `removed` then the alert, quit →
     goodbye `quit` with `reconnect:true`, a silent remote client dropped. Unit tests: certificate
     key and SPKI reader, bind classes, clip format, ≤2 frames in flight then wait for a keyframe,
     store round trips, admission deadlines.
   - **TLS probe** (`windows/tools/tlsprobe`, macOS, CI on macos-latest): a SwiftPM tool that dials
     with the device's own `RemoteTLS.options(identity:role: .client(alpn:))` and
     `RemoteTLS.parameters(dialing: true)` from upstream `StreamProtocol`, a fresh P-256 key, and
     prints each NWConnection state and the OSStatus, then the device's verdict (`RESULT revoked`
     for -9825/-9829, `notSill` for any other TLS status). It is the only test that sees what
     Apple's stack reports; rustls and Python clients decode the alert by its number and cannot.
     ```
     cd windows && swift build -c release --package-path tools/tlsprobe
     tools/tlsprobe/.build/release/tlsprobe 127.0.0.1 17455 --expect=revoked   # exit 0 = "Not paired"
     ```
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
| 2026-11-27 | laptop + Duo | Checkpoint 1 at 4d6f4ab1 | see "Checkpoint 1 results" below: all steps pass except step 5's "Not paired" (R1) |
| 2026-11-27 | macOS (arm64), loopback | `tlsprobe` against sill-host and against upstream `SillHost --synthetic --remote` | see "R1: what Apple's client sees" below |
| 2026-11-27 | macOS (arm64) | `cargo test --workspace --locked` (Checkpoint 1 fixes) | 57 passed, 0 failed; clippy `-D warnings` (host and `--target x86_64-pc-windows-msvc` for sill-platform, sill-store) and fmt clean |

### Phase 2 MVP results (task/windows-phase2, 2026-11-27)

Laptop: HP, Core Ultra 7 155H, Intel Arc iGPU (driver 32.0.101.8831), Windows 11 Pro 26H2 build
26300.9974, one monitor 1920×1200 at 60 Hz, 100 % scaling. Phone: iPhone Duo (iPhone19,4) on the
same Wi-Fi. CPU is percent of **one** core. The device's own "frame age" (kind 12) compares the
laptop's clock with the phone's, whose offset is unknown (the laptop was 75–80 ms off
time.apple.com, `w32tm /stripchart`), so it is **not trustworthy** and §8 is not scored on it.

What works on the device (Kamal, App Store Sill 0.6.2 (1607) and Sill Dev 0.6.3 (9, 11)):
- The desktop on the Duo, and quality and resolution changes (kind 17) answered and applied.
  Before 8d1f9036 the app hung on them; before 7adf8b0f a slow keyframe stalled the stream.
- Control, Sill Dev 11 with `pcKeys: 1`: pointer, click, right-click, drag, typing in Notepad,
  copy and paste, the Start button and the Windows key row, scrolling in Edge. Kamal: "All work".
- Picture: "the screen looks ok, but the text is a little off looking, it's not as sharp as the
  mac screen, which is expected". A 1920×1200 desktop shown in a 935×567 pt panel is downscaled
  about 2×.
- Not yet: the apps drawer (Phase 3, first item).

Measurements (Wi-Fi only; the Tailscale run is pending):

| | Run 1: 1920×1200 (default) | Run 2: `--capture-scale 0.67`, 1286×804 | Run A: 1920×1200, latency meter |
|---|---|---|---|
| Length, door | 103 s, remote (typed address) | 69 s, home | 193 s, remote |
| Frames sent, busiest second | 27 | 34 | 30 |
| Drops | 0 | 0 | 5 |
| Encode, submit → AU | 34.5 ms median, 39.8 p95 | 26.9 ms median, 31.1 p95 | 32.4 ms median |
| Host latency, capture → AU | — | — | 39 ms median, 48 p95 |
| Host latency, capture → last byte sent | — | — | 40 ms median |
| RTT (kind 12) | 5 ms median, 15 p95, 57 max | 5 median, 8 p95, 27 max | 7 median, 73 p95 (Wi-Fi spikes) |
| CPU, moving | 14.1 % median, 21.9 p95 | 10.9 % median | 15–17 % |
| CPU, still screen | 0.0 % (66 s) | 0.0 % (43 s) | — |
| Memory, streaming | 194 MB | 162 MB | 196 MB |
| GPU, by engine | VideoProcessing 36 %, VideoDecode (Intel's encoder) 34 %, 3D 9 % | VideoProcessing 40 %, VideoDecode 27 % | VideoDecode 42 %, VideoProcessing 38 % |

Glass to glass: Kamal filmed the laptop's stopwatch and its copy on the Duo (1920×1200, Wi-Fi),
and 12 frames of each video were read.
- Video 1 (slow motion, 3–4 ms a frame): the Duo **40–70 ms behind, about 55 ms median**.
- Video 2 (239.5 fps, 4.2 ms a frame): **30–70 ms, about 40 ms median**.
- The stopwatch draws about every 33 ms, and the laptop's last digit blurs between two values, so
  each reading is good to ±15 ms. This is display to display: the laptop panel's own lag is not
  in it.
- Upstream documents the Mac host at about 40–60 ms glass to glass on a LAN (`docs/BRIEF.md`
  lines 82 and 101), so the Windows host is in the Mac's range. §8's 20 ms was this port's own
  stretch target.

Idle footprint, host running and no client:
- Before any viewer: 0.00 % CPU, 17.8 MB working set, 7.8 MB private.
- 30 s and 1 min after the last viewer left: 0.16–0.31 % CPU, 63 MB working set, 34–35 MB private.
  Direct3D and Media Foundation stay loaded after the pipeline is torn down.
- 10 min: not taken yet (the phone reconnected during the attempt).

Against the MVP targets (WINDOWS_PORT.md §8):

| Target | Result |
|---|---|
| LAN median frame age ≤ 20 ms | **Missed** (a stretch target). Host capture → sent is 40 ms median, glass to glass 40–55 ms median: within the Mac host's documented 40–60 ms. The encoder (32 ms) is most of it. |
| RTT ≤ 15 ms | **Met:** 5–7 ms median. |
| 60 fps at 1080p, ≤ 15 Mbps | **Missed.** The Intel MF encoder takes one frame in flight, at about 30 ms at 1920×1200: 30–35 fps at most (R3). |
| Host CPU ≤ 10 % of one core, static desktop | **Met:** 0 % (WGC sends no frames when nothing changes). |
| Idle ≈ 0 % CPU, < 40 MB | **Met** before any viewer (17.8 MB). **Missed** after one (63 MB): a Phase 5 item (unload the pipeline's DLLs, or a separate capture process). |

Next for latency: send the frame to the encoder at once (capture → submit is about 7 ms), a
BGRA input or a shader conversion to free the video processor (36–40 %), and Phase 5's direct
oneVPL path.

### Checkpoint 1 results (4d6f4ab1, 2026-11-27)

Laptop: Intel Arc, Windows 11 26H2. Duo: App Store Sill 0.6.2 (1607) and Sill Dev 0.6.3 (7).

| Step | Result |
|---|---|
| 0 prerequisites | PASS (both profiles Private) |
| 1 binary | PASS (release build in 24.4 s, 3.79 MB) |
| 2 pre-flight | PASS |
| 3 Tailscale pairing and stream (App Store build) | PASS: 30 fps, 0 drops, RTT 50–60 ms |
| 4 reconnect | PASS: flight mode about 2 s on LAN; Wi-Fi → Tailscale handover in 4.4 s. The 8 s silent drop was not exercised |
| 5 unpair | goodbye `removed` and no redial for 75 s: PASS. "Not paired" after a tap: **FAIL (R1)**, the Duo said "Something answered at 100.90.47.26, but it isn't Sill." |
| 6 quit | PASS; auto-reconnect after a 3 s restart: PASS; after an 80 s outage the Duo needed a tap (see below) |
| 7 Bonjour LAN | PASS, partial |

Also seen: frame age 0 ms in every device stats line; 15 keyframes/s (672 kbit/s) into a stalled
link during flight mode; WSL's vEthernet address 172.28.16.1 listened on and put in the pairing
link; "picked the Desktop." twice per connection; key shown as `D38CQ4…` in logs but `aNDLkEyP…` in
`--list-devices`; `--pair` could not reach a running host; the Dev and App Store builds both say
hello as "iPhone". Fixed after the checkpoint on `task/windows-phase1` (each needs the laptop to
confirm): R1 (alert), the keyframe rule, virtual adapters skipped, one pick line, base64url key
labels and `--unpair` by unique key prefix, `--list-devices` with the pairing time (UTC) and the
app of the last hello, `--pair` to a running host, Bonjour withdrawn before exit.

Not changed, with the reason:
- **Frame age 0:** the host stamps each frame with its wall clock at send (unit test
  `frames_carry_the_host_wall_clock_at_send`); the device clamps a negative age to 0
  (`iOSClient/StreamClient.swift`, `handle`, `.frame`). So 0 means the PC's clock runs ahead of the
  Duo's by more than the real age. Check with `w32tm /stripchart` (Diagnostics).
- **80 s outage needed a tap:** the device's reconnect after goodbye `quit` has no time limit; it
  ends only on a connection, a tap, Disconnect or Forget, and dials only while Sill is in the
  foreground (`DiscoveryPolicy.redialRow`: up to 12 dials per listing, 5/10/20/30 s apart; a row
  listed again starts the count again). A returning row is matched by its `r` tag (the stored
  recognition key survives a restart) or its name, never its port; the Mac host also takes a new
  home port on every start. Likely cause: on the connect screen Sill no longer holds auto-lock off
  (`iOSClient/DeviceIdlePolicy.swift`), so an 80 s wait can lock the phone and stop the dials. Not
  proven. Re-run with the phone kept awake (touch it now and then) and note whether it locked. Host
  side, `quit` now withdraws the Bonjour registration before the process ends.

### R1: what Apple's client sees (macOS loopback, 2026-11-27)

`windows/tools/tlsprobe` (the device's `RemoteTLS` options) with a fresh, unpaired key:

| Host | Alert on the wire (`openssl s_client -msg`) | Probe | Verdict |
|---|---|---|---|
| upstream `SillHost --synthetic --remote=17465` (headless test env) | `fatal certificate_unknown` (46) | ready at 7–10 ms, first receive fails `tls(-9825)` at 8–10 ms | revoked → "Not paired" |
| sill-host at 4d6f4ab1 | `fatal bad_certificate` (42) | ready at 5–11 ms, then `tls(-9808)` (receive, or `.failed`) | notSill → "isn't Sill" |
| sill-host after the fix | `certificate_unknown` (46) | ready at ~6 ms, first receive `tls(-9825)`, both doors and `sill-pair/1` | revoked |

Record shapes match on both hosts (a logging TCP relay: the server's Finished flight, the client's
flight, one 19-byte encrypted alert, then FIN). The keys and epochs were not the cause; the alert
number was. Apple's client reports a received `certificate_unknown` as -9825 and `bad_certificate`
as -9808. Regression guard: the CI macOS job runs the probe against sill-host with
`--expect=revoked`, and `crates/sill-net/tests/host.rs` asserts `CertificateUnknown`.

### sillclient.py runs (macOS loopback, 2026-11-27)

Host: `target/debug/sill-host --data-dir $S/hd --name "Test PC" --remote-port 17455 --home-port 17456 --bind 127.0.0.1 --pair --log-level debug`
(ports off 7455: Sill.app runs on this Mac). `$S` is a scratch directory; `CODE`/`LINK` are what `--pair` printed.

| # | Command | Result |
|---|---|---|
| 1 | `python3 Scripts/sillclient.py 17455 4 desktop --identity=$S/id --pair-code=$CODE --hello=1.0,1 --device=sillclient --junk --fps=60 --ping=0.25 --keyframe@2` | `PAIR ok: Test PC (…), proof_M checked, pin saved`; session: `first kinds: 2 16`, 120 frames (8 key) in 4 s = 30.0 fps, 16 pongs (rtt p50 0.3 ms), 17 ticks; host log: kinds 200 and 201 skipped, viewport fps 60, keyframe request |
| 2 | same `--pair-code` again from a new identity (window used) | `PAIR FAIL: [SSL: TLS_ALERT_BAD_CERTIFICATE]` (remote door takes no pairing without a window; since R1's fix `…CERTIFICATE_UNKNOWN`) |
| 3 | `… 17455 3 --identity=$S/id --tls --big-payload=2000000` | `EOF from host at 0.00s` after kinds 2, 16; host: `kind 200 announces 2000000 bytes, over the cap of 1048576; closed.` |
| 4 | `… 17455 2 --identity=$S/id3 --tls --pin=none --expect-tls-fail` (unpaired key) | `read failed at 0.00s: [SSL: TLS_ALERT_BAD_CERTIFICATE] tls alert bad certificate`, `EXPECT-TLS-FAIL ok: refused` (the handshake completed first, as on Apple's stack); since R1's fix `[SSL: TLS_ALERT_CERTIFICATE_UNKNOWN]` |
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
`[SSL: TLS_ALERT_CERTIFICATE_UNKNOWN]` (or `SSLV3_ALERT_CERTIFICATE_UNKNOWN`) and
`EXPECT-TLS-FAIL ok: refused`. Ctrl+C the host.

### 3. Pair the Duo over Tailscale
```
tailscale ip -4                                   # note 100.x.y.z
.\windows\target\release\sill-host.exe --pair
```
Host prints `Listening on 100.x.y.z (Tailscale): remote door 100.x.y.z:7455, …`,
`Bonjour: advertising "<name>" as _sill._tcp on port N (p=…).`, `Pairing open for 5 minutes…`,
`Code: dddd dddd dddd`, then about 3 s later `Bonjour: registered.`
No `Listening on 172.x…` line for the Hyper-V/WSL adapter (virtual adapters are skipped since
2026-11-27). If a host is already running, `sill-host --pair` in a second window asks it to open
the window and prints the code there.
On the Duo (App Store build): Sill → Add a Mac → Type address and code → `100.x.y.z` → the code → Pair.
Expected host log, in order:
- `Paired <device> (key XXXXXXXX…) from 100.a.b.c, with the code.` (the key as the first 8
  characters of what `--list-devices` prints)
- `Client connected: <device> at 100.a.b.c:port (remote door).`
- `Client hello: <device>, Sill <version> (<build>), protocol 1 (…)`
- `Viewport from <device>: W×H points, fps N (the test picture plays at 30).`
- once a second: `Device stats <device>: fps … frame age … ms … rtt … ms …` (frame age is the
  device's clock minus the host's send time; it reads 0 when the PC's clock runs ahead of the
  phone's, see Diagnostics) and
  `stats: sessions 1 (home 0, remote 1) | frames sent 30 dropped 0 …`
Expected on the Duo: "Paired", then the test picture (colour bars, grey ramp, a moving white bar,
"Sill test picture frame NN/30") within 2 s. Touch input does nothing yet (Phase 2); with
`--log-level debug` the host logs each input event.

### 4. Reconnect
Phone in flight mode 10 s, then off: it reconnects by itself within 5 s (`Client left: … silent
for 8 s; dropped` may appear first, then a new `Client connected`). While the phone is away the
stats line must show `keyframes` 0–2 per second at most (the test clip's own) and `dropped`
growing, never 15 keyframes a second; after it is back, one keyframe, then the clip's.

### 5. Unpair → "Not paired", no redial
In a second PowerShell: `.\windows\target\release\sill-host.exe --list-devices`, then
`.\windows\target\release\sill-host.exe --unpair <first characters of its key>` (a name works
when only one device has it). Expected host log: `Paired devices changed on disk: 0 now.`,
`<device> at …: no longer paired; goodbye removed.`, then **no further lines** from that phone.
Then tap the host's row on the Duo once: one `Refused 100.a.b.c:port at the remote door: key
XXXXXXXX… not trusted for sill/1 (TLS alert certificate_unknown).`, and the Duo shows **"Not
paired"** for this host and does not redial (risk R1 closed only if it does). Before the fix it
showed "Something answered at …, but it isn't Sill."

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
w32tm /stripchart /computer:time.apple.com /samples:5 /dataonly   # PC clock vs Apple's (frame age 0 = PC ahead)
w32tm /resync                                     # elevated; once the offset above is over ~20 ms
```

## Performance measurements
From the host stats line and kind 12: frame age median/max, RTT median/max, fps, drops, encode ms,
CPU %, working set, GPU utilisation. Targets in WINDOWS_PORT.md §8.
