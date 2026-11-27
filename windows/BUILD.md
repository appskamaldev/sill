# Build

Status: Phase 1 task A. The portable crates (`sill-wire`, `sill-pairing`, `sill-policy`) build and
test on macOS and Windows; there is no `sill-host.exe` yet.

## Layout

```
windows/
├── Cargo.toml            workspace (edition 2021); Cargo.lock is committed, CI builds --locked
├── crates/sill-wire      header, kinds, caps, frame reader, blobs, JSON payloads, SillVersion
├── crates/sill-pairing   code, proofs, tag, door TXT, SPKI/MacID, link, addresses, SafeText, kind 18
├── crates/sill-policy    door trust, pairing window, device gate, viewport bounds
├── vectors/*.json        golden vectors from upstream Swift (generated, committed)
└── tools/                make-vectors.sh and the Swift generator package (tools/vectors)
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
