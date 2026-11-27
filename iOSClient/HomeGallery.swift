import Foundation

/// The connect screen as a gallery of computers (direction A, Kamal, 2026-11-27): what each card says
/// about its computer, which card is the hero, which dial a status line names, and which outcomes are
/// a banner at the top instead of the status line. Pure: Foundation only, checked on its own with
/// swiftc (Tests/checks/home-gallery). ConnectScreen draws it; StreamClient raises the banners.
enum HomeGallery {
    // MARK: A card's state

    /// How a listed computer is reached at home: the row's word (DiscoveryPolicy.Method).
    enum Via: Equatable { case wired, wifi, direct, unknown }

    /// What a card says about its computer, beside its name. Never a colour alone: always an icon, a
    /// colour and a word (`look`).
    enum State: Equatable {
        /// Listed at home, paired (or a DEBUG build's plain door).
        case here(Via)
        /// A saved computer dialled from afar (its Remote row). "Away", never "Remote".
        case away
        /// Not paired: a tap asks, and the computer shows a code (over the cable it pairs by itself).
        case notPaired(cable: Bool)
        /// A saved computer that removed this device (or answered with a new key): a tap pairs again.
        case removed
        /// The computer's Sill is too old for this build: nothing is dialled.
        case updateNeeded
        /// A tap's dial of it did not answer. `justFailed`: the latest tap's, which says what to do.
        case offline(justFailed: Bool)
    }

    /// The facts of a row the state follows from (FoundMac, mapped by ConnectScreen).
    struct Row: Equatable {
        enum Route: Equatable { case network, direct, remote }
        /// DiscoveryPolicy.RowWord.
        enum Word: Equatable { case method(Via), notPaired, openDoor, pairsOverCable, updateSill }
        var route: Route
        var word: Word
        /// A saved computer's row (its tag, or its Bonjour name, named one).
        var saved: Bool
    }

    /// A row's state. `unanswered`: a tap's dial of it ended without a session or a banner.
    static func state(_ row: Row, unanswered: Bool = false, justFailed: Bool = false) -> State {
        if unanswered { return .offline(justFailed: justFailed) }
        if row.route == .remote { return .away }
        switch row.word {
        case .method(let via): return .here(via)
        case .openDoor: return .notPaired(cable: false)
        case .notPaired: return row.saved ? .removed : .notPaired(cable: false)
        case .pairsOverCable: return row.saved ? .removed : .notPaired(cable: true)
        case .updateSill: return .updateNeeded
        }
    }

    /// The colours a card's state line takes: system colours and the one accent.
    enum Tint: Equatable { case green, blue, orange, red, tertiary, accent }

    /// What a card shows for its state: an SF Symbol (nil: a ProgressView), its colour and its word.
    struct Look: Equatable {
        let symbol: String?
        let tint: Tint
        let word: String
    }

    static let connectingWord = "Connecting\u{2026}"
    static let notAnswering = "Not answering. Tap to try again."

    static func look(_ state: State, connecting: Bool = false) -> Look {
        if connecting { return Look(symbol: nil, tint: .accent, word: connectingWord) }
        switch state {
        case .here(.wired): return Look(symbol: "cable.connector", tint: .green, word: "Wired")
        case .here(.wifi): return Look(symbol: "wifi", tint: .green, word: "Wi\u{2011}Fi")
        case .here(.direct): return Look(symbol: "antenna.radiowaves.left.and.right", tint: .green, word: "Direct")
        case .here(.unknown): return Look(symbol: "network", tint: .green, word: "Here")
        case .away: return Look(symbol: "globe", tint: .blue, word: "Away")
        case .notPaired(let cable): return Look(symbol: cable ? "cable.connector" : "key", tint: .orange, word: "Tap to pair")
        case .removed: return Look(symbol: "minus.circle", tint: .red, word: "Removed")
        case .updateNeeded: return Look(symbol: "arrow.down.circle", tint: .orange, word: "Update Sill")
        case .offline(let just): return Look(symbol: "moon.zzz", tint: .tertiary, word: just ? notAnswering : "Not available")
        }
    }

