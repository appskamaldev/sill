#!/bin/bash
# TrackpadTaps (iOSClient/TrackpadTaps.swift) on its own: the trackpad's clicks and its drag. A tap is 3 pt of
# travel at most and lifts within 0.35 s; one finger the left button, two the right; tap, then touch again within
# 0.3 s of the lift, and the stroke drags with the left button down until it lifts. Moving never presses a
# button. Then 20,000 random strokes against a model.
#   Tests/checks/trackpad-taps/run.sh             compile and run the check
#   Tests/checks/trackpad-taps/run.sh --mutants   one-line mutants of TrackpadTaps.swift; each must fail it
here="$(cd "$(dirname "$0")" && pwd)"
source "$here/../common.sh"
if [ "${1:-}" = "--mutants" ]; then run_mutants python3 "$here/mutants.py"; exit; fi
swiftc -O iOSClient/TrackpadTaps.swift "$here/main.swift" -o "$out/check"
"$out/check"
