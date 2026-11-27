import Foundation

var checks = 0
var failures = 0
func check(_ ok: Bool, _ what: @autoclosure () -> String, line: Int = #line) {
    checks += 1
    if !ok { failures += 1; print("FAIL (line \(line)): \(what())") }
}

// MARK: -SillAutoConnect's target

let saved: [(id: String, name: String, hosts: [String])] = [
    (id: "pc1", name: "KAMAL-LAPTOP", hosts: ["100.90.47.26", "192.168.100.67"]),
    (id: "mac1", name: "Mac mini", hosts: ["fd7a:115c::1", "mac-mini.tail.ts.net"]),
]
check(WindowsRig.autoConnectTarget("kamal-laptop", saved: saved) == "pc1", "a name, any case")
check(WindowsRig.autoConnectTarget("Mac mini", saved: saved) == "mac1", "a name with a space")
check(WindowsRig.autoConnectTarget("100.90.47.26", saved: saved) == "pc1", "an address")
check(WindowsRig.autoConnectTarget("192.168.100.67:7455", saved: saved) == "pc1", "an address with a port")
check(WindowsRig.autoConnectTarget("[fd7a:115c::1]:7455", saved: saved) == "mac1", "bracketed IPv6 with a port")
check(WindowsRig.autoConnectTarget("FD7A:115C::1", saved: saved) == "mac1", "bare IPv6, any case")
check(WindowsRig.autoConnectTarget("Mac-Mini.Tail.ts.net", saved: saved) == "mac1", "a host name, any case")
check(WindowsRig.autoConnectTarget("10.0.0.9", saved: saved) == nil, "nothing matches")
check(WindowsRig.autoConnectTarget("  ", saved: saved) == nil, "empty")
check(WindowsRig.autoConnectTarget("x", saved: [(id: "a", name: "X", hosts: []), (id: "b", name: "x", hosts: [])]) == nil,
      "two by the same name: neither")
check(WindowsRig.bareHost("localhost:9") == "localhost", "a name's port goes")

// MARK: the input script's exception

check(WindowsRig.remoteInputAllowed(hostPlatform: "windows", flag: true), "a Windows host with the flag")
check(!WindowsRig.remoteInputAllowed(hostPlatform: "windows", flag: false), "not without the flag")
check(!WindowsRig.remoteInputAllowed(hostPlatform: nil, flag: true), "never a Mac")
check(!WindowsRig.remoteInputAllowed(hostPlatform: "Windows", flag: true), "only the wire's exact word")

// MARK: the Windows sequence

let steps = WindowsRig.windowsSequence()
check(steps.map(\.name) == ["move to centre", "left click", "type \"sill rig 123\"", "Return", "Start", "Esc",
                            "Ctrl+A", "Backspace", "right click", "scroll down 3"], "the order: \(steps.map(\.name))")
check(zip(steps, steps.dropFirst()).allSatisfy { $1.at > $0.at }, "each later than the one before")
func lines(_ i: Int) -> [String] { steps[i].events.map(WindowsRig.describe) }
check(lines(0) == ["pointer move 0.500,0.500"], "move: \(lines(0))")
check(lines(1) == ["pointer leftDown 0.500,0.500", "pointer leftUp 0.500,0.500"], "left click: \(lines(1))")
check(steps[2].events == [.text("sill rig 123")], "the text")
check(lines(3) == ["key 0x28 down mods 0x0", "key 0x28 up mods 0x0"], "Return: \(lines(3))")
check(lines(4) == ["key 0xE3 down mods 0x0", "key 0xE3 up mods 0x0"], "Start alone is Win down and up: \(lines(4))")
check(lines(5) == ["key 0x29 down mods 0x0", "key 0x29 up mods 0x0"], "Esc: \(lines(5))")
check(lines(6) == ["key 0xE0 down mods 0x40000", "key 0x04 down mods 0x40000", "key 0x04 up mods 0x40000", "key 0xE0 up mods 0x0"],
      "Ctrl+A: \(lines(6))")
check(lines(7) == ["key 0x2A down mods 0x0", "key 0x2A up mods 0x0"], "Backspace: \(lines(7))")
check(lines(8) == ["pointer rightDown 0.500,0.500", "pointer rightUp 0.500,0.500"], "right click: \(lines(8))")
check(steps[9].events.count == 3 && steps[9].events.allSatisfy {
    if case .scroll(_, _, let dx, let dy) = $0 { return dx == 0 && dy < 0 } else { return false } }, "three steps down")

print("windows-rig: \(checks - failures) of \(checks) checks passed")
exit(failures == 0 ? 0 : 1)
