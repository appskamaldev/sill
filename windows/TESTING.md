# Testing

Status: layers 1 and 2 run (Phase 1 task A, 2026-11-27); layers 3–5 are still a plan.

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
3. **Protocol integration (Windows, no device):** upstream `Scripts/sillclient.py` (Python `ssl`,
   TLS 1.3, ALPN) against `sill-host.exe` on loopback: handshake, pairing by code, hello, window
   list, ping/pong, junk kinds skipped, oversized payload closes, unpaired key refused with alert.
4. **Hardware checkpoints (Kamal):** the Duo with the App Store build, LAN and Tailscale.
5. **Regression vs Mac host:** the same device build still works with Sill.app on the Mac after any
   client change.

## Results

| Date | Where | Command | Result |
|---|---|---|---|
| 2026-11-27 | macOS (arm64) | `cargo test --workspace` | 33 tests passed, 0 failed |
| 2026-11-27 | CI windows-latest, macos-latest | `.github/workflows/windows.yml` (test; clippy and fmt on Windows) | 33 passed, 0 failed on each; clippy and fmt clean: https://github.com/appskamaldev/sill/actions/runs/46020622253 |

## Checkpoint 1 procedure (draft, finalised in Phase 1)
On the Windows PC (PowerShell):
```
tailscale ip -4                       # note 100.x.y.z
.\sill-host.exe --pair                # prints the 12-digit code, opens a 5-minute window
```
On the Duo: Sill → Add a Mac → "Type address and code" → `100.x.y.z` → code → Pair.
Expected: "Paired", then a test picture within 2 s; host log shows `hello appVersion=…`,
`session ready`, pings every 0.25 s.
Then: phone in flight mode 10 s → off: reconnects alone ≤ 5 s.
Then: `.\sill-host.exe --unpair <name>` → phone shows "Not paired", no redial (host log shows no
further handshakes from it).

Diagnostics: `Get-NetTCPConnection -LocalPort 7455`, `Test-NetConnection 100.x.y.z -Port 7455`
(from another machine), `%LOCALAPPDATA%\Sill\logs\host.log`, `tailscale status`.

## Performance measurements
From the host stats line and kind 12: frame age median/max, RTT median/max, fps, drops, encode ms,
CPU %, working set, GPU utilisation. Targets in WINDOWS_PORT.md §8.
