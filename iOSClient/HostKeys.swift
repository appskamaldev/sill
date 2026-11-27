import Foundation
import StreamProtocol

// FORK EXTENSION (appskamaldev/sill, not upstream): the keys this device offers and sends for the host it
// streams from, a Mac or a Windows PC (`WindowList.hostPlatform`). A Mac host never sends the field, so
// everything stays as it was with one. A Windows host's list says "windows", and this device's hello says
// `pcKeys` 1, so its kind 8 keys are a PC's own, literally: Win is HID 0xE3 with the command flag, Ctrl
// 0xE0 with control, Alt 0xE2 with option, Shift as ever (the same bytes KeyModifiers.keys sends; only
// their names and the shortcuts differ). A hardware keyboard's ⌘ goes as Ctrl there, behind this device's
// "Use ⌘ as Ctrl on PCs" (on by default), so ⌘C copies on the PC as it does on a Mac.
//
// Pure: Foundation, StreamProtocol and KeyChords.swift only, checked with swiftc in Tests/checks/host-keys.

/// The kind of computer at the other end, by its window list.
enum HostPlatform: Equatable {
    case mac
    case windows

    /// `WindowList.hostPlatform`: "windows" is a PC; missing or anything else is a Mac.
    init(wire: String?) {
        self = wire == "windows" ? .windows : .mac
    }
}

enum HostKeys {
    /// What this device's hello says (`Hello.pcKeys`): its keys are a PC's when the host is one.
    static let pcKeysGeneration = 1

    /// Whether this device sends a PC's keys literally: the host says it is Windows, and the hello said so.
    static func literal(_ platform: HostPlatform, declared: Int? = pcKeysGeneration) -> Bool {
        platform == .windows && (declared ?? 0) >= 1
    }

    // MARK: The modifier caps

    /// A modifier as the key row names it, and the bit it latches (which `KeyModifiers.keys` sends as its
    /// own key: 0xE0, 0xE1, 0xE2, 0xE3).
    struct Modifier: Equatable {
        let title: String
        let label: String
        let flag: KeyModifiers
        /// Its own key's HID usage, as `KeyModifiers.keys` sends it.
        var usage: UInt16 { flag.keys.first?.usage ?? 0 }
    }

    /// The modifiers a host's key row shows, in its order: a Mac's ctrl, opt, cmd, shift; a PC's Start
    /// (Win), Ctrl, Alt, Shift.
    static func modifiers(_ platform: HostPlatform) -> [Modifier] {
        switch platform {
        case .mac:
            return [Modifier(title: "ctrl", label: "Control", flag: .control),
                    Modifier(title: "opt", label: "Option", flag: .option),
                    Modifier(title: "cmd", label: "Command", flag: .command),
                    Modifier(title: "shift", label: "Shift", flag: .shift)]
        case .windows:
            return [Modifier(title: startTitle, label: "Start", flag: .command),
                    Modifier(title: "Ctrl", label: "Control", flag: .control),
                    Modifier(title: "Alt", label: "Alt", flag: .option),
                    Modifier(title: "Shift", label: "Shift", flag: .shift)]
        }
    }

    /// The Start cap's text: a neutral four-pane glyph and the word (there is no Windows logo symbol).
    static let startTitle = "⊞ Start"

    /// A modifier's name for a host, VoiceOver's and the copy's: Win is "Start" on a PC, ⌘ "Command" on a Mac.
    static func name(of flag: KeyModifiers, on platform: HostPlatform) -> String {
        modifiers(platform).first { $0.flag == flag }?.label ?? ""
    }

    /// How long a tapped Start waits for a key to join it on a PC (`StartTap`): then it goes alone, as a
    /// press and release, which opens the Start menu.
    static let startAloneAfter: Double = 0.8

    /// The Start cap on a PC: a tap latches Win, as cmd latches ⌘; a key within `startAloneAfter` goes
    /// with it (Win+D), and the latch is spent as ever. With none, the latch is spent on Win alone, down
    /// and up (what else is latched with it), and the Start menu opens. Nil: nothing to send now.
    static func startExpired(latched: KeyModifiers) -> [InputEvent]? {
        guard latched.contains(.command) else { return nil }
        return KeyChord.press(winKey, with: latched.subtracting(.command))
    }

    /// Win's own key: the left GUI key, which a Mac calls ⌘.
    static let winKey: UInt16 = 0xE3
    /// Forward delete, the PC's Del.
    static let deleteKey: UInt16 = 0x4C

    // MARK: Shortcuts

    /// One shortcut: its words, VoiceOver's, its SF Symbol, the key and what goes with it.
    struct Chord: Equatable {
        let title: String
        let label: String
        let symbol: String
        let usage: UInt16
        let modifiers: KeyModifiers

        /// Exactly this shortcut, whatever is latched: its modifiers' own keys down, the key down and up,
        /// their keys up (KeyChord.press).
        var events: [InputEvent] { KeyChord.press(usage, with: modifiers) }
    }

    /// Search: a Mac's Spotlight (⌘Space), a PC's Win+S.
    static func search(_ platform: HostPlatform) -> Chord {
        switch platform {
        case .mac: return Chord(title: "Spotlight", label: "Open Spotlight on the Mac", symbol: "magnifyingglass",
                                usage: 0x2C, modifiers: .command)
        case .windows: return Chord(title: "Search", label: "Search on the PC", symbol: "magnifyingglass",
                                    usage: 0x16, modifiers: .command)
        }
    }

