# Testing

Status: plan only. Nothing has run yet.

## Layers
1. **Conformance vectors (macOS + Windows, CI):** Swift tool compiles upstream `StreamProtocol`
   and emits vectors; `cargo test -p sill-wire -p sill-pairing -p sill-policy` must match.
2. **Unit tests** for ported policies (door, gate, pairing window, lock, clipboard), with the cases
   upstream's `Tests/checks/*` already encode.
3. **Protocol integration (Windows, no device):** upstream `Scripts/sillclient.py` (Python `ssl`,
   TLS 1.3, ALPN) against `sill-host.exe` on loopback: handshake, pairing by code, hello, window
   list, ping/pong, junk kinds skipped, oversized payload closes, unpaired key refused with alert.
4. **Hardware checkpoints (Kamal):** the Duo with the App Store build, LAN and Tailscale.
5. **Regression vs Mac host:** the same device build still works with Sill.app on the Mac after any
   client change.

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
