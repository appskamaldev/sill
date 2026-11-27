#!/bin/bash
# WindowsRig (iOSClient/WindowsRig.swift, a fork extension, DEBUG only): the Windows rig's launch
# arguments and its input sequence, compiled with StreamProtocol's sources, KeyChords.swift,
# TextComposition.swift and HostKeys.swift as one module, each with `import StreamProtocol` stripped.
#   Tests/checks/windows-rig/run.sh             compile and run the check
here="$(cd "$(dirname "$0")" && pwd)"
source "$here/../common.sh"
T=$(mktemp -d)
cp Sources/StreamProtocol/*.swift "$T"/
for f in iOSClient/KeyChords.swift iOSClient/TextComposition.swift iOSClient/HostKeys.swift iOSClient/WindowsRig.swift; do
    sed '/^import StreamProtocol$/d' "$f" > "$T/$(basename "$f")"
done
cp "$here/main.swift" "$T/main.swift"
swiftc -O -D DEBUG "$T"/*.swift -o "$out/check"
rm -rf "$T"
"$out/check"
