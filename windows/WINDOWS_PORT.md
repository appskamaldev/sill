# Sill for Windows — Phase 0 assessment

Status: **Phase 0 complete, awaiting Kamal's approval.** No host code has been written.
Audited upstream: `github.com/Saffsanity/sill` at `22ab5a3e` (2026-11-24, 1,754 commits,
Apache-2.0). Paths below are upstream paths unless they start with `windows/`.

Companion documents: [ARCHITECTURE.md](ARCHITECTURE.md) (original and proposed architecture),
[PROTOCOL.md](PROTOCOL.md) (the wire, as implemented), [COMPATIBILITY.md](COMPATIBILITY.md)
(unmodified-client feasibility, feature parity matrix), [ROADMAP.md](ROADMAP.md),
[SECURITY.md](SECURITY.md), [TESTING.md](TESTING.md), [BUILD.md](BUILD.md).

---

## 1. Verdict in one paragraph

A Windows host that the **unmodified App Store Sill client** connects to, pairs with, streams from
and controls is **technically feasible**. Nothing on the wire is Apple-only: the protocol is TCP +
TLS 1.3 with mutually pinned self-signed P-256 certificates, a 14-byte binary header, JSON control
messages, HEVC Main 8-bit video and (optionally) AAC-ELD audio. Every optional feature (menus,
gestures, clipboard, audio, cursor shape, settings, signed host info) is capability-gated and the
client degrades cleanly without it. Two things cannot be fixed host-side: the client calls every
host a **"Mac"** (≈98 UI strings, no OS field on the wire), and audio is **AAC-ELD only**, which
Windows does not encode natively. Neither blocks the MVP.

## 2. Assumptions in the brief that the source code contradicts

| Brief says / implies | What the code actually does | Consequence |
|---|---|---|
| Sill is a remote-desktop app; desktop streaming is primary | It is a **per-window** streamer first (`docs/BRIEF.md`); Desktop is a secondary source (`StreamSource.desktop`) | Window streaming (Phase 3) is the real parity target. Desktop-first MVP is still right because it is simpler and the client auto-selects Desktop when `active` is `none` (`StreamClient.swift:4561-4595`). |
| "Device discovery" over Tailscale | Bonjour `_sill._tcp` only; mDNS does not cross Tailscale | Over Tailscale the path is **typed address + 12-digit code** (`AddMacCard.swift:116-173`) to the remote door, port 7455. That works with the unmodified client. |
| "Adaptive streaming quality / adaptive bitrate" | **None in the Mac host.** Bitrate is fixed per session; `LinkJudge` only *reports* a suggestion in kind 16 `link` | Windows host can match (fixed) or exceed (adaptive) without protocol change. Adaptive is a Windows-side improvement, not parity. |
| "Unattended access" | No unattended login. Mac host streams the **lock screen** and lets a *paired* device type the password (`LockPolicy.swift`); off-console = placeholder only | On Windows the lock/login screen is the Winlogon **secure desktop**, uncapturable from a user process. Parity needs a SYSTEM service (Phase 4, risk R6). |
| "Virtual display support" | Optional, **off by default** (`HostConfig.swift:137`), private `CGVirtualDisplay` API | Windows needs an Indirect Display Driver, which needs driver signing. Deferred; host reports `virtualDisplayAvailable:false`. |
| "Clipboard sync where supported" | Text only (generation 1), opt-in, ≤256 KiB | Easy parity. |
| "Preserve encryption" — implies TLS might be optional | TLS 1.3 is **mandatory**: a release client never dials a plain door (`DiscoveryPolicy.swift:1081-1092`) | The plain-TCP dev door is useless for the App Store client. TLS is Phase 1 work, not hardening. |
| "Configurable authentication" | Fixed design: pairing by QR/typed code/ask, then key pinning; "Require pairing" on/off at the home door | Windows host offers the same switches. No passwords, no accounts. |
| Remote access needs nothing external | True: no relay, no cloud; `RouterAddress` only *reads* the router's external IP | Match exactly. |
| One iOS app for Mac and Windows "retaining the existing UX" | True for function; the copy says "Mac" | Accept for MVP. Optional later client change (C1 in COMPATIBILITY.md). |

## 3. Feasibility of an unmodified client — summary

Mandatory for a working session (all verified in source, none tested against hardware yet):

1. **TLS 1.3 server**, ALPNs `sill/1` and `sill-pair/1`, P-256 ECDSA certificate, client
   certificate *required*, trust = SHA-256(SPKI) pin only, no tickets/resumption
   (`Sources/StreamProtocol/RemoteTLS.swift:53-81`).
