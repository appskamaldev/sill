#!/bin/bash
# HostKeys (iOSClient/HostKeys.swift, a fork extension): the keys this device offers and sends for a Mac
# host and for a Windows PC (`WindowList.hostPlatform`, `Hello.pcKeys`), compiled with StreamProtocol's
# sources, KeyChords.swift and TextComposition.swift as one module, each with `import StreamProtocol` stripped.
#   Tests/checks/host-keys/run.sh             compile and run the check
here="$(cd "$(dirname "$0")" && pwd)"
source "$here/../common.sh"
T=$(mktemp -d)
cp Sources/StreamProtocol/*.swift "$T"/
for f in iOSClient/KeyChords.swift iOSClient/TextComposition.swift iOSClient/HostKeys.swift; do
    sed '/^import StreamProtocol$/d' "$f" > "$T/$(basename "$f")"
done
cp "$here/main.swift" "$T/main.swift"
swiftc -O "$T"/*.swift -o "$out/check"
rm -rf "$T"
"$out/check"
