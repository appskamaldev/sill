// iOSClient/TrackpadTaps.swift on its own: the trackpad's clicks and its drag (2026-11-27). A tap is a stroke
// whose fingers each stay within 3 pt of where they landed and whose last lift comes within 0.35 s of its first
// touch: one finger the left button, two the right. The one-finger tap's button goes down at the lift and comes up
// when its 0.3 s window closes untouched; a touch inside the window drags, the button held until that stroke
// ends. Moving the pointer never presses a button. Then 20,000 random strokes against a model written here.
//   swiftc -O iOSClient/TrackpadTaps.swift Tests/checks/trackpad-taps/main.swift -o check && ./check
import CoreGraphics
import Foundation

typealias T = TrackpadTaps
var fails = 0, passes = 0
func check(_ ok: Bool, _ name: String) { if ok { passes += 1; print("ok   \(name)") } else { fails += 1; print("FAIL \(name)") } }

let o = CGPoint(x: 100, y: 200)
func at(_ dx: CGFloat, _ dy: CGFloat) -> CGPoint { CGPoint(x: o.x + dx, y: o.y + dy) }

/// A one-finger stroke: lands at `t`, moves to each point, lifts `length` later.
func stroke(_ t: inout T, id: Int = 0, at time: Double, length: Double = 0.1, moves: [CGPoint] = [], from: CGPoint = o) -> (down: [T.Send], up: [T.Send]) {
    let d = t.down(id, at: from, time: time)
    for m in moves { t.moved(id, to: m) }
    let u = t.up(id, time: time + length)
    return (d, u)
}

// MARK: - The constants

check(T.travel == 3, "a tap's travel is 3 pt: \(T.travel)")
check(T.tapTime == 0.35, "a tap lifts within 0.35 s: \(T.tapTime)")
check(T.dragWindow == 0.3, "tap, then touch within 0.3 s, drags: \(T.dragWindow)")

// MARK: - One finger: the tap

do {
    var t = T()
    let s = stroke(&t, at: 1)
    check(s.down == [], "a finger landing sends nothing")
    check(s.up == [.leftDown], "a tap: the left button down at the lift")
    check(t.buttonDown && t.upAt == 1.1 + T.dragWindow, "and it waits for the drag window: \(String(describing: t.upAt))")
    check(t.expire(time: 1.2) == [], "the window still open: nothing")
    check(t.expire(time: 1.1 + T.dragWindow) == [.leftUp], "the window closes: the button up")
    check(!t.buttonDown && t.upAt == nil, "nothing held after it")
    check(t.expire(time: 9) == [], "a second expiry sends nothing")
}
do {
    var t = T()
    check(stroke(&t, at: 0, moves: [at(3, 0)]).up == [.leftDown], "3 pt of travel is still a tap")
    var u = T()
    check(stroke(&u, at: 0, moves: [at(3.01, 0)]).up == [], "past 3 pt it is no tap")
    var v = T()
    check(stroke(&v, at: 0, moves: [at(2.2, 2.2)]).up == [], "the distance counts, not each axis (2.2, 2.2 is 3.1 pt)")
    var w = T()
    check(stroke(&w, at: 0, moves: [at(10, 0), o]).up == [], "out and back is no tap: the farthest point counts")
    var x = T()
    check(stroke(&x, at: 0, moves: [at(1, 0), at(2, 1), at(-2, 2)]).up == [.leftDown], "a tremble within 3 pt is a tap")
}
do {
    var t = T()
    check(stroke(&t, at: 0, length: T.tapTime).up == [.leftDown], "lifted at 0.35 s: a tap")
    var u = T()
    check(stroke(&u, at: 0, length: T.tapTime + 0.01).up == [], "lifted after 0.35 s: no click")
    var v = T()
    check(stroke(&v, at: 0, length: 0.45).up == [], "a 0.45 s rest is no click and no drag (the old press and hold)")
}

// MARK: - Moving never presses a button

do {
    var t = T()
    _ = t.down(0, at: o, time: 0)
    t.moved(0, to: at(1, 0))
    // A rest before moving: the old press and hold put the button down here.
    t.moved(0, to: at(40, 0))
    check(!t.buttonDown && !t.dragging, "a rest, then a move: no button down")
    check(t.up(0, time: 2) == [], "and its lift sends nothing")
    var u = T()
    check(stroke(&u, at: 0, length: 0.1, moves: [at(5, 0)]).up == [], "a quick 5 pt nudge: no click (UIKit's tap took ~10 pt)")
    check(!u.buttonDown && u.upAt == nil, "and nothing waits")
}

// MARK: - Tap, then touch: the drag

