**The trackpad's clicks and drag: moving never clicks; tap, then touch again, drags (2026-11-27, branch `task/duo-trackpad` from main at 22ab5a3).**
On an iPhone Duo in the laptop pose, moving the pointer on the lower half's trackpad also clicked or dragged on the
Mac. Client only, no wire change: the pad sends the same kind 8 pointer events it always did.

- **The cause, from the code (TrackpadView.swift at 22ab5a3).** Two recognizers pressed buttons while the finger was
  only moving the pointer. (a) The press-and-hold drag, a `UILongPressGestureRecognizer` with `minimumPressDuration`
  0.45 and UIKit's default `allowableMovement` (10 pt): a finger that rested 0.45 s within 10 pt before moving sent
  `leftDown`, and the tracker's moves after it went to the Mac as a drag (InputInjector makes a move with the left
  button down a `leftMouseDragged`). The tracker moves the pointer from 2 pt, so a slow start inside those 10 pt both
  moved the pointer and armed the drag. (b) The one-finger `UITapGestureRecognizer` takes about 10 pt of travel, runs
  beside the tracker (the pad's delegate lets every recognizer run together) and the pan begins only ~10 pt in, so a
  short nudge moved the pointer and then clicked at the lift.
- **What it does now (Kamal, 2026-11-27).** `TrackpadTaps` (pure, `Tests/checks/trackpad-taps`) decides from the
  stroke observer's touches (`StrokeObserver.onTouch`, already on the pad): a tap is a stroke whose every finger
  stays within 3 pt of where it landed and whose last finger lifts within 0.35 s of its first touch; one finger the
  left button, two the right. Tap, then touch again within 0.3 s of the tap's lift, anywhere, and the stroke drags:
  the left button stays down while it moves and comes up at its lift. The press and hold and both tap recognizers
  are gone. Scrolling, its phases and coast, three- and four-finger gestures, the latched modifiers and the haptics
  are as they were (a firm tick as a drag starts, a soft one as it ends).
- **The first tap's click.** Not sent at once and then followed by the drag's button down: the host counts a down
  that soon after a click at the same place as a double-click (`mouseEventClickState`), and a double-click-drag
  selects by word, and on a title bar zooms the window. As a Mac's trackpad does, the tap's button goes down at its
  lift and comes up when the 0.3 s window closes untouched, or stays down for the drag. Two taps are a double-click
  (the second's lift lets the button up and presses it again). The cost: a click's button comes up 0.3 s after the
  lift, so what acts on the up (most buttons) acts 0.3 s later than before. A three-finger gesture begun inside a
  tap's window is the drag's, not a gesture (no stroke goes silent while a button is down).
- **Words.** The pad's hint: "Drag to move the pointer. Tap to click; tap, then drag to drag. Two fingers scroll."
  The tour's laptop card: "Tap, then touch the trackpad again and drag to move a window or select text."
- **Not changed.** The picture's own touch and hold in Direct Touch (TouchHold, Hold, Then Drag) is another surface.
  Upstream's `ready-222` (draft PR #398, Touch Mode) lays this same `TrackpadSurface` over the picture, so it takes
  these clicks too; it edits TrackpadView.swift around the removed recognizers (its `allowedTouchTypes` loop names
  `tap`, `twoFingerTap` and `longPress`), which a merge resolves by dropping those three names.
- **Verified.** `Tests/checks/run-all.sh trackpad-taps tour gestures pointer-presence touch-hold repo-rules`: all
  passed (trackpad-taps 57 checks, 18 of 18 mutants caught). The iOS app builds for the simulator (Debug). Not run:
  the touch rig, harness photos, a real device.

**Trackpad in Full Screen (2026-11-27, branch `task/duo-trackpad-fullscreen`, on top of the above).**
Kamal: in full screen, an option where the whole picture is the relative trackpad, off by default.

- **What it does.** Settings › This iPhone (This iPad) › Trackpad in Full Screen (`Sill.trackpadInFullScreen`, off
  unless turned on, never sent), the first of the rows about touch. While the controls are hidden and it is on
  (`FullScreenPolicy.pictureIsTrackpad`), StreamScreen's and PortraitStreamScreen's picture panes lay the portrait
  pad's own `TrackpadView` over the input view (`overPicture`): the same surface, clicks, drag, scroll, gestures,
  latch and haptics; the pointer moves from where it is. This device's pointer shows meanwhile
  (`showsOwnPointer`), and the zoom goes back to the whole picture as the pad comes (the pad takes the pinch).
  Shown, the picture is direct touch as always; the laptop pose's lower pad is unchanged.
- **Taken from upstream's `ready-222` (draft PR #398, Touch Mode), written the same way so the two merge:**
  `FingerCounter` only watches (`WatchesTouchesOnly`), so the pad never waits for the swipe in and stops
  answering; the tracker counts a held stroke's travel from its landing (`EdgeRecognizers.heldLanding`); the events
  with no touch (hover, scroll, transform) fall through to the picture. Not taken: #222's Pencil and pointer-click
  hand-off (`TouchMode.taker`, `takePointerTouch`): over the picture with the switch on, an iPad's Pencil is a
  finger on the pad. #222's Touch Mode is the general form (every layout, a segmented control); when it lands this
  switch either goes or becomes "Trackpad mode only in full screen", Noah's call.
- **Verified.** full-screen check (its new lines and four mutants), the iOS build for the simulator. In the agents'
  Duo simulator: see the report of 2026-11-27.
