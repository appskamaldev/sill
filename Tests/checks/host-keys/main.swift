import Foundation

var checks = 0
var failures = 0
func check(_ ok: Bool, _ what: @autoclosure () -> String, line: Int = #line) {
    checks += 1
    if !ok { failures += 1; print("FAIL (line \(line)): \(what())") }
}
func json<T: Encodable>(_ value: T) -> String {
    let e = JSONEncoder(); e.outputFormatting = [.sortedKeys]
    return String(data: try! e.encode(value), encoding: .utf8)!
}
func keys(_ events: [InputEvent]) -> [String] {
    events.map { e -> String in
        guard case .key(let u, let d, let m) = e else { return "\(e)" }
        return String(format: "%02X%@%llX", u, d ? "v" : "^", m >> 16)
    }
}

// MARK: The platform, from the wire

check(HostPlatform(wire: "windows") == .windows, "\"windows\" is a PC")
for wire in [nil, "", "mac", "macos", "Windows", "linux"] as [String?] {
    check(HostPlatform(wire: wire) == .mac, "\(wire ?? "nil") is a Mac")
}
check(HostKeys.pcKeysGeneration == 1, "pcKeys is generation 1")
check(HostKeys.literal(.windows), "a PC with pcKeys declared is literal")
check(!HostKeys.literal(.windows, declared: nil) && !HostKeys.literal(.windows, declared: 0), "not without pcKeys")
check(!HostKeys.literal(.mac) && !HostKeys.literal(.mac, declared: 1), "a Mac is never literal")

// MARK: The wire's two fields (fork extensions)

