#!/bin/sh
# Regenerates windows/vectors/*.json from upstream's Swift StreamProtocol (macOS, Swift 5.9+).
# Run after every upstream merge; commit the result. No xcodebuild, no simulator.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../vectors"
mkdir -p "$out"
swift build -c release --package-path "$here/vectors" 2>&1 | grep -E "error|warning: unre|Compiling|Build complete" | grep -v "^\[" || true
bin="$(swift build -c release --package-path "$here/vectors" --show-bin-path)/make-vectors"
# JSONEncoder's key order follows Swift's per-process hash seed (an enum's "_0" moves between
# runs); fixing the seed keeps the files the same from run to run. Readers never depend on order.
SWIFT_DETERMINISTIC_HASHING=1 "$bin" "$out"
