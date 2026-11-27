# Roadmap

Each phase ends with a **hardware checkpoint** by Kamal (exact steps in TESTING.md). A build that
compiles is not a pass; interoperability is proven only on the Duo.

## Phase 0 — Feasibility ✅ (this assessment, 2026-11-27)
Audit, protocol doc, feasibility, stack, risks, structure, MVP definition. **Awaiting approval.**

## Phase 1 — Protocol interoperability (no video yet)
Priority order:
1. Fork + `windows/` workspace; CI on GitHub Windows runner (`cargo test`, clippy) and macOS (portable crates).
2. **Golden vectors**: a Swift tool compiles upstream `StreamProtocol` and writes JSON/binary
   vectors (every kind's payload, enum shapes, ParameterSets, ImageBlob, pairing code/proof/tag,
   SPKI fingerprint, MacID, PairLink). Rust tests must match byte for byte.
3. `sill-wire`, `sill-pairing`, `sill-policy` (DoorPolicy, PairingWindow, DeviceGate) with tests.
4. `sill-net`: rustls server (ALPN Acceptor, per-ALPN verifier, refusal alert per R1), remote
   door 7455, home door + `DnsServiceRegister`, session: hello → window list (empty) → host
   settings → ping/pong → tick → goodbye. A **synthetic** encoder (a fixed pre-encoded HEVC test
   clip) so the device shows a picture without capture.
5. `sill-store` (DPAPI) and console pairing code.
6. **Checkpoint 1**: upstream `Scripts/sillclient.py` passes against the Windows host; then the Duo
   pairs by typed Tailscale address, connects, shows the test clip, survives reconnect, and shows
   "Not paired" after removal.

## Phase 2 — Minimum viable streaming (the MVP)
1. WGC primary-monitor capture → D3D11 NV12 → MF HEVC hardware MFT; encoder probe + self-test.
2. Fan-out with drop-to-keyframe; keyframe on join / kind 44 / source change.
3. Input: pointer, scroll, text, keys, modifier policy.
4. Stats line + kind 12 logging.
5. Tray icon (status, addresses, pairing code/QR, connected devices, disconnect, quit), start at
   logon, firewall rule, host settings (kind 16/17), signed host info (kind 18), lock state.
6. **Checkpoint 2 (MVP)**: acceptance list in WINDOWS_PORT.md §8, LAN and Tailscale.

## Phase 3 — Application streaming
Window enumeration + filters, window list/thumbnails/icons/app list/launch, WGC per-window capture,
source switching, viewport fit with restore, DPI handling, window lifecycle (close, minimize,
resize → pipeline restart), window commands, cursor shape + remote pointer, key repeat.
**Checkpoint 3**: stream Notepad, Edge and a UWP app; switch between them from the bar; fold/unfold.

## Phase 4 — Advanced
Multiple monitors (kind 47), clipboard, gestures, audio (after R5 decision), home/away + adaptive
bitrate, automatic reconnection hardening, lock/off-console states, privacy mode, unattended
(SYSTEM helper on secure desktop, opt-in), virtual display investigation (IddCx).

## Phase 5 — Optimisation and distribution
Profiling per vendor (Intel/AMD/NVIDIA), direct NVENC/AMF/QSV if MF falls short, 24 h soak,
security review (fuzz the parsers, TLS refusal paths), MSIX or MSI installer with signed binaries,
documentation, regression against the Mac host (the same device build talks to both), optional
client changes C1/C2 if approved.

## Windows 10 / Server (evaluated separately)
- Windows 10 21H2+: WGC works but shows a capture border; `DnsServiceRegister` ok; process loopback
  audio needs 2004+. Expected to work with the border caveat.
- Windows Server 2022: TLS 1.3 fine; often no GPU → no HEVC hardware → unsupported until a software
  encoder decision. Remote Desktop sessions are not the console; capture of an RDP session is a
  separate investigation.
