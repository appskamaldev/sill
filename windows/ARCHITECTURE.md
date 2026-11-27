# Architecture

## 1. Original Sill architecture (upstream `22ab5a3e`)

| Part | Path | Role |
|---|---|---|
| Wire | `Sources/StreamProtocol/` (~3.7k lines) | Shared by host and client: framing, kinds, JSON types, TLS builder, pairing crypto, policies that both sides need. Pure Swift + Network/Security/CryptoKit. |
| Host core | `Sources/SillHost/` (`SillHostCore`, 76 files) | Capture (ScreenCaptureKit), encode (VideoToolbox), network (Network.framework), input (CGEvent), catalog, virtual display, audio, clipboard, menus, lock/privacy. |
| Menu bar app | `Sources/SillMenuBar/` | Sill.app: status item, Settings, Log, pairing window, updater, login item. |
| CLI | `Sources/SillHostCLI/` | `SillHost` with flags (`--pairing`, `--remote`, `--synthetic`, test hooks). |
| iOS client | `iOSClient/` (68 files) | SwiftUI/UIKit app; `StreamClient` is the session; `HEVCDisplayView` decodes with AVSampleBufferDisplayLayer; Duo posture layouts; trackpad. |
| Checks | `Tests/checks/*` | ~50 pure checks compiled with swiftc; harnesses in `Scripts/` (incl. `sillclient.py`, a Python TLS test client). |

Host pipeline (`StreamCoordinator.swift`, `HEVCEncoder.swift`, `StreamServer.swift`):

```
SCStream (one source; 420f; cursor off; repaint-only frames)
  → EncoderMailbox (1 slot, newest wins)
  → VTCompressionSession (HEVC Main, real-time, no reordering, CBR-ish, keyframe on demand)
  → StreamServer broadcast: per client ≤2 sends in flight, else drop delta → wait for keyframe
      home door (TLS, Bonjour, dynamic port) ─┐
      remote door (TLS, :7455) ───────────────┴─► devices
Catalog (2 s poll): window list, thumbnails (JPEG), icons (PNG), app list
Input: kind 8 → CaptureScale.InputLanding (frame fraction → screen points) → CGEvent
```