    /// The hero's second line: where the computer is ("Here", "Away"), else its state's word.
    static func place(_ state: State) -> String {
        switch state {
        case .here: return "Here"
        case .away: return "Away"
        default: return look(state).word
        }
    }

    /// A tap on the card connects (rather than pairs, updates or tries again): the hero's capsule.
    static func connects(_ state: State) -> Bool {
        switch state {
        case .here, .away: return true
        default: return false
        }
    }

    /// The thumbnail is drawn without colour: the computer did not answer.
    static func desaturated(_ state: State) -> Bool {
        if case .offline = state { return true }
        return false
    }

    /// VoiceOver's one element per card: "KAMALELTAYEB, computer, Away, Connecting". The kind is
    /// "computer" until a host says which it is (every host is a "Mac" to this build).
    static func label(name: String, state: State, connecting: Bool) -> String {
        var parts = [name, "computer"]
        switch state {
        case .offline: parts.append("Not available")
        default: parts.append(look(state).word)
        }
        if connecting { parts.append("Connecting") }
        return parts.joined(separator: ", ")
    }

    /// What a tap does, for VoiceOver. `device` is this device's kind ("iPhone").
    static func hint(state: State, connecting: Bool, device: String) -> String {
        if connecting { return "Connecting. Cancel stops it." }
        switch state {
        case .here, .away: return "Connects to it."
        case .notPaired(let cable): return cable ? "Pairs over the USB cable, then connects." : "Pairs with a code it shows, then connects."
        case .removed: return "Pairs this \(device) with it again."
        case .updateNeeded: return "Its Sill is too old for this \(device)."
        case .offline: return "Tries to reach it again."
        }
    }

    // MARK: The hero

    /// A card the hero is picked from: its row's id, its state and when this device last had a
    /// session with it (SavedMac.lastConnectedAt, seconds; nil for never or an unsaved computer).
    struct Candidate: Equatable {
        let id: String
        let state: State
        let lastUsed: Double?
    }

    /// The hero: the most recently used computer a tap connects to (Here or Away), the first listed
    /// of those used at the same time; with none used, the first such; nil when none connects.
    static func hero(_ cards: [Candidate]) -> String? {
        var best: Candidate?
        for card in cards where connects(card.state) {
            guard let b = best else { best = card; continue }
            if (card.lastUsed ?? -.infinity) > (b.lastUsed ?? -.infinity) { best = card }
        }
        return best?.id
    }

    // MARK: Which card is dialling

    /// The computer a status line says is being dialled, and how ("Connecting to Mac mini remotely…"
    /// is the Mac mini's Remote row): StreamClient's dial lines and the ask's "Pairing with…". Nil for
    /// any other line. `route` is nil for an ask, which names a row at home of either kind.
    static func dialed(_ status: String) -> (name: String, route: Row.Route?)? {
        let ellipsis = "\u{2026}"
        for prefix in ["Connecting to ", "Reconnecting to ", "Pairing with "] where status.hasPrefix(prefix) && status.hasSuffix(ellipsis) {
            var rest = String(status.dropFirst(prefix.count).dropLast(ellipsis.count))
            if prefix == "Pairing with " {
                if rest.hasSuffix(" over the cable") { rest.removeLast(" over the cable".count) }
                return rest.isEmpty ? nil : (rest, nil)
            }
            var route = Row.Route.network
            if rest.hasSuffix(" remotely") { rest.removeLast(" remotely".count); route = .remote }
            else if rest.hasSuffix(" directly") { rest.removeLast(" directly".count); route = .direct }
            return rest.isEmpty ? nil : (rest, route)
        }
        return nil
    }

    /// Whether a card named `name` on `route` is the one `status` dials.
    static func isDialing(_ status: String, name: String, route: Row.Route) -> Bool {
        guard let d = dialed(status), d.name == name else { return false }
        return d.route.map { $0 == route } ?? (route != .remote)
    }

    // MARK: The banner

