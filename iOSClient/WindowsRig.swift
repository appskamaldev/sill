import Foundation
import os
import StreamProtocol

// FORK EXTENSION (appskamaldev/sill, not upstream), DEBUG harness only: the Windows rig
// (Tests/windowsrig). What the rig's launch arguments ask for, and the input sequence it sends a
// Windows host, as pure values: `Tests/checks/windows-rig` checks them. The app's side (the log file,
// the dial, the sends) is in StreamClient+Remote.swift and ContentView.swift's InputScript.

#if DEBUG
enum WindowsRig {
    /// The rig's log, in the app's Documents: the script reads it with `simctl get_app_container`.
    static let logName = "sill-rig.log"

    /// The rig's arguments are on this launch: only then does anything go to its log.
    static let active: Bool = {
        let d = UserDefaults.standard
        return d.string(forKey: "SillPairCode") != nil || d.string(forKey: "SillAutoConnect") != nil
            || d.string(forKey: "SillInputScript") == "windows" || d.bool(forKey: "SillRigLog")
    }()

    /// `-SillAutoConnect <saved name or address>`: the saved computer it names, by its name (any
    /// case) or by one of its addresses (a `:port` after it is ignored, so is `[…]` around IPv6).
    /// Nil when none matches, or more than one does by name.
    static func autoConnectTarget(_ query: String, saved: [(id: String, name: String, hosts: [String])]) -> String? {
        let q = query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return nil }
        let byName = saved.filter { $0.name.caseInsensitiveCompare(q) == .orderedSame }
        if byName.count == 1 { return byName[0].id }
        if byName.count > 1 { return nil }
        let host = bareHost(q)
        return saved.first { $0.hosts.contains { $0.lowercased() == host } }?.id
    }

    /// "100.90.47.26:7455" → "100.90.47.26", "[fd7a::1]:7455" → "fd7a::1", "fd7a::1" as it is.
    static func bareHost(_ text: String) -> String {
        var s = text.lowercased()
        if s.hasPrefix("["), let close = s.firstIndex(of: "]") { return String(s[s.index(after: s.startIndex)..<close]) }
        if s.filter({ $0 == ":" }).count == 1, let colon = s.lastIndex(of: ":") { s = String(s[..<colon]) }
        return s
    }

    /// The input script's exception to its loopback rule: a host whose list says `hostPlatform`
    /// "windows" (it posts the input on that PC, which is the point) and `-SillAllowRemoteInputTest 1`.
    static func remoteInputAllowed(hostPlatform: String?, flag: Bool) -> Bool {
        flag && hostPlatform == "windows"
    }

    /// One step of the Windows sequence: when (seconds after the first window list), its name in the
    /// log, and the wire events it sends.
    struct Step: Equatable {
        let at: Double
        let name: String
        let events: [InputEvent]
    }

    /// `-SillInputScript windows`: the pointer to the frame's centre, a left click, "sill rig 123",
    /// Return, Start alone (Win down and up, as the key row's Start left alone: HostKeys.startExpired),
    /// Esc, Ctrl+A, Backspace, a right click, and three wheel steps down (plain `.scroll`, as a wheel
    /// sends: a negative dy moves the content up, so the view goes down). A step every `gap` seconds,
    /// the first at `start`.
    static func windowsSequence(start: Double = 4, gap: Double = 1.5) -> [Step] {
        let c = 0.5
        let scrollStep: Double = -0.05
        let parts: [(String, [InputEvent])] = [
            ("move to centre", [.pointer(.move, x: c, y: c)]),
            ("left click", [.pointer(.leftDown, x: c, y: c), .pointer(.leftUp, x: c, y: c)]),
            ("type \"sill rig 123\"", [.text("sill rig 123")]),
            ("Return", KeyChord.press(0x28, with: [])),
            ("Start", HostKeys.startExpired(latched: .command) ?? []),
            ("Esc", KeyChord.press(0x29, with: [])),
            ("Ctrl+A", KeyChord.press(0x04, with: .control)),
            ("Backspace", KeyChord.press(0x2A, with: [])),
            ("right click", [.pointer(.rightDown, x: c, y: c), .pointer(.rightUp, x: c, y: c)]),
            ("scroll down 3", Array(repeating: InputEvent.scroll(x: c, y: c, dx: 0, dy: scrollStep), count: 3)),
        ]
        return parts.enumerated().map { i, p in Step(at: start + Double(i) * gap, name: p.0, events: p.1) }
    }

    /// One event as the log writes it, short enough to match against the host's log.
    static func describe(_ e: InputEvent) -> String {
        switch e {
        case .pointer(let a, let x, let y): return String(format: "pointer %@ %.3f,%.3f", a.rawValue, x, y)
        case .scroll(let x, let y, let dx, let dy): return String(format: "scroll at %.3f,%.3f d %.3f,%.3f", x, y, dx, dy)
        case .text(let t): return "text \(t.debugDescription)"
        case .scrollGesture(let p, let x, let y): return String(format: "scrollGesture %@ %.3f,%.3f", p.rawValue, x, y)
        case .key(let u, let d, let m): return String(format: "key 0x%02X %@ mods 0x%llX", u, d ? "down" : "up", m)
        }
    }

    /// A line for the rig: on the console, in os_log (subsystem "sill.rig") and appended to
    /// Documents/sill-rig.log, each with this device's wall-clock time to the millisecond (UTC), to
    /// match against the host's log. Any thread.
    static func log(_ text: String) {
        let stamp = stampFormatter.string(from: Date())
        let line = "\(stamp) \(text)"
        print("rig: \(line)")
        logger.notice("\(text, privacy: .public)")
        logQueue.async {
            guard let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else { return }
            let url = dir.appendingPathComponent(logName)
            let data = Data((line + "\n").utf8)
            if let handle = try? FileHandle(forWritingTo: url) {
                handle.seekToEndOfFile()
                handle.write(data)
                try? handle.close()
            } else {
                try? data.write(to: url)
            }
        }
    }

    private static let logger = Logger(subsystem: "sill.rig", category: "rig")
    private static let logQueue = DispatchQueue(label: "sill.rig.log")
    private static let stampFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
}
#endif
