# Build

Status: planned; no code yet. Finalised in Phase 1.

## Windows host (Windows 11 x64)
Prerequisites: Visual Studio 2022 Build Tools (MSVC, Windows 11 SDK), Rust stable via rustup
(`x86_64-pc-windows-msvc`), Python 3.11+ (for the protocol test client), Git.
```
git clone <fork> sill && cd sill\windows
cargo build --release
cargo test
target\release\sill-host.exe --help
```

## Portable crates on macOS
```
cd windows && cargo test -p sill-wire -p sill-pairing -p sill-policy
```

## Golden vectors (macOS, needs Xcode's Swift)
```
windows/tools/make-vectors.sh     # compiles ../Sources/StreamProtocol, writes windows/vectors/
```

## iOS client
Unmodified upstream for MVP (App Store build). Building from source follows upstream
`docs/DEVELOPMENT.md`.