    /// An outcome important enough to stay at the top of the list until it is dismissed or a session
    /// comes: who removed this device, an update needed, a revoked or replaced key, the wrong computer,
    /// Remote Access off, Local Network refused. Its words are GoodbyePolicy's, RemoteCopy's and
    /// DiscoveryPolicy.HomeCopy's, split into a title and one line.
    struct Banner: Equatable {
        enum Kind: String, CaseIterable { case removed, revoked, wrongMac, newKey, remoteOff, updateNeeded, localNetwork }
        enum Action: Equatable { case pairAgain, update, openSettings }

        let kind: Kind
        /// The computer it is about, as the status line named it; "" for Local Network.
        let mac: String
        let title: String
        let line: String
        let action: Action?
        /// The status line it stands for: the screen does not show or speak that line again.
        let source: String

        var actionTitle: String? {
            switch action {
            case .pairAgain?: return "Pair Again"
            case .update?: return "Update"
            case .openSettings?: return "Open Settings"
            case nil: return nil
            }
        }

        /// What VoiceOver says as it appears.
        var announcement: String { "\(title). \(line)" }
    }

    /// The banner of `kind`. `storeAvailable`: Sill's App Store address is known (SillLinks.appStore),
    /// so Update has somewhere to go; `message`: the computer's own words for an update (GoodbyePolicy).
    static func banner(_ kind: Banner.Kind, mac: String, device: String, source: String,
                       storeAvailable: Bool = false, message: String? = nil) -> Banner {
        let title: String, line: String, action: Banner.Action?
        switch kind {
        case .removed:
            title = "\(mac) removed this \(device)"; line = "Pair again to use it."; action = .pairAgain
        case .revoked:
            title = "\(mac) no longer accepts this \(device)"; line = "Pair it again from Sill\u{2019}s Settings on the Mac."
            action = .pairAgain
        case .wrongMac:
            title = "This isn\u{2019}t the \(mac) this \(device) paired with"
            line = "If Sill was set up again on it, forget it here and pair again."; action = .pairAgain
        case .newKey:
            title = "\(mac) has a new key since this \(device) paired"; line = "Pair again to use it."; action = .pairAgain
        case .remoteOff:
            title = "\(mac) turned off Remote Access"; line = "Connect at home, or turn it on again in Sill on the Mac."
            action = nil
        case .updateNeeded:
            title = "Update Sill on this \(device)"
            line = message ?? "\(mac) needs a newer Sill on this \(device)."
            action = storeAvailable ? .update : nil
        case .localNetwork:
            title = "Allow Local Network"; line = "Sill needs it to find your computer."; action = .openSettings
        }
        return Banner(kind: kind, mac: mac, title: title, line: line, action: action, source: source)
    }

    /// The banner a session's goodbye raises (its reason, StreamProtocol's Goodbye): "removed",
    /// "remoteOff" and "update"; nil for every other reason, whose status line is enough.
    static func goodbyeBanner(reason: String, mac: String, device: String, source: String,
                              storeAvailable: Bool = false, message: String? = nil) -> Banner? {
        switch reason {
        case "removed": return banner(.removed, mac: mac, device: device, source: source)
        case "remoteOff": return banner(.remoteOff, mac: mac, device: device, source: source)
        case "update": return banner(.updateNeeded, mac: mac, device: device, source: source, storeAvailable: storeAvailable, message: message)
        default: return nil
        }
    }

    /// The banner a remote dial's failure raises (RemoteDialPolicy.Failure's raw value): revoked,
    /// wrongMac, remoteOff and localNetwork; nil for the rest, which a tap may yet get past.
    static func dialBanner(failure: String, mac: String, device: String, source: String) -> Banner? {
        switch failure {
        case "revoked": return banner(.revoked, mac: mac, device: device, source: source)
        case "wrongMac": return banner(.wrongMac, mac: mac, device: device, source: source)
        case "remoteOff": return banner(.remoteOff, mac: mac, device: device, source: source)
        case "localNetwork": return banner(.localNetwork, mac: mac, device: device, source: source)
        default: return nil
        }
    }
}
