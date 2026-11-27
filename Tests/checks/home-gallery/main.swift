// iOSClient/HomeGallery.swift on its own: the connect screen's gallery (2026-11-27). Each card state's icon,
// colour and word, VoiceOver's label and hint, the hero, the card a dial line names, and the banners.
//   swiftc -O iOSClient/HomeGallery.swift Tests/checks/home-gallery/main.swift -o check && ./check
import Foundation

typealias G = HomeGallery
var fails = 0, passes = 0
func check(_ ok: Bool, _ name: String) { if ok { passes += 1; print("ok   \(name)") } else { fails += 1; print("FAIL \(name)") } }

func row(_ route: G.Row.Route, _ word: G.Row.Word, saved: Bool = true) -> G.Row { G.Row(route: route, word: word, saved: saved) }

// MARK: - A row's state

check(G.state(row(.network, .method(.wifi))) == .here(.wifi), "a paired Wi-Fi row is Here, Wi-Fi")
check(G.state(row(.network, .method(.wired))) == .here(.wired), "a paired wired row is Here, Wired")
check(G.state(row(.direct, .method(.direct))) == .here(.direct), "a Direct row is Here, Direct")
check(G.state(row(.network, .method(.unknown))) == .here(.unknown), "a row whose interfaces do not say is Here")
check(G.state(row(.remote, .method(.unknown))) == .away, "a Remote row is Away")
check(G.state(row(.remote, .notPaired)) == .away, "a Remote row is Away whatever its home word")
check(G.state(row(.network, .notPaired, saved: false)) == .notPaired(cable: false), "an unsaved row that asks is Tap to pair")
check(G.state(row(.network, .openDoor, saved: false)) == .notPaired(cable: false), "an open door's row is Tap to pair")
check(G.state(row(.network, .pairsOverCable, saved: false)) == .notPaired(cable: true), "the cable's unpaired row is Tap to pair, over the cable")
check(G.state(row(.network, .notPaired, saved: true)) == .removed, "a saved row that reads Not paired is Removed")
check(G.state(row(.network, .pairsOverCable, saved: true)) == .removed, "a saved row over the cable that reads Not paired is Removed")
check(G.state(row(.network, .updateSill)) == .updateNeeded, "Update Sill")
check(G.state(row(.remote, .method(.unknown)), unanswered: true) == .offline(justFailed: false), "an unanswered row is offline")
check(G.state(row(.network, .method(.wifi)), unanswered: true, justFailed: true) == .offline(justFailed: true), "the latest tap's unanswered row")

// MARK: - Icon, colour and word

let all: [G.State] = [.here(.wired), .here(.wifi), .here(.direct), .here(.unknown), .away, .notPaired(cable: false),
                      .notPaired(cable: true), .removed, .updateNeeded, .offline(justFailed: false), .offline(justFailed: true)]
for s in all {
    let l = G.look(s)
    check(l.symbol != nil && !(l.symbol ?? "").isEmpty && !l.word.isEmpty, "\(s): an icon and a word beside its colour")
    check(!l.word.contains("Remote") && l.word != l.word.uppercased(), "\(s): never Remote, never in capitals: \(l.word)")
}
func look(_ s: G.State, _ symbol: String, _ tint: G.Tint, _ word: String) {
    check(G.look(s) == G.Look(symbol: symbol, tint: tint, word: word), "\(s): \(symbol), \(tint), \(word): \(G.look(s))")
}
look(.here(.wired), "cable.connector", .green, "Wired")
look(.here(.wifi), "wifi", .green, "Wi\u{2011}Fi")
look(.here(.direct), "antenna.radiowaves.left.and.right", .green, "Direct")
look(.here(.unknown), "network", .green, "Here")
look(.away, "globe", .blue, "Away")
look(.notPaired(cable: false), "key", .orange, "Tap to pair")
look(.notPaired(cable: true), "cable.connector", .orange, "Tap to pair")
look(.removed, "minus.circle", .red, "Removed")
look(.updateNeeded, "arrow.down.circle", .orange, "Update Sill")
look(.offline(justFailed: false), "moon.zzz", .tertiary, "Not available")
look(.offline(justFailed: true), "moon.zzz", .tertiary, "Not answering. Tap to try again.")
for s in all {
    check(G.look(s, connecting: true) == G.Look(symbol: nil, tint: .accent, word: "Connecting\u{2026}"), "\(s) connecting: a ProgressView, the accent, Connecting…")
}
check(G.place(.here(.wired)) == "Here" && G.place(.away) == "Away" && G.place(.removed) == "Removed", "the hero's place: Here, Away, else the word")
check(G.desaturated(.offline(justFailed: true)) && G.desaturated(.offline(justFailed: false)) && !G.desaturated(.away), "only an offline card's picture loses its colour")
check(G.connects(.here(.wifi)) && G.connects(.away) && !G.connects(.removed) && !G.connects(.notPaired(cable: false))
      && !G.connects(.offline(justFailed: false)) && !G.connects(.updateNeeded), "Here and Away connect; nothing else does")