let hello = Hello(appVersion: "0.6", pcKeys: 1)
check(json(hello) == #"{"appVersion":"0.6","pcKeys":1}"#, "Hello's pcKeys key: \(json(hello))")
check(json(Hello(appVersion: "0.6")) == #"{"appVersion":"0.6"}"#, "no pcKeys key when nil")
check(Wire.decode(Hello.self, from: Data("{}".utf8)) == Hello(), "{} still decodes")
check(Wire.decode(Hello.self, from: Data(#"{"appVersion":"1.0","offConsoleView":1}"#.utf8))?.pcKeys == nil, "an upstream hello: pcKeys nil")
check(Wire.decode(Hello.self, from: Data(#"{"pcKeys":1}"#.utf8))?.pcKeys == 1, "pcKeys decodes")
check(Wire.decode(Hello.self, from: Data(#"{"pcKeys":"1"}"#.utf8)) == nil, "pcKeys as a string does not decode")

let oldList = #"{"macName":"Mac","windows":[],"active":{"desktop":{}}}"#
var macList = WindowList(macName: "Mac", windows: [], active: .desktop)
let macJSON = json(macList)
check(!macJSON.contains("hostPlatform"), "a Mac's list has no hostPlatform key: \(macJSON)")
check(Wire.decode(WindowList.self, from: Wire.encode(macList))?.hostPlatform == nil, "a Mac's list: nil")
macList.hostPlatform = "windows"
check(json(macList).contains(#""hostPlatform":"windows""#), "the PC's key: \(json(macList))")
let back = Wire.decode(WindowList.self, from: Wire.encode(macList))
check(back?.hostPlatform == "windows" && HostPlatform(wire: back?.hostPlatform) == .windows, "a PC's list round-trips")
if let old = Wire.decode(WindowList.self, from: Wire.encode(WindowList(macName: "Mac", windows: [], active: .desktop))) {
    check(HostPlatform(wire: old.hostPlatform) == .mac, "an old list is a Mac's")
} else { check(false, "an old list decodes") }
_ = oldList

// MARK: The modifiers per platform

let mac = HostKeys.modifiers(.mac)
check(mac.map(\.title) == ["ctrl", "opt", "cmd", "shift"], "a Mac's caps: \(mac.map(\.title))")
check(mac.map(\.label) == ["Control", "Option", "Command", "Shift"], "a Mac's names")
let pc = HostKeys.modifiers(.windows)
check(pc.map(\.label) == ["Start", "Control", "Alt", "Shift"], "a PC's names: \(pc.map(\.label))")
check(pc.map(\.title) == [HostKeys.startTitle, "Ctrl", "Alt", "Shift"], "a PC's caps")
check(!pc.contains { $0.title.contains("cmd") || $0.title.contains("opt") || $0.label.contains("Command") || $0.label.contains("Option") },
      "no Mac names on a PC's caps")
check(pc.map(\.usage) == [0xE3, 0xE0, 0xE2, 0xE1], "Win 0xE3, Ctrl 0xE0, Alt 0xE2, Shift 0xE1: \(pc.map(\.usage))")
check(pc.map(\.flag) == [.command, .control, .option, .shift], "with command, control, option, shift")
check(pc.map(\.flag.rawValue) == [1 << 20, 1 << 18, 1 << 19, 1 << 17], "the flags' bits")
check(mac.map(\.usage) == [0xE0, 0xE2, 0xE3, 0xE1], "a Mac's keys unchanged")
check(HostKeys.name(of: .command, on: .windows) == "Start" && HostKeys.name(of: .command, on: .mac) == "Command", "⌘'s name per host")
check(HostKeys.name(of: .option, on: .windows) == "Alt" && HostKeys.name(of: .option, on: .mac) == "Option", "⌥'s name per host")
// Win then D, latched from the row: Win's own key around D.
check(keys(KeyChord.press(0x07, with: .command)) == ["E3v10", "07v10", "07^10", "E3^0"], "Win+D from the latch")
// Start alone.
check(HostKeys.startExpired(latched: []) == nil, "nothing latched: nothing")
check(HostKeys.startExpired(latched: .shift) == nil, "Win spent already: nothing")
check(keys(HostKeys.startExpired(latched: .command) ?? []) == ["E3v0", "E3^0"], "Start alone: Win down and up")
check(keys(HostKeys.startExpired(latched: [.command, .control]) ?? []) == ["E0v4", "E3v4", "E3^4", "E0^0"], "Start with Ctrl latched")
check(HostKeys.startAloneAfter > 0.3 && HostKeys.startAloneAfter <= 1.0, "Start waits under a second")
check(HostKeys.deleteKey == 0x4C && HostKeys.winKey == 0xE3, "Del is forward delete; Win the left GUI key")

// MARK: The shortcuts per platform

check(HostKeys.chords(.mac).isEmpty, "a Mac's row has no Shortcuts menu")
let chords = HostKeys.chords(.windows)
let expected: [(String, String)] = [("Search", "Win+S"), ("Switch apps", "Alt+Tab"), ("Show desktop", "Win+D"), ("Task view", "Win+Tab"),
                                    ("File Explorer", "Win+E"), ("Task Manager", "Ctrl+Shift+Esc"), ("Lock", "Win+L"), ("Close", "Alt+F4")]
check(chords.map(\.title) == expected.map(\.0), "a PC's shortcuts: \(chords.map(\.title))")
check(chords.map(HostKeys.keysText) == expected.map(\.1), "their keys: \(chords.map(HostKeys.keysText))")
check(!chords.contains { $0.modifiers == [.control, .option] && $0.usage == 0x4C }, "never Ctrl+Alt+Del")
check(Set(chords.map(\.title)).count == chords.count && Set(chords.map(\.label)).count == chords.count, "every shortcut its own name")
check(chords.allSatisfy { !$0.label.contains("Mac") && !$0.title.contains("Mac") && !$0.symbol.isEmpty }, "PC words, a symbol each")
check(keys(chords[1].events) == ["E2v8", "2Bv8", "2B^8", "E2^0"], "Alt+Tab exactly: \(keys(chords[1].events))")
check(keys(chords[5].events) == ["E0v4", "E1v6", "29v6", "29^6", "E1^4", "E0^0"], "Ctrl+Shift+Esc exactly: \(keys(chords[5].events))")
check(keys(chords[7].events) == ["E2v8", "3Dv8", "3D^8", "E2^0"], "Alt+F4 exactly")
check(keys(HostKeys.search(.windows).events) == ["E3v10", "16v10", "16^10", "E3^0"], "Search is Win+S")
check(HostKeys.search(.mac).events == KeyChord.spotlight, "a Mac's search is Spotlight, ⌘Space, as before")
check(HostKeys.paste(.mac) == KeyChord.paste, "a Mac's paste is ⌘V, as before")
check(keys(HostKeys.paste(.windows)) == ["E0v4", "19v4", "19^4", "E0^0"], "a PC's paste is Ctrl+V: \(keys(HostKeys.paste(.windows)))")
for chord in chords {   // every shortcut leaves nothing held
    var held: Set<UInt16> = []
    for e in chord.events { if case .key(let u, let d, _) = e { if d { held.insert(u) } else { held.remove(u) } } }
    check(held.isEmpty, "\(chord.title) leaves nothing down")
    if case .key(_, _, let m)? = chord.events.last { check(m == 0, "\(chord.title)'s last up carries no flag") }
}

// MARK: ⌘ as Ctrl on PCs

check(HostKeys.commandAsControlDefault, "on by default")
check(HostKeys.commandAsControlKey == "Sill.commandAsControl", "its key")
check(HostKeys.remapsCommand(.windows, switchOn: true), "a PC, on: remapped")
check(!HostKeys.remapsCommand(.windows, switchOn: false), "a PC, off: ⌘ is Win")
check(!HostKeys.remapsCommand(.mac, switchOn: true) && !HostKeys.remapsCommand(.mac, switchOn: false), "a Mac: never")
check(!HostKeys.remapsCommand(.windows, switchOn: true, declared: nil), "not without pcKeys")
let cmd = KeyModifiers.command.rawValue, ctl = KeyModifiers.control.rawValue, opt = KeyModifiers.option.rawValue
let sh = KeyModifiers.shift.rawValue
let remap = HostKeys.commandAsControl
check(remap(.key(hidUsage: 0xE3, down: true, modifiers: cmd)) == .key(hidUsage: 0xE0, down: true, modifiers: ctl), "⌘ down is Ctrl down")
check(remap(.key(hidUsage: 0xE7, down: false, modifiers: 0)) == .key(hidUsage: 0xE4, down: false, modifiers: 0), "right ⌘ up is right Ctrl up")
check(remap(.key(hidUsage: 0x06, down: true, modifiers: cmd)) == .key(hidUsage: 0x06, down: true, modifiers: ctl), "⌘C is Ctrl+C")
check(remap(.key(hidUsage: 0x06, down: true, modifiers: cmd | sh)) == .key(hidUsage: 0x06, down: true, modifiers: ctl | sh), "⌘⇧C is Ctrl+Shift+C")
check(remap(.key(hidUsage: 0x2B, down: true, modifiers: opt)) == .key(hidUsage: 0x2B, down: true, modifiers: opt), "⌥Tab is Alt+Tab")
check(remap(.key(hidUsage: 0xE2, down: true, modifiers: opt)) == .key(hidUsage: 0xE2, down: true, modifiers: opt), "⌥ is Alt")
check(remap(.key(hidUsage: 0xE0, down: true, modifiers: ctl)) == .key(hidUsage: 0xE0, down: true, modifiers: ctl), "⌃ is Ctrl")
check(remap(.key(hidUsage: 0x04, down: true, modifiers: cmd | ctl)) == .key(hidUsage: 0x04, down: true, modifiers: ctl), "⌃⌘A is Ctrl+A")
check(remap(.key(hidUsage: 0x04, down: false, modifiers: 0)) == .key(hidUsage: 0x04, down: false, modifiers: 0), "a plain key as it was")
// A hardware ⌘C through ForwardedKeys and the remap leaves nothing held: Ctrl down, C down and up, Ctrl up.
var fk = ForwardedKeys()
var sent: [InputEvent] = []
if let e = fk.began(0xE3, modifiers: cmd, typing: .text) { sent.append(remap(e)) }
if let e = fk.began(0x06, modifiers: cmd, typing: .text) { sent.append(remap(e)) }
if let e = fk.ended(0x06) { sent.append(remap(e)) }
if let e = fk.ended(0xE3) { sent.append(remap(e)) }
check(keys(sent) == ["E0v4", "06v4", "06^4", "E0^0"], "⌘C from a keyboard to a PC: \(keys(sent))")
check(HostKeys.hostWord(.windows) == "PC" && HostKeys.hostWord(.mac) == "Mac", "the host's word")

print("host-keys: \(checks - failures) of \(checks) checks pass")
exit(failures == 0 ? 0 : 1)