2. **Refuse an unpaired key with a TLS alert inside the handshake** (bad_certificate → device
   sees -9825), never a bare close; the client maps -9825/-9829 to "Not paired" and stops
   redialing (`iOSClient/DiscoveryPolicy.swift:1303-1324`, `RemoteDialPolicy.swift:234-242`).
3. **Pairing** on `sill-pair/1`: one kind 19 → one kind 20, PBKDF2-HMAC-SHA256 (600,000 rounds)
   code key, HMAC proofs bound to both TLS fingerprints (`Pairing.swift:70-136`).
4. **Bonjour** `_sill._tcp` with TXT `p` (and `r`) for LAN discovery; *not* needed for Tailscale.
5. **Session**: read hello (kind 23); send window list (kind 2) within 10 s; echo pings (10→11);
   send ticks (13); parameter sets (0) then keyframe (1), HEVC length-prefixed (4-byte) access units.
6. **Input**: kind 8 JSON in Swift's synthesized-enum shape, positions as 0…1 of the frame, keys
   as USB HID usages with UIKeyModifierFlags bits.

Full analysis and the feature-by-feature matrix: [COMPATIBILITY.md](COMPATIBILITY.md).

## 4. Technology stack recommendation

### Language: **Rust** (stable, MSVC target) with `windows-rs`

Weighed against what Sill actually does, not in the abstract:

| Criterion | C++20 / C++/WinRT | **Rust + windows-rs** | C# / .NET 8 |
|---|---|---|---|
| WGC, D3D11, Media Foundation, SendInput, WASAPI, DNS-SD | Native, best samples (OBS, Sunshine) | Full coverage, Microsoft-maintained bindings; same COM calls | WGC/D3D via CsWinRT/Vortice ok; MF interop awkward |
| Hot path (capture → encode → send) | Zero-cost | Zero-cost; GPU textures never touch Rust memory | GC and marshalling on a 120 fps path; workable but needs care |
| Parsing hostile network input (the Mac host invests heavily here: caps, `SafeText`, `ViewportBounds`, DoorPolicy) | Memory-unsafe by default | Memory-safe by default | Memory-safe |
| TLS 1.3 + ALPN-dependent client-cert policy | OpenSSL or Schannel (Schannel client-cert self-signed policy is fiddly) | rustls: `Acceptor` reads ClientHello ALPN, then picks a per-ALPN config with a custom client verifier → in-handshake refusal like the Mac | SslStream (Schannel underneath); custom client-cert callback ok, ALPN-dependent refusal awkward |
| Idle footprint | ~5–15 MB | ~5–15 MB | ~40–80 MB |
| Protocol tests runnable on Kamal's Mac | Possible, more CMake work | `cargo test` on macOS for every pure crate, unchanged | Yes |
| Build complexity | CMake + vcpkg | `cargo` + MSVC build tools | dotnet SDK |
| Tray UI | Win32 | Win32 via windows-rs (Shell_NotifyIcon, dialogs) | WinForms/WPF easiest |

Decision drivers: the host is a long-running, network-facing process that parses untrusted
bytes, with a hardware-bound hot path that is COM calls on GPU handles. Rust gives C++-level
performance and Windows API reach with memory safety on the attack surface, and lets the
portable crates (protocol, pairing, policy) be tested on macOS beside the Swift reference.
C++ would be the choice if we needed to embed vendor SDK sample code heavily; we don't for MVP.
C# loses on footprint and hot-path predictability.

### Windows technologies, validated against the Mac host