    /// Paste on the host (the clipboard's Paste right away): ⌘V on a Mac, Ctrl+V on a PC.
    static func paste(_ platform: HostPlatform) -> [InputEvent] {
        KeyChord.press(ClipboardPolicy.pasteKey, with: platform == .windows ? .control : .command)
    }

    /// The Shortcuts menu a host's key row offers: none on a Mac (its row has Spotlight); a PC's Search
    /// and the shortcuts a Windows keyboard has that a phone does not. Ctrl+Alt+Del cannot be sent to
    /// Windows, so Task Manager is Ctrl+Shift+Esc.
    static func chords(_ platform: HostPlatform) -> [Chord] {
        switch platform {
        case .mac: return []
        case .windows: return [
            search(.windows),
            Chord(title: "Switch apps", label: "Switch apps, Alt Tab", symbol: "rectangle.on.rectangle", usage: 0x2B, modifiers: .option),
            Chord(title: "Show desktop", label: "Show desktop, Start D", symbol: "menubar.dock.rectangle", usage: 0x07, modifiers: .command),
            Chord(title: "Task view", label: "Task view, Start Tab", symbol: "square.grid.2x2", usage: 0x2B, modifiers: .command),
            Chord(title: "File Explorer", label: "File Explorer, Start E", symbol: "folder", usage: 0x08, modifiers: .command),
            Chord(title: "Task Manager", label: "Task Manager, Control Shift Escape", symbol: "list.bullet.rectangle",
                  usage: 0x29, modifiers: [.control, .shift]),
            Chord(title: "Lock", label: "Lock the PC, Start L", symbol: "lock", usage: 0x0F, modifiers: .command),
            Chord(title: "Close", label: "Close the window, Alt F4", symbol: "xmark.square", usage: 0x3D, modifiers: .option),
        ]
        }
    }

    /// The shortcut's keys as a PC's keyboard prints them, for the menu's subtitle: "Alt+Tab", "Win+D".
    static func keysText(_ chord: Chord) -> String {
        var parts: [String] = []
        if chord.modifiers.contains(.command) { parts.append("Win") }
        if chord.modifiers.contains(.control) { parts.append("Ctrl") }
        if chord.modifiers.contains(.shift) { parts.append("Shift") }
        if chord.modifiers.contains(.option) { parts.append("Alt") }
        let key: String
        switch chord.usage {
        case 0x04...0x1D: key = String(UnicodeScalar(UInt8(0x41 + chord.usage - 0x04)))
        case 0x2B: key = "Tab"
        case 0x29: key = "Esc"
        case 0x3A...0x45: key = "F\(chord.usage - 0x39)"
        default: key = "?"
        }
        return (parts + [key]).joined(separator: "+")
    }

    // MARK: A hardware keyboard's ⌘ on a PC

    /// "Use ⌘ as Ctrl on PCs", this device's own switch (Settings › This iPhone/iPad › Keys): on unless
    /// turned off. Never sent.
    static let commandAsControlKey = "Sill.commandAsControl"
    static let commandAsControlDefault = true

    /// Whether a hardware keyboard's ⌘ goes as Ctrl: to a PC sent keys literally, with the switch on.
    static func remapsCommand(_ platform: HostPlatform, switchOn: Bool, declared: Int? = pcKeysGeneration) -> Bool {
        literal(platform, declared: declared) && switchOn
    }

    /// A hardware keyboard's key on its way to a PC with ⌘ as Ctrl: ⌘'s own keys (0xE3, 0xE7) go as
    /// Ctrl's (0xE0, 0xE4), and the command flag as control. ⌥ is Alt and ⌃ Ctrl as they are. Anything
    /// but a key goes unchanged.
    static func commandAsControl(_ event: InputEvent) -> InputEvent {
        guard case .key(let usage, let down, let modifiers) = event else { return event }
        let command = KeyModifiers.command.rawValue
        let mods = modifiers & command != 0 ? (modifiers & ~command) | KeyModifiers.control.rawValue : modifiers
        let key: UInt16
        switch usage {
        case 0xE3: key = 0xE0
        case 0xE7: key = 0xE4
        default: key = usage
        }
        return .key(hidUsage: key, down: down, modifiers: mods)
    }

    // MARK: Words

    /// What the stream screen calls the host where its keys are concerned: "Mac", or "PC".
    static func hostWord(_ platform: HostPlatform) -> String { platform == .windows ? "PC" : "Mac" }

    enum Copy {
        static let commandAsControlTitle = "Use ⌘ as Ctrl on PCs"
        static let commandAsControlFootnote = "A keyboard's ⌘ works as Ctrl on a Windows PC, so ⌘C copies and ⌘V pastes."
        /// The Shortcuts cap, on a PC.
        static let shortcuts = "Shortcuts"
        static let shortcutsLabel = "PC shortcuts"
        /// VoiceOver's action on a PC's scrolling row with Arrow Keys off: on to its last caps.
        static let showMore = "Show more keys"
    }
}