// MARK: - VoiceOver

check(G.label(name: "KAMALELTAYEB", state: .away, connecting: true) == "KAMALELTAYEB, computer, Away, Connecting", "the spec's label")
check(G.label(name: "Mac mini", state: .here(.wifi), connecting: false) == "Mac mini, computer, Wi\u{2011}Fi", "a Here card's label")
check(G.label(name: "Mac mini", state: .offline(justFailed: true), connecting: false) == "Mac mini, computer, Not available", "an offline card's label is short")
for s in all { check(!G.hint(state: s, connecting: false, device: "iPhone").isEmpty, "\(s): a hint says what a tap does") }
check(G.hint(state: .away, connecting: true, device: "iPhone").contains("Cancel"), "connecting: the hint names Cancel")
check(G.hint(state: .removed, connecting: false, device: "iPad") == "Pairs this iPad with it again.", "removed: pairs this device again")

// MARK: - The hero

func c(_ id: String, _ s: G.State, _ t: Double?) -> G.Candidate { G.Candidate(id: id, state: s, lastUsed: t) }
check(G.hero([]) == nil, "no cards, no hero")
check(G.hero([c("a", .notPaired(cable: false), 9), c("b", .removed, 9), c("c", .offline(justFailed: false), 9), c("d", .updateNeeded, 9)]) == nil,
      "no hero that a tap does not connect to")
check(G.hero([c("a", .here(.wifi), 1), c("b", .away, 5), c("c", .here(.wired), 3)]) == "b", "the most recently used")
check(G.hero([c("a", .here(.wifi), nil), c("b", .away, nil)]) == "a", "none used: the first listed")
check(G.hero([c("a", .here(.wifi), 4), c("b", .away, 4)]) == "a", "a tie: the first listed")
check(G.hero([c("a", .here(.wifi), nil), c("b", .away, 1)]) == "b", "used beats never used")
check(G.hero([c("a", .offline(justFailed: true), 9), c("b", .away, 1)]) == "b", "an offline computer is passed over however recent")

// MARK: - Dial lines

check(G.dialed("Connecting to Mac mini remotely\u{2026}").map { $0.name == "Mac mini" && $0.route == .remote } == true, "remotely: the Remote row")
check(G.dialed("Reconnecting to Mac mini remotely\u{2026}").map { $0.name == "Mac mini" && $0.route == .remote } == true, "reconnecting remotely")
check(G.dialed("Connecting to Mac mini directly\u{2026}").map { $0.name == "Mac mini" && $0.route == .direct } == true, "directly: the Direct row")
check(G.dialed("Connecting to Mac mini\u{2026}").map { $0.name == "Mac mini" && $0.route == .network } == true, "plain: the network row")
check(G.dialed("Pairing with Mac mini over the cable\u{2026}").map { $0.name == "Mac mini" && $0.route == nil } == true, "the ask over the cable")
check(G.dialed("Pairing with Mac mini\u{2026}").map { $0.name == "Mac mini" && $0.route == nil } == true, "the ask")
check(G.dialed("Looking for computers on this network") == nil, "an idle line names nothing")
check(G.dialed("Mac mini disconnected. Sill will reconnect when it can reach it.") == nil, "a loss names nothing")
check(G.dialed("Connecting to \u{2026}") == nil, "no name, nothing")
check(G.isDialing("Connecting to Mac mini remotely\u{2026}", name: "Mac mini", route: .remote), "the Remote card dials")
check(!G.isDialing("Connecting to Mac mini remotely\u{2026}", name: "Mac mini", route: .network), "not its network card")
check(!G.isDialing("Connecting to Mac mini remotely\u{2026}", name: "Mac mini (2)", route: .remote), "not another name")
check(G.isDialing("Pairing with Mac mini\u{2026}", name: "Mac mini", route: .network)
      && G.isDialing("Pairing with Mac mini\u{2026}", name: "Mac mini", route: .direct)
      && !G.isDialing("Pairing with Mac mini\u{2026}", name: "Mac mini", route: .remote), "the ask: a row at home, never a Remote one")