| Function | Mac host uses | Windows choice | Notes / why |
|---|---|---|---|
| Window capture | ScreenCaptureKit per window, repaint-only frames, cursor off | **Windows Graphics Capture** (`GraphicsCaptureItem.CreateForWindow`), `IsCursorCaptureEnabled=false`, `IsBorderRequired=false` (Win11) | WGC also delivers frames on change only; captures occluded windows (better than SCK). Minimized windows produce no frames (same as Mac). |
| Desktop capture | SCK display filter | **WGC for monitor** (`CreateForMonitor`); DXGI Desktop Duplication as fallback | WGC has one API for both sources; DDA kept for Windows 10 1803- and for dirty-rect metrics. |
| GPU processing | CVPixelBuffer 420f | **Direct3D 11** (BGRA→NV12 via `ID3D11VideoProcessor`) | D3D11 is what WGC and MF hardware MFTs consume; D3D12 adds nothing here. |
| Video encode | VideoToolbox HEVC Main, real-time, no reordering, keyframe on demand | **Media Foundation hardware HEVC MFT** (async, `CODECAPI_AVLowLatencyMode`, B-frames 0, `CODECAPI_AVEncVideoForceKeyFrame`, CBR) via DXGI device manager | Covers NVIDIA/AMD/Intel uniformly. Direct NVENC/AMF/QSV SDKs only if MF latency or control falls short (measured, Phase 5). |
| Audio capture | SCK audio, app or all, 48 kHz stereo | **WASAPI loopback**; per-app via process-loopback activation (Win10 2004+) | — |
| Audio encode | AudioToolbox AAC-ELD, 480/512 frames | **Open: no native AAC-ELD.** Options in §7 R5 | Phase 4. |
| Window enumeration | CGWindowList + filters (`CatalogWindows.swift`) | `EnumWindows` + visibility/cloaked (`DWMWA_CLOAKED`)/tool-window/owner filters | Window id = HWND truncated to u32 (HWNDs are 32-bit-significant on x64). |
| Window resize | Accessibility kAXSize/Position | `SetWindowPos` (DPI-aware, per-monitor v2) | UIPI blocks elevated windows unless the host is elevated. |
| Window commands | AX close/minimize/fullscreen | `WM_SYSCOMMAND` SC_CLOSE / SC_MINIMIZE / SC_MAXIMIZE | "fullScreen" maps to maximize toggle. |
| Input | CGEvent | **SendInput** (absolute virtual-desk mouse, wheel/hwheel, scan codes, `KEYEVENTF_UNICODE`) | Cmd→Ctrl mapping host-side (see PROTOCOL.md §Input). |
| Menus | AX menu bar | UI Automation (MenuBar/MenuItem) — **later**; not sending kind 24 hides the button | Phase 4+, optional. |
| Virtual display | private CGVirtualDisplay | Indirect Display Driver (IddCx) | Needs signed driver; deferred. |
| Discovery | Bonjour (`NWListener.Service`) | **`DnsServiceRegister`** (windns.h, Win10 1809+) | No Bonjour SDK install. Fallback: pure-Rust mDNS responder if Windows' responder misbehaves. |
| Identity storage | Keychain | **DPAPI** (`CryptProtectData`, per user) files in `%LOCALAPPDATA%\Sill` | Same items as the Keychain: host key, recognition key, paired devices, require-pairing. |
| Clipboard | NSPasteboard polling | `AddClipboardFormatListener` + custom registered format as own-write marker | — |
| Lock / session | SCDynamicStore, session notifications | `WTSRegisterSessionNotification`, `WTSGetActiveConsoleSessionId` | — |
| Background / startup | Login item, menu bar app | **Per-user process started at logon** (Run key or Task Scheduler) with tray icon; SYSTEM service only for Phase 4 unattended/secure-desktop | A service in session 0 cannot capture or inject into the user desktop; a user-session process is the correct default. |
| Firewall | `socketfilterfw` | Windows Defender Firewall rule via `INetFwPolicy2`, scoped to Private profile + optional Tailscale interface | Installer creates it (needs elevation once). |

### Third-party crates (all MIT/Apache-2.0, compatible with upstream's Apache-2.0)

`windows` (Microsoft), `rustls` + `ring` provider, `rcgen` (certificate), `p256`/`ecdsa`, `sha2`,
`hmac`, `pbkdf2`, `serde`/`serde_json`, `tokio` + `tokio-rustls` (network I/O only), `tracing`
(local logs only). No telemetry, no network services. Upstream's "Apple frameworks only" rule
cannot apply to Windows; each added crate needs a one-line justification in ARCHITECTURE.md.

## 5. Repository structure

Recommendation: **fork `Saffsanity/sill` to Kamal's GitHub** (history and attribution kept) and
add a self-contained `windows/` tree. Upstream files stay untouched except where a client change
is approved later. This lets us track upstream protocol changes with `git merge upstream/main`.

