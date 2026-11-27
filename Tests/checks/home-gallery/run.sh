#!/bin/bash
# HomeGallery (iOSClient/HomeGallery.swift) on its own: the connect screen's gallery. Each card state's
# icon, colour and word (never a colour alone), VoiceOver's label and hint, the hero, which card a dial
# line names, and which outcomes are a banner, with its title, line and action.
#   Tests/checks/home-gallery/run.sh             compile and run the check
here="$(cd "$(dirname "$0")" && pwd)"
source "$here/../common.sh"
swiftc -O iOSClient/HomeGallery.swift "$here/main.swift" -o "$out/check"
"$out/check"
