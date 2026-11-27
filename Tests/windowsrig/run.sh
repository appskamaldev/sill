#!/bin/bash
# windowsrig (TEST ONLY, a fork extension; README.md): Sill Dev on the "Sill-Duo" simulator against a host
# (the Windows host on Kamal's laptop, or a synthetic host on this Mac), with no hands.
#
#   Tests/windowsrig/run.sh pair HOST[:PORT] CODE   pair by the 12-digit code as Add a Mac… does, then connect
#   Tests/windowsrig/run.sh connect [NAME|ADDRESS]  connect to a saved computer (default 100.90.47.26)
#   Tests/windowsrig/run.sh input [NAME|ADDRESS]    connect, then send the Windows input sequence
#   Tests/windowsrig/run.sh shot                    launch plainly, one screenshot
#
# Environment: RIG_NO_BUILD=1 (reuse the last build), RIG_WAIT=<s> (how long to wait for the result;
# default 60, input 90), RIG_KEEP_BOOTED=1 (leave Sill-Duo booted: only for a run straight after).
# Results: .build/windowsrig/results/<time>-<mode>/ (rig.log, console.out.log, console.err.log, *.png, summary.txt).
# It takes the shared Xcode lock and always releases it; it makes Sill-Duo only when it is missing and
# shuts it down at the end. It never touches any other simulator.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
lock="$HOME/.claude/bin/xcode-lock"
label="sill/rig"
name="Sill-Duo"
work="$root/.build/windowsrig"
derived="$work/DerivedData"
logname="sill-rig.log"

usage() { sed -n '5,8p' "$0" | sed 's/^# //' >&2; exit 2; }
mode="${1:-}"; [ -n "$mode" ] || usage; shift
args=()
wait_s="${RIG_WAIT:-60}"
case "$mode" in
    pair)
        [ $# -eq 2 ] || usage
        [[ "$2" =~ ^[0-9]{12}$ ]] || { echo "windowsrig: the code is 12 digits" >&2; exit 2; }
        args=(-SillPairAddress "$1" -SillPairCode "$2") ;;
    connect) args=(-SillAutoConnect "${1:-100.90.47.26}") ;;
    input)
        args=(-SillAutoConnect "${1:-100.90.47.26}" -SillInputScript windows -SillAllowRemoteInputTest 1)
        wait_s="${RIG_WAIT:-90}" ;;
    shot) args=(-SillRigLog 1); wait_s="${RIG_WAIT:-8}" ;;
    *) usage ;;
esac
results="$work/results/$(date +%Y%m%d-%H%M%S)-$mode"
mkdir -p "$results"

udid=""
"$lock" wait "$label" >/dev/null
cleanup() {
    status=$?
    if [ -n "$udid" ] && [ -z "${RIG_KEEP_BOOTED:-}" ]; then
        xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true
        echo "windowsrig: $name shut down"
    fi
    "$lock" release "$label" >/dev/null 2>&1 || true
    exit $status
}
trap cleanup EXIT INT TERM

# Sill-Duo: found by name, made only when it is missing (the iPhone Duo type, an iOS 27 runtime).
udid="$(xcrun simctl list devices -j | python3 -c '
import json, sys
for runtime, devices in json.load(sys.stdin)["devices"].items():
    for d in devices:
        if d["name"] == sys.argv[1] and d.get("isAvailable", True): print(d["udid"]); raise SystemExit
' "$name")"
if [ -z "$udid" ]; then
    dtype="$(xcrun simctl list devicetypes -j | python3 -c '
import json, sys
print(next(t["identifier"] for t in json.load(sys.stdin)["devicetypes"] if t["identifier"].endswith(".iPhone-Duo")))')"
    runtime="$(xcrun simctl list runtimes -j | python3 -c '
import json, sys
rs = [r for r in json.load(sys.stdin)["runtimes"] if r["isAvailable"] and r["identifier"].startswith("com.apple.CoreSimulator.SimRuntime.iOS-27-")]
ids = [r["identifier"] for r in rs]
pick = "com.apple.CoreSimulator.SimRuntime.iOS-27-1"
print(pick if pick in ids else sorted(ids, key=lambda i: [int(x) for x in i.split("iOS-")[1].split("-")])[-1])')"
    udid="$(xcrun simctl create "$name" "$dtype" "$runtime")"
    echo "windowsrig: made $name $udid ($dtype, $runtime)"
fi
echo "windowsrig: $name is $udid"