```
sill/                       (fork; upstream untouched)
├── Sources/StreamProtocol/ (authoritative wire spec — read, never edited by the port)
├── iOSClient/              (unmodified for MVP)
└── windows/
    ├── *.md                (these documents)
    ├── Cargo.toml          (workspace)
    ├── crates/
    │   ├── sill-wire/      header, kinds, JSON payload types, ParameterSets, ImageBlob, caps  [portable]
    │   ├── sill-pairing/   PairingCode, PairingProof, PairLink, RecognitionTag, MacID, SPKI, signed info  [portable]
    │   ├── sill-policy/    DoorPolicy, DeviceGate, OriginPolicy, PairingWindow, LockPolicy… ports  [portable]
    │   ├── sill-net/       listeners, rustls config, session actor, pacing, Bonjour  [Windows + portable core]
    │   ├── sill-capture/   WGC / DDA, D3D11 colour conversion  [Windows]
    │   ├── sill-encode/    MF HEVC MFT, Annex-B → length-prefixed, keyframe control  [Windows]
    │   ├── sill-input/     SendInput, HID→scan-code map, modifier policy  [Windows]
    │   ├── sill-catalog/   window enumeration, thumbnails, icons, window commands  [Windows]
    │   └── sill-store/     DPAPI-protected identity and paired devices  [Windows]
    ├── host/               sill-host.exe: tray app + CLI flags  [Windows]
    ├── vectors/            golden wire vectors generated from the Swift StreamProtocol
    ├── tools/              vector generator (Swift, runs on the Mac), test client wrappers
    └── installer/          (Phase 5)
```

Until the fork exists, this repository (`~/Projects/windows-app`) holds `windows/` only. Creating
the fork needs Kamal's OK (it leaves this Mac).

## 6. Proposed architecture (summary)

Details and diagrams: [ARCHITECTURE.md](ARCHITECTURE.md).

```
           ┌──────────── sill-host.exe (user session, per-monitor-v2 DPI aware) ─────────────┐
 Tray UI ──┤ Coordinator: one active source, client set, settings, lock state              │
           │   │                                                                           │
           │   ├─ Capture thread: WGC FramePool (D3D11 BGRA) ─► VideoProcessor NV12 ─┐     │
           │   │                                                  1-slot mailbox ◄──┘     │
           │   ├─ Encode thread: MF HEVC MFT (async, GPU input) ─► AU (Annex-B→len4)      │
           │   ├─ Fan-out: per-client queue, ≤2 in flight, drop deltas → keyframe        │
           │   ├─ Catalog thread: EnumWindows 2 s, thumbnails, icons                      │
           │   └─ Input: kind 8 → SendInput (coordinator's source rect mapping)            │
           │ Network (tokio): home door :dyn (Bonjour) + remote door :7455, rustls TLS 1.3 │
           └───────────────────────────────────────────────────────────────────────────────┘
```

Same threading rule as upstream: capture → encoder → network, explicit hand-offs, drop frames
rather than queue them.