Design rules worth keeping on Windows: latency over quality (drop, don't queue); one source
streamed to all devices; every hostile-input cap enforced at read time; policy as pure, separately
tested types (DoorPolicy, DeviceGate, LockPolicy, ClipboardPolicy, PairingWindow…).

## 2. Proposed Windows host architecture

One process, `sill-host.exe`, in the interactive user session, per-monitor-v2 DPI aware.
Portable crates hold everything that is not a Windows API call, so they are unit-tested on macOS
and Windows alike, against vectors generated from the Swift reference.

```
crates/sill-wire      ← framing, kinds, payload types (serde), caps            [portable]
crates/sill-pairing   ← code, proofs, link, tag, MacID, SPKI, cert, signed info [portable]
crates/sill-policy    ← door/gate/origin/pairing-window/lock/clipboard rules     [portable]
crates/sill-net       ← tokio listeners, rustls config, session tasks, pacing   [portable core + Windows bits]
crates/sill-capture   ← WGC (window, monitor), DDA fallback, D3D11 NV12          [Windows]
crates/sill-encode    ← MF HEVC hardware MFT, AU repack, keyframe control, probe [Windows]
crates/sill-input     ← SendInput, HID→scan code, modifier policy               [Windows]
crates/sill-catalog   ← EnumWindows, thumbnails, icons, apps, window commands    [Windows]
crates/sill-store     ← identity, paired devices, settings (DPAPI on Windows)    [portable + Windows bits]
crates/sill-platform  ← DPAPI, DnsServiceRegister (cfg(windows); stand-ins elsewhere) [Windows]
host/                 ← coordinator, tray, settings window, CLI flags            [Windows]
```

### Threads and hand-offs

| Thread | Owns | Hands to |
|---|---|---|
| Capture (WGC `FrameArrived` on a free-threaded frame pool) | D3D11 BGRA texture → NV12 texture (VideoProcessor, GPU) | 1-slot mailbox (newest wins) |
| Encoder | MF async MFT, DXGI device manager (same D3D11 device: zero copy) | encoded AU (`Arc<[u8]>`) to fan-out |
| Network (tokio, 2 workers) | listeners, TLS, per-client writer with ≤2 in-flight frames | — |
| Catalog | 2 s window poll, thumbnails at 2/6 s cadence | catalog messages to fan-out |
| Coordinator (main/UI thread) | source, settings, lock state, tray | commands to the above |

GPU→CPU copies: none on the video path (encoder outputs bytes from GPU input). Thumbnails take one
small staging copy every 2–6 s per window.

### Instrumentation (from day one)

Per-second log line, local only: capture interval, BGRA→NV12 ms, encode ms (submit→output), AU
bytes, fps out, mailbox drops, per-client drops/in-flight, process CPU %, working set, GPU engine
utilisation (PDH `\GPU Engine(*)\Utilization Percentage`). The device's own kind 12 adds frame age
(host encode → device receive), RTT and device fps. A `--stats-csv` flag writes the same to a file
for long runs. No telemetry leaves the machine.

### Settings and storage

`%LOCALAPPDATA%\Sill\`: `identity.bin` (DPAPI: P-256 private key), `devices.json.dpapi` (paired
devices: fingerprint, name, model, pairedAt, method), `recognition.bin` (DPAPI), `settings.json`
(bind addresses, ports, require pairing, quality, start at logon, Cmd mapping), `logs\`.

### Network configuration

- Home door: dynamic port + Bonjour; bound to chosen interfaces (default: all private LAN
  interfaces + Tailscale).
- Remote door: 7455; default bound to the Tailscale interface (100.64.0.0/10, fd7a:115c:a1e0::/48)
  and LAN; **never** to a public address unless the user turns on internet access.
- Firewall rule: Private profile + Tailscale adapter only.

## 3. Crate justification (third-party)

| Crate | Why |
|---|---|
| `windows` | Microsoft's official bindings for every Win32/WinRT API used. |
| `rustls`, `tokio-rustls`, `ring` | TLS 1.3 with ALPN-dependent client-cert policy (Acceptor API); no OpenSSL DLLs to ship. |
| `rcgen` | Build the self-signed P-256 certificate each launch. |
| `p256`, `ecdsa`, `sha2`, `hmac`, `pbkdf2` | Pairing proofs, fingerprints, signed host info (pure Rust, auditable). |
| `serde`, `serde_json` | JSON payloads in the exact Swift shapes. |
| `tokio` | Many TLS connections with timeouts; network threads only, never the video path. |
| `tracing` | Local log files and the Log window. |
| `base64` | Standard base64 (`Data` fields, kind 18) and base64url, configured to read exactly what Foundation reads. |
| `unicode-segmentation`, `unicode-general-category` | `SafeText.label` counts grapheme clusters and drops control/format/private-use/unassigned characters, as Swift's `Character` and `Unicode.Scalar.Properties` do; std has neither. |

In use since Phase 1 task A (portable crates): `serde`, `serde_json`, `base64`, `sha2`, `hmac`,
`pbkdf2`, `p256` (with `ecdsa`), `unicode-segmentation`, `unicode-general-category`. All MIT and/or
Apache-2.0, except one transitive crate: `subtle` (BSD-3-Clause, permissive), which every RustCrypto
MAC and curve crate uses for constant-time comparison; it cannot be dropped without dropping `hmac`. Accepted by Kamal on 2026-11-27.
`unicode-ident` (MIT/Apache-2.0 plus Unicode-3.0 data licence) is a build-time dependency of serde's
derive only.

Added in Phase 1 task B (`sill-platform`, `sill-store`, `sill-net`, `host/`):

| Crate | Licence | Why |
|---|---|---|
| `tokio` (+ `mio`, `bytes`, `socket2`…) | MIT | Listeners, TLS sessions with deadlines, timers; network threads only. |
| `rustls` (ring provider, TLS 1.3 only) | Apache-2.0 OR ISC OR MIT | The `Acceptor`/`LazyConfigAcceptor` API reads the ClientHello's ALPN before choosing the client-cert policy; a custom `ClientCertVerifier` does pin-only trust and sends `certificate_unknown` (the Mac host's alert; see PROTOCOL.md §2). |
| `tokio-rustls` | MIT/Apache-2.0 | rustls on tokio streams; `into_fallible` hands the socket back after a refusal so it closes without a reset. |
| `rcgen` | MIT/Apache-2.0 | Builds the self-signed P-256 certificate at every launch from the stored key. |
| `socket2` | MIT/Apache-2.0 | TCP keepalive 5 s / 2 s / 3 probes (time, interval and count on Windows and macOS), IPV6_V6ONLY. |
| `if-addrs` | MIT/Apache-2.0 | Lists this machine's addresses for the default bind (loopback, LAN, Tailscale; never public). |
| `getrandom` | MIT/Apache-2.0 | The OS CSPRNG (BCryptGenRandom / getentropy) for keys, codes, secrets, tags. Already in the tree via `rand_core`. |
| `windows` (0.62, Windows only) | MIT/Apache-2.0 | DPAPI (`CryptProtectData`) and Bonjour (`DnsServiceRegister`). |
| `tracing`, `tracing-subscriber` | MIT | Console and `logs\host.log` lines, level by `--log-level`. |

Licences outside MIT/Apache/BSD, all from the ring provider the task named, **accepted by Kamal on 2026-11-27**:
`ring` (Apache-2.0 AND ISC), `rustls-webpki` (ISC) and `untrusted` (ISC). ISC is a permissive
licence equivalent to MIT (keep the notice). The alternative provider, `aws-lc-rs`, is also
Apache-2.0 AND ISC (and needs CMake/NASM on Windows); a pure-RustCrypto provider is not mature.
`webpki` is used only for the TLS 1.3 signature check over a raw SPKI; no chain is ever evaluated.

Added in Phase 2 (tray, control pipe, the Sill app):

| Component | Licence | Why |
|---|---|---|
| `qrcode` (0.14, no default features) | MIT/Apache-2.0 | The pairing QR's modules, sent to the app as rows over the control pipe (the app draws them; no image crate). |
| Windows App SDK 1.6 (WinUI 3) | MIT | The app's window, tray flyout, Mica/acrylic and Fluent controls (ADR-7). Bundled self-contained; nothing to install. |
| .NET 8 runtime | MIT | The app's runtime, bundled self-contained and trimmed (partial). |
| Microsoft.Windows.SDK.BuildTools | MIT | Build-time only (WinRT projections). |
| WebView2 projection | BSD-3-Clause | Pulled in by the Windows App SDK; unused by the app, trimmed where the linker can. |

The app adds no other NuGet package: JSON is System.Text.Json (source-generated), the pipe is
`System.IO.Pipes`.

## 4. Architecture decisions (ADR log)

| # | Date | Decision | Status |
|---|---|---|---|
| ADR-1 | 2026-11-27 | Rust + windows-rs for the host | Accepted |
| ADR-2 | 2026-11-27 | Fork upstream, add `windows/`, leave upstream files untouched | Accepted |
| ADR-3 | 2026-11-27 | Wire compatibility with the unmodified client is the hard constraint; no new protocol | Accepted |
| ADR-4 | 2026-11-27 | WGC + D3D11 + MF hardware HEVC; no software encoder in MVP | Accepted |
| ADR-5 | 2026-11-27 | User-session process by default; SYSTEM helper only for secure-desktop features, opt-in | Accepted |
| ADR-6 | 2026-11-27 | Golden vectors generated from upstream Swift are the conformance oracle | Accepted |
| ADR-7 | 2026-11-27 | Tray flyout and settings window as a separate small C#/WinUI 3 app (Mica, Fluent), started on demand from the tray; the Rust host stays the engine; they talk over a local named pipe. Modern look outweighs ~60–80 MB on disk, paid only while the window is open | Accepted (Kamal) |
| ADR-8 | 2026-11-27 | From Phase 2 the Windows test session on Kamal's laptop is the only code writer for `windows/`; the Mac session coordinates and owns the iOS client | Accepted (Kamal) |
