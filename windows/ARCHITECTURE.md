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
crates/sill-store     ← DPAPI identity, paired devices, settings                 [Windows]
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
MAC and curve crate uses for constant-time comparison; it cannot be dropped without dropping `hmac`.
`unicode-ident` (MIT/Apache-2.0 plus Unicode-3.0 data licence) is a build-time dependency of serde's
derive only.

## 4. Architecture decisions (ADR log)

| # | Date | Decision | Status |
|---|---|---|---|
| ADR-1 | 2026-11-27 | Rust + windows-rs for the host | Proposed |
| ADR-2 | 2026-11-27 | Fork upstream, add `windows/`, leave upstream files untouched | Proposed |
| ADR-3 | 2026-11-27 | Wire compatibility with the unmodified client is the hard constraint; no new protocol | Proposed |
| ADR-4 | 2026-11-27 | WGC + D3D11 + MF hardware HEVC; no software encoder in MVP | Proposed |
| ADR-5 | 2026-11-27 | User-session process by default; SYSTEM helper only for secure-desktop features, opt-in | Proposed |
| ADR-6 | 2026-11-27 | Golden vectors generated from upstream Swift are the conformance oracle | Proposed |