## 7. Technical risks and mitigations

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | rustls cannot produce exactly the alert the client maps to "Not paired" (-9825/-9829) | Medium / High (wrong alert → endless redial or wrong UI) | Choose the verifier error that emits `bad_certificate` (42) or `certificate_unknown` (46); verify with Python `ssl` client and then the real device in Phase 1. Fallback: OpenSSL via `openssl` crate with verify callback. |
| R2 | Swift JSON shape mismatch (enum `_0` keys, Data as base64, optional omission, Double formatting) | Medium / High | Golden vectors generated by compiling upstream `StreamProtocol` on the Mac; Rust tests round-trip every vector. |
| R3 | MF HEVC MFT latency or keyframe control worse than VideoToolbox (some MFTs buffer frames, ignore force-keyframe, insert AUD/SEI) | Medium / High | Encoder probe + self-test at launch (as `EncoderProbe.swift`); measure per vendor; NVENC/AMF/QSV direct path as Phase 5 fallback. |
| R4 | No hardware HEVC encoder (VMs, old GPUs, many servers); no licence-clean software HEVC encoder (x265 is GPL) | High on servers / Medium | MVP requires hardware HEVC and says so clearly. Software fallback decision deferred (GPL x265 as a separate optional download, or none). |
| R5 | Audio: client plays only AAC-ELD; Windows has no AAC-ELD encoder; FDK-AAC licence is not OSI-approved and carries patent caveats | High / Medium | Phase 4 decision: (a) FDK-AAC as optional component after licence review, or (b) small backward-compatible client change adding `"opus"` to `Hello.audio` (iOS decodes Opus via AudioToolbox) — the host then sends Opus only to clients that list it. |
| R6 | Locked/login screen is the secure desktop; a user-session process cannot capture it or inject into it | Certain / Medium | MVP: report `locked:true`, send placeholder, refuse input (exactly the Mac's off-console behaviour). Phase 4: SYSTEM service that launches a helper on the Winlogon desktop (the approach Sunshine uses), behind an explicit opt-in. No UAC bypass. |
| R7 | UIPI: input and window control cannot reach elevated windows; UAC prompts appear on the secure desktop | Certain / Low-Medium | Document; optional "run elevated" mode; never auto-elevate, never disable UAC. |
| R8 | WGC yellow border / capture consent; borderless needs Windows 11 | Low | `IsBorderRequired=false` on Win11; Windows 10 shows the border (documented). |
| R9 | `DnsServiceRegister` quirks (TXT encoding, multiple interfaces, firewall for UDP 5353) | Medium / Low | Only affects LAN discovery; Tailscale path doesn't use it. Fallback pure-Rust responder. |
| R10 | Client says "Mac" everywhere | Certain / Cosmetic | Accept; optional additive client change later (`hostPlatform` field). |
| R11 | Upstream protocol evolves (floor rules are additive) | Medium / Low | Track upstream; vectors regenerated per merge; unknown kinds skipped as upstream does. |
| R12 | Away quality: Tailscale connections land on the remote door, where the Mac's policy applies 4 Mbps/1x | Certain / Medium for Kamal | Host-side setting: treat VPN/Tailscale origins as home quality (host policy, no wire change). |
| R13 | DPI: window points vs pixels, per-monitor scale, mixed-DPI monitors | Medium | Per-monitor-v2 awareness; WindowInfo `width/height` in DIPs (the Mac's "points"); input mapped in physical pixels of the source rect. |
| R14 | Clock skew distorts `frameAgeMs` (header timestamp is wall clock) | Low | Instrumentation note only; Windows Time service. |

## 8. First working MVP — precise definition

**Goal:** Kamal controls a Windows 11 PC's primary display from his iPhone Duo using the
**unmodified App Store Sill client**, over Tailscale and over LAN.

In scope:
- `sill-host.exe` (console for MVP; tray in Phase 2 exit), Windows 11 x64, GPU with hardware HEVC.
- Persistent P-256 identity and paired-device list (DPAPI).
- Remote door TCP 7455 and home door (dynamic port + Bonjour `_sill._tcp`, `p=1`, `r`), both
  TLS 1.3 per §3; bind address configurable; default bind = LAN + Tailscale interfaces, never a
  public interface unless enabled.
- Pairing by typed code (code shown in the host console/window, 5-minute window, 5 tries, back-off
  as `PairingWindow.swift`). QR is Phase 2 exit (needs a window to show it).
- Session: hello, gate (floor "0"), window list with `windows: []` and `active` desktop,
  `hostVersion`, `protocol:1`, `launchID`; hostSettings minimal (fixed values, `persistent:false`);
  ping/pong, tick, goodbye on quit; up to 8 sessions.
- Video: primary monitor via WGC → NV12 → MF hardware HEVC Main, CBR 15 Mbps (home) / settings
  bitrate, fps = clamp(max viewport fps, 24…60), keyframe on join, on kind 44, on source change;
  per-client drop-to-keyframe.
- Input: pointer move/left/right down/up, scroll (wheel), text (Unicode), keys (HID → scan code,
  Cmd→Ctrl for shortcuts, Option→Alt).
- Logging: per-second stats line (capture ms, encode ms, fps, drops, bytes) plus the device's own
  ClientStats (kind 12: frame age, RTT, fps), so latency is measured from day one.

Out of MVP: window list/thumbnails/icons, window streaming, audio, clipboard, menus, gestures,
cursor shape (client draws its default arrow), multi-monitor, lock-screen streaming, installer.

**Acceptance (all on real hardware, by Kamal with exact steps in TESTING.md):**
1. Duo, App Store build: Add a Mac → type `100.x.y.z` (Tailscale) and the code → paired.
2. Desktop picture appears within 2 s of connect; moving the trackpad moves the Windows pointer;
   tap clicks; keyboard types into Notepad; Cmd+C/Cmd+V copy-paste inside Windows.
3. Kill the Wi-Fi on the phone for 10 s → device reconnects by itself, picture back within 5 s.
4. Unpair the device on the host → device shows "Not paired" and does not redial (R1).
5. Same on LAN via Bonjour (no address typed).
6. Measured: LAN median frame age ≤ 20 ms, RTT ≤ 15 ms, 60 fps at 1080p at ≤ 15 Mbps; host CPU
   ≤ 10 % of one core while streaming a static desktop; idle (no client) ≈ 0 % CPU, < 40 MB.
7. Golden-vector and pairing-vector tests pass on Windows and macOS.

## 9. What needs Kamal

1. **Approve the approach** (Rust, fork layout, MVP scope).
2. OK to **fork Saffsanity/sill to your GitHub** and push (`windows/` added there).
3. Windows test machine details: GPU model, Windows 11 build, whether Tailscale is installed.
4. Decision on audio direction (R5) — can wait until Phase 4.
5. Whether a small, backward-compatible iOS client change is acceptable later (label "PC"
   instead of "Mac"; Opus audio). Not needed for MVP.
