# `windows-rig`

**Checks:** a fork extension (appskamaldev/sill, not upstream), DEBUG only: the Windows rig's pure parts (`WindowsRig`, Tests/windowsrig). `-SillAutoConnect`'s match of a saved computer by name (any case; two of one name match neither) or by address (a port, IPv6 brackets and case ignored); the input script's exception to its loopback rule (only `hostPlatform` "windows" with `-SillAllowRemoteInputTest 1`); `-SillInputScript windows`'s sequence in order with its exact events: the pointer to the centre, a left click, "sill rig 123" as text, Return, Start alone as Win down and up, Esc, Ctrl+A with Ctrl's own key around it, Backspace, a right click, three wheel steps down

**Compiles:** `Sources/StreamProtocol/*.swift`, `iOSClient/KeyChords.swift`, `iOSClient/TextComposition.swift`, `iOSClient/HostKeys.swift` and `iOSClient/WindowsRig.swift` (`swiftc -D DEBUG`)