do {
    var t = T()
    _ = stroke(&t, at: 0)                       // the tap: down at 0.1
    let d = t.down(0, at: at(200, 50), time: 0.3)
    check(d == [], "a touch in the window, anywhere: nothing more goes out, the button is already down")
    check(t.dragging && t.buttonDown && t.upAt == nil, "the stroke drags, the window is closed")
    t.moved(0, to: at(260, 80))
    check(t.buttonDown, "the button stays down through the moves")
    check(t.expire(time: 0.4 + T.dragWindow) == [], "the old window's expiry lets nothing up")
    check(t.up(0, time: 1.5) == [.leftUp], "the drag's lift: the button up, once")
    check(!t.buttonDown && !t.dragging && t.upAt == nil, "nothing held after the drag")
}
do {
    var t = T()
    _ = stroke(&t, at: 0)                       // lifts at 0.1, the window to 0.4
    check(t.down(0, at: o, time: 0.1 + T.dragWindow) == [] && t.dragging, "a touch at the window's edge still drags")
    _ = t.up(0, time: 1)
    var u = T()
    _ = stroke(&u, at: 0)
    check(u.down(0, at: o, time: 0.1 + T.dragWindow + 0.001) == [.leftUp], "after the window: the tap's button up first")
    check(!u.dragging && !u.buttonDown, "and the stroke is its own: no drag")
    u.moved(0, to: at(50, 0))
    check(u.up(0, time: 1) == [], "it moves the pointer only")
}
do {
    var t = T()
    _ = stroke(&t, at: 0)
    _ = t.expire(time: 0.4)
    check(t.down(0, at: o, time: 0.41) == [] && !t.dragging, "a touch after the window closed by its timer: no drag, nothing to let up")
}
do {
    // Two taps: a double-click. The second stroke is a tap itself: its button up, then down again.
    var t = T()
    var sent: [T.Send] = []
    let a = stroke(&t, at: 0); sent += a.down + a.up
    let b = stroke(&t, at: 0.25); sent += b.down + b.up
    check(sent == [.leftDown, .leftUp, .leftDown], "two taps: down, up, down again: \(sent)")
    sent += t.expire(time: 0.35 + T.dragWindow)
    check(sent == [.leftDown, .leftUp, .leftDown, .leftUp], "a double-click, every down with its up")
    // Then a third: a triple-click.
    var u = T()
    var all: [T.Send] = []
    for i in 0..<3 { let s = stroke(&u, at: Double(i) * 0.25); all += s.down + s.up }
    all += u.expire(time: 100)
    check(all == [.leftDown, .leftUp, .leftDown, .leftUp, .leftDown, .leftUp], "three taps: three clicks: \(all)")
}
do {
    // Tap, then a touch that rests and lifts slowly: the drag ends, nothing more.
    var t = T()
    _ = stroke(&t, at: 0)
    check(stroke(&t, at: 0.2, length: 1).up == [.leftUp], "tap, then a long rest: the button up at the lift, no second click")
    check(t.upAt == nil && !t.buttonDown, "nothing waits after it")
}
do {
    // A drag cancelled by the system: the button comes up.
    var t = T()
    _ = stroke(&t, at: 0)
    _ = t.down(0, at: o, time: 0.2)
    t.moved(0, to: at(30, 0))
    check(t.cancelled(0) == [.leftUp], "a drag cancelled: the button up")
    check(t.up(0, time: 1) == [], "a lift after the cancel: nothing")
    var u = T()
    _ = u.down(0, at: o, time: 0)
    check(u.cancelled(0) == [] && u.up(0, time: 0.05) == [], "a cancelled touch is no tap")
    var v = T()
    _ = v.down(0, at: o, time: 0)
    _ = v.down(1, at: at(40, 0), time: 0.01)
    check(v.cancelled(0) == [] && v.up(1, time: 0.1) == [], "two fingers, one cancelled: no click when the other lifts")
}
do {
    // `reset` (the pad leaving its window): a held button comes up.
    var t = T()
    _ = stroke(&t, at: 0)
    check(t.reset() == [.leftUp] && !t.buttonDown && t.upAt == nil, "reset in the window: the button up")
    check(t.reset() == [], "reset with nothing held: nothing")
    var u = T()
    _ = stroke(&u, at: 0)
    _ = u.down(0, at: o, time: 0.2)
    check(u.reset() == [.leftUp], "reset mid-drag: the button up")
}

// MARK: - Two fingers: the right click

