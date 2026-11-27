# Security

The Windows host keeps upstream's security model exactly (upstream `SECURITY.md`,
`docs/remote-access-plan.md`, `docs/home-pairing-plan.md`). Tailscale is an extra layer, never a
reason to drop one.

## Model
- **Mutual TLS 1.3, pinned keys.** Host and device each hold a P-256 key; trust is SHA-256(SPKI).
  No CAs, no passwords, no accounts, no cloud.
- **Pairing** binds both fingerprints from the live TLS session into HMAC proofs; a relay in the
  middle cannot pair. Typed codes are 12 digits with PBKDF2 (600,000 rounds), a 5-minute window,
  5 tries, exponential back-off.
- **Admission inside the handshake**: unpaired keys are refused with a TLS alert on `sill/1`.
- **Hostile-input caps** at read time (payload sizes per direction, viewport bounds, text limits,
  SafeText for names, clipboard ≤256 KiB, rate limits), ported 1:1 from upstream.
- **Pre-admission limits**: 8 shared handshake slots + 2 for known addresses, 2 per source, 4 s
  handshake deadline, address back-off after 5 failures/60 s.

## Windows-specific rules
- Keys and paired devices encrypted with **DPAPI (current user)**; files ACL'd to the user.
- Default listening: LAN private interfaces + Tailscale only. Internet-facing binding is off by
  default and needs an explicit switch. Firewall rule limited to the Private profile and the
  Tailscale adapter.
- No UAC bypass, no disabling UAC, no driver tricks. Elevated windows stay unreachable unless the
  user runs the host elevated. Secure-desktop features (Phase 4) only via an opt-in SYSTEM helper,
  documented, off by default.
- No telemetry, no update pings, no external services. Logs stay local and never contain keys,
  codes or clipboard text.
- No hardcoded credentials; pairing codes and QR secrets are generated per window from the OS CSPRNG
  (`BCryptGenRandom`).

## Review items (Phase 5)
Fuzz `sill-wire` parsers (cargo-fuzz), test every refusal path with the Python client, verify TLS
config (no resumption, TLS 1.3 only), review firewall/binding defaults, review the SYSTEM helper.