if [ -z "${RIG_NO_BUILD:-}" ] || [ ! -d "$derived/Build/Products/Debug-iphonesimulator/Sill.app" ]; then
    echo "windowsrig: building Sill (Debug, simulator)…"
    if ! xcodebuild -quiet -project "$root/iOSClient/Sill.xcodeproj" -scheme Sill -configuration Debug \
            -destination 'generic/platform=iOS Simulator' -derivedDataPath "$derived" \
            ARCHS=arm64 CODE_SIGN_IDENTITY=- build > "$results/build.log" 2>&1; then
        grep -E 'error:' "$results/build.log" | head -20 >&2
        echo "windowsrig: the build failed; $results/build.log" >&2
        exit 1
    fi
fi
app="$derived/Build/Products/Debug-iphonesimulator/Sill.app"
bid="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")"

state="$(xcrun simctl list devices -j | python3 -c '
import json, sys
for rt, ds in json.load(sys.stdin)["devices"].items():
    for d in ds:
        if d["udid"] == sys.argv[1]: print(d["state"])' "$udid")"
[ "$state" = Booted ] || xcrun simctl boot "$udid"
xcrun simctl bootstatus "$udid" -b >/dev/null
xcrun simctl install "$udid" "$app"
container="$(xcrun simctl get_app_container "$udid" "$bid" data)"
rig="$container/Documents/$logname"
rm -f "$rig"

echo "windowsrig: launching $bid ($mode)"
xcrun simctl launch --terminate-running-process --stdout="$results/console.out.log" --stderr="$results/console.err.log" \
    "$udid" "$bid" "${args[@]}" >/dev/null

# The result: the rig's own lines say when the run is over.
case "$mode" in
    pair) done_re='session: CONNECTED|pair: FAILED' ;;
    connect) done_re='session: CONNECTED|autoconnect: FAILED' ;;
    input) done_re='input: DONE|input: REFUSED|autoconnect: FAILED' ;;
    shot) done_re='^$NEVER' ;;
esac
# A screenshot of the default display (deviceScaleFactor), with a time limit: a display's capture can hang.
# The iPhone Duo has two displays (inner and cover) and simctl will not choose: each one it lists is taken.
displays=""
shot() {
    local cap=(perl -e 'alarm 30; exec @ARGV' xcrun simctl io "$udid" screenshot)
    if [ -z "$displays" ]; then
        if "${cap[@]}" "$results/$1.png" >>"$results/shot.log" 2>&1; then displays=none; return; fi
        displays="$(grep -oE -- '--display=[0-9]+' "$results/shot.log" | cut -d= -f2 | sort -u | tr '\n' ' ')"
        [ -n "$displays" ] || { displays=none; echo "windowsrig: screenshot $1 failed (shot.log)"; return; }
    fi
    if [ "$displays" = none ]; then "${cap[@]}" "$results/$1.png" >>"$results/shot.log" 2>&1 || echo "windowsrig: screenshot $1 failed"; return; fi
    for d in $displays; do
        "${cap[@]}" --display="$d" "$results/$1-display$d.png" >>"$results/shot.log" 2>&1 || echo "windowsrig: screenshot $1 display $d failed"
    done
}
shot_taken=0
for ((i = 0; i < wait_s; i++)); do
    if [ -f "$rig" ] && grep -Eq "$done_re" "$rig"; then break; fi
    # A first picture once connected, in case the run then ends badly.
    if [ $shot_taken = 0 ] && [ -f "$rig" ] && grep -q 'session: CONNECTED' "$rig"; then
        sleep 3; shot connected; shot_taken=1
    fi
    sleep 1
done
sleep 5
shot "$mode"
xcrun simctl terminate "$udid" "$bid" >/dev/null 2>&1 || true
[ -f "$rig" ] && cp "$rig" "$results/rig.log" || : > "$results/rig.log"

{
    echo "mode: $mode   simulator: $name $udid   app: $bid"
    echo "results: $results"
    grep -E 'pair: (OK|FAILED)|autoconnect:|session: CONNECTED|input: ([0-9]+ steps|REFUSED|DONE)' "$results/rig.log" | sed 's/^/  /' || true
    sent="$(grep -c 'input: t=' "$results/rig.log" || true)"
    [ "$mode" = input ] && echo "  events sent: $sent"
    last_status="$(grep 'status: ' "$results/rig.log" | tail -n 1 | sed 's/^[^ ]* //' || true)"
    [ -n "$last_status" ] && echo "  last $last_status"
    if grep -Eq "$done_re" "$results/rig.log" && ! grep -Eq 'FAILED|REFUSED' "$results/rig.log"; then echo "RESULT: OK"
    elif [ "$mode" = shot ]; then echo "RESULT: shot taken"
    else echo "RESULT: FAILED (or no answer in ${wait_s}s)"; fi
} | tee "$results/summary.txt"