do {
    var t = T()
    _ = t.down(0, at: o, time: 0)
    _ = t.down(1, at: at(40, 0), time: 0.02)
    check(t.up(0, time: 0.1) == [], "the first finger of two lifting: nothing yet")
    check(t.up(1, time: 0.12) == [.rightClick], "two fingers tap: a right click at the last lift")
    check(!t.buttonDown && t.upAt == nil, "a right click leaves no window: no drag after it")
}
do {
    var t = T()
    _ = t.down(0, at: o, time: 0)
    _ = t.down(1, at: at(40, 0), time: 0.01)
    t.moved(1, to: at(43.5, 0))
    _ = t.up(0, time: 0.1)
    check(t.up(1, time: 0.1) == [], "two fingers, one past 3 pt: no right click (a scroll)")
    var u = T()
    _ = u.down(0, at: o, time: 0)
    _ = u.down(1, at: at(40, 0), time: 0.01)
    u.moved(1, to: at(43, 0))
    _ = u.up(0, time: 0.1)
    check(u.up(1, time: 0.1) == [.rightClick], "two fingers within 3 pt each: a right click")
    var v = T()
    _ = v.down(0, at: o, time: 0)
    _ = v.down(1, at: at(40, 0), time: 0.01)
    _ = v.up(0, time: 0.3)
    check(v.up(1, time: 0.36) == [], "two fingers lifting after 0.35 s: no right click")
}
do {
    // Tap, then a two-finger tap in the window: the left button up, then the right click.
    var t = T()
    _ = stroke(&t, at: 0)
    _ = t.down(0, at: o, time: 0.2)
    _ = t.down(1, at: at(30, 0), time: 0.21)
    _ = t.up(0, time: 0.3)
    check(t.up(1, time: 0.31) == [.leftUp, .rightClick], "tap, then two fingers tap: left up, right click")
}

// MARK: - Three fingers: a gesture's, never a click

do {
    var t = T()
    for i in 0..<3 { _ = t.down(i, at: at(CGFloat(i) * 30, 0), time: 0.01 * Double(i)) }
    var sent: [T.Send] = []
    for i in 0..<3 { sent += t.up(i, time: 0.1) }
    check(sent == [], "three fingers tapping: no click: \(sent)")
}

// MARK: - Random strokes against a model

struct Rng { var s: UInt64; mutating func next() -> UInt64 { s = s &* 6364136223846793005 &+ 1442695040888963407; return s >> 33 }
             mutating func unit() -> Double { Double(next() % 1_000_000) / 1_000_000 } }
var rng = Rng(s: 427)
var wrong = 0, stuck = 0, doubleDown = 0, orphanUp = 0, taps = 0, drags = 0, rights = 0, plain = 0
var t = T()
var now = 0.0
var down = false            // the button as the Mac sees it
var modelUpAt: Double?      // the model's open window
for _ in 0..<20_000 {
    now += 0.05 + rng.unit() * 0.6
    // The timer may or may not fire before this stroke.
    if rng.next() % 2 == 0, let u = t.upAt, u <= now {
        for s in t.expire(time: u) { if s == .leftUp { if !down { orphanUp += 1 }; down = false } }
        modelUpAt = nil
    }
    let fingers = [1, 1, 1, 2, 2, 3][Int(rng.next() % 6)]
    let length = rng.unit() * 0.6
    let travel = CGFloat(rng.unit() * 8)
    let cancel = rng.next() % 20 == 0
    // The model.
    let dragStroke = modelUpAt.map { now <= $0 } ?? false
    let isTap = !cancel && travel <= T.travel && length <= T.tapTime
    var expected: [T.Send] = []
    if let u = modelUpAt, now > u { expected.append(.leftUp) }
    if dragStroke { expected.append(.leftUp) }
    if isTap && fingers == 1 { expected.append(.leftDown) }
    if isTap && fingers == 2 { expected.append(.rightClick) }
    // The stroke.
    var sent: [T.Send] = []
    for i in 0..<fingers { sent += t.down(i, at: o, time: now) }
    for i in 0..<fingers { t.moved(i, to: at(travel, 0)) }
    for i in 0..<fingers { sent += cancel ? t.cancelled(i) : t.up(i, time: now + length) }
    for s in sent {
        switch s {
        case .leftDown: if down { doubleDown += 1 }; down = true
        case .leftUp: if !down { orphanUp += 1 }; down = false
        case .rightClick: if down { wrong += 1 }
        }
    }
    if sent != expected { wrong += 1 }
    if dragStroke { drags += 1 } else if isTap && fingers == 1 { taps += 1 } else if isTap && fingers == 2 { rights += 1 } else { plain += 1 }
    now += length
    modelUpAt = isTap && fingers == 1 ? now + T.dragWindow : nil
    if t.upAt != modelUpAt { wrong += 1 }
}
now += 10
for s in t.expire(time: now) where s == .leftUp { down = false }
if down || t.buttonDown { stuck += 1 }
check(wrong == 0, "every random stroke as the model says: \(wrong) wrong")
check(doubleDown == 0 && orphanUp == 0, "never two downs in a row, never an up without a down: \(doubleDown), \(orphanUp)")
check(stuck == 0, "no button left down at the end")
check(taps > 1000 && drags > 500 && rights > 500 && plain > 1000,
      "the random strokes reach every end: \(taps) taps, \(drags) drags, \(rights) right clicks, \(plain) neither")

print("\(passes) passed, \(fails) failed")
exit(fails == 0 ? 0 : 1)