// MARK: - Banners

let device = "iPhone"
for reason in ["removed", "remoteOff", "update"] {
    check(G.goodbyeBanner(reason: reason, mac: "M", device: device, source: "s") != nil, "goodbye \(reason): a banner")
}
for reason in ["quit", "busy", "internetOff", "pairingRequired", "", "pairAgain"] {
    check(G.goodbyeBanner(reason: reason, mac: "M", device: device, source: "s") == nil, "goodbye \(reason.isEmpty ? "(unreadable)" : reason): the status line only")
}
for failure in ["revoked", "wrongMac", "remoteOff", "localNetwork"] {
    check(G.dialBanner(failure: failure, mac: "M", device: device, source: "s") != nil, "dial failure \(failure): a banner")
}
for failure in ["busy", "refused", "vpnOff", "nameNotFound", "notSill", "noAnswer"] {
    check(G.dialBanner(failure: failure, mac: "M", device: device, source: "s") == nil, "dial failure \(failure): no banner")
}
let removed = G.goodbyeBanner(reason: "removed", mac: "KAMALELTAYEB", device: device, source: "KAMALELTAYEB removed this iPhone. To use it again, pair it again.")!
check(removed.title == "KAMALELTAYEB removed this iPhone", "removed: the mockup's title: \(removed.title)")
check(removed.line == "Pair again to use it." && removed.actionTitle == "Pair Again", "removed: its line and Pair Again")
check(removed.source == "KAMALELTAYEB removed this iPhone. To use it again, pair it again.", "the status line it stands for")
check(removed.announcement == "KAMALELTAYEB removed this iPhone. Pair again to use it.", "what VoiceOver says")
let update = G.goodbyeBanner(reason: "update", mac: "Mac mini", device: device, source: "s", storeAvailable: true, message: "Update Sill on your iPhone to keep using Mac mini.")!
check(update.action == .update && update.actionTitle == "Update", "update with the App Store: Update")
check(update.line == "Update Sill on your iPhone to keep using Mac mini.", "update: the computer's own words")
check(G.goodbyeBanner(reason: "update", mac: "Mac mini", device: device, source: "s")!.action == nil, "update without the App Store's address: no button")
check(G.banner(.localNetwork, mac: "", device: device, source: "s").action == .openSettings, "Local Network: Open Settings")
check(G.banner(.localNetwork, mac: "", device: device, source: "s").actionTitle == "Open Settings", "Open Settings' words")
check(G.banner(.remoteOff, mac: "M", device: device, source: "s").action == nil, "Remote Access off: nothing to press here")
for kind in [G.Banner.Kind.removed, .revoked, .wrongMac, .newKey] {
    check(G.banner(kind, mac: "M", device: device, source: "s").action == .pairAgain, "\(kind): Pair Again")
}
for kind in G.Banner.Kind.allCases {
    let b = G.banner(kind, mac: "Mac mini", device: device, source: "s")
    check(!b.title.isEmpty && !b.line.isEmpty && !b.title.hasSuffix(".") && b.line.hasSuffix("."), "\(kind): a title and one line: \(b.title) / \(b.line)")
    check(!b.line.contains("\n") && !b.title.contains("\n"), "\(kind): no line breaks")
}

print("home-gallery: \(passes) of \(passes + fails) checks passed")
exit(fails == 0 ? 0 : 1)
