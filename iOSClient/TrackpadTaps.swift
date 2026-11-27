import CoreGraphics
import Foundation

/// The trackpad's clicks and its drag (TrackpadSurface): which stroke on the pad presses a button, and when the
/// button comes up. Moving the pointer never presses one; only a tap does, and a tap, then a touch, drags.
///
/// - A tap is a stroke whose every finger stays within `travel` of where it landed and whose last finger lifts
///   within `tapTime` of its first touch. One finger is the left button, two the right (a click at the lift).
///   Three or more are a gesture's (TrackpadGestures), never a click.
/// - Tap, then touch again within `dragWindow` of the tap's lift, anywhere on the pad, and the stroke drags: the
///   left button stays down while the finger moves and comes up as the stroke ends, as on a Mac's or a Windows
///   trackpad.
/// - The first tap's click is not sent and then followed by a second button down: the host counts a down that
///   soon after a click as a double-click (InputInjector's click state), and a double-click-drag selects by word
///   and zooms a window by its title bar. So, as a Mac's own trackpad does it, the tap's button goes down at its
///   lift and comes up when the window closes untouched (`expire`), or stays down for the drag that follows. A
///   tap within the window is the next click of the same run: its button comes up and goes down again, so two
///   taps are a double-click.
///
/// Before this (2026-11-27), a 0.45 s press and hold put the button down, so a finger resting a moment before
/// it moved dragged, and UIKit's tap took about 10 pt of travel, so a short nudge also clicked.
///
/// Pure: CoreGraphics and Foundation only, checked in Tests/checks/trackpad-taps. The pad's stroke observer feeds
/// it each touch with the time; the pad presses the latched modifiers around each button and sends the rest.
struct TrackpadTaps {
    /// How far, in points, a finger of a tap may move from where it landed.
    static let travel: CGFloat = 3
    /// How long, in seconds, from a tap's first touch to its last lift.
    static let tapTime: TimeInterval = 0.35
    /// How long, in seconds, after a one-finger tap's lift a touch starts a drag.
    static let dragWindow: TimeInterval = 0.3

    /// What goes to the Mac, at the pad's cursor.
    enum Send: Equatable {
        case leftDown, leftUp, rightClick
    }

    /// The left button is down: a tap's, waiting for its window, or a drag's.
    private(set) var buttonDown = false
    /// The stroke under way drags (it began within a tap's window).
    private(set) var dragging = false
    /// When the tap's button comes up unless a touch comes first.
    private(set) var upAt: TimeInterval?

    private var landed: [Int: CGPoint] = [:]
    private var strokeStart: TimeInterval?
    private var touches = 0
    private var moved = false
    private var spoiled = false

    /// A finger lands. The first of a stroke inside a tap's window starts a drag (nothing goes out: the tap's
    /// button is down already); after the window, the tap's button comes up first.
    mutating func down(_ id: Int, at point: CGPoint, time: TimeInterval) -> [Send] {
        var out: [Send] = []
        if landed.isEmpty {
            out = expire(time: time, strictly: true)
            if upAt != nil {
                upAt = nil
                dragging = true
            }
            strokeStart = time
            touches = 0
            moved = false
            spoiled = false
        }
        touches += 1
        landed[id] = point
        return out
    }

    mutating func moved(_ id: Int, to point: CGPoint) {
        guard let from = landed[id] else { return }
        if hypot(point.x - from.x, point.y - from.y) > Self.travel { moved = true }
    }

    /// A finger lifts; the stroke's last decides.
    mutating func up(_ id: Int, time: TimeInterval) -> [Send] {
        guard landed.removeValue(forKey: id) != nil, landed.isEmpty, let start = strokeStart else { return [] }
        strokeStart = nil
        let tap = !moved && !spoiled && time - start <= Self.tapTime
        var out: [Send] = []
        if dragging {
            dragging = false
            buttonDown = false
            out.append(.leftUp)
        }
        guard tap else { return out }
        switch touches {
        case 1:
            out.append(.leftDown)
            buttonDown = true
            upAt = time + Self.dragWindow
        case 2:
            out.append(.rightClick)
        default:
            break
        }
        return out
    }

    /// A finger was taken away (the system took the touch): the stroke is no tap, and a drag ends at its end.
    mutating func cancelled(_ id: Int) -> [Send] {
        guard landed.removeValue(forKey: id) != nil else { return [] }
        spoiled = true
        guard landed.isEmpty else { return [] }
        strokeStart = nil
        guard dragging else { return [] }
        dragging = false
        buttonDown = false
        return [.leftUp]
    }

    /// The tap's window closed with no touch: its button comes up. The pad calls it at `upAt`.
    mutating func expire(time: TimeInterval) -> [Send] { expire(time: time, strictly: false) }

    /// At `upAt` itself a timer closes the window, and a touch still drags.
    private mutating func expire(time: TimeInterval, strictly: Bool) -> [Send] {
        guard let upAt, strictly ? time > upAt : time >= upAt else { return [] }
        self.upAt = nil
        buttonDown = false
        return [.leftUp]
    }

    /// The pad is going away: a button held comes up, and nothing is remembered.
    mutating func reset() -> [Send] {
        let out: [Send] = buttonDown ? [.leftUp] : []
        self = TrackpadTaps()
        return out
    }
}
