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

**Trackpad in Full Screen: a pointer always shows (2026-11-27, same branch).** Kamal, on the Duo (build 5): with
the switch on, no pointer in full screen; the laptop pose's pad shows one.

- **Why, from the code.** The sprite (PointerPresence.sprite) drew the pad's pointer only while this device had
  the pointer and the pad was the last thing to set it (`origin == .trackpad`). Otherwise it drew the Mac's, and
  only while the Mac's was over the stream. Entering full screen with the Mac's pointer outside the streamed
  picture (where it usually is after the Mac's own mouse, or a window stream), or after a key, typing or a hover
  cleared this device's own pointer, nothing showed until the pad moved it. Shown in the agents' simulator
  (live, a synthetic host): hidden at runtime with no input since, no sprite; with a pad move, one. On the Duo
  itself the cause is not confirmed: see the report of 2026-11-27.
- **What it does now.** While the picture is the trackpad (`FullScreenPolicy.pictureIsTrackpad`, told to the
  client by StreamScreen's `setPictureIsTrackpad`), `PointerPresence.padOverPicture` keeps a pointer drawn: the
  Mac's while the Mac has it over the stream, else where the pad last put it (`padLast`, kept when a key or typing
  clears `own`, forgotten with the session), else the Mac's last place, else the middle (where the pad's cursor
  starts). The shape is the Mac's (kind 14), as always. Off the picture, nothing changes.
- **Verified.** `Tests/checks/pointer-presence/run.sh` (178 checks) and `--mutants` (49 of 49, six new); the
  simulator: hidden with no input, the arrow in the middle of the picture. Not run: the Duo's own glass.

**Trackpad in Full Screen: the picture's fingers are the pad's whatever the stacking (2026-11-27, same branch).**
Kamal, on the Duo (builds 6 and 7, iOS 27.3), open flat and closed: with the switch on, full screen was still direct
touch, the Mac's pointer jumping to the finger and a touch clicking there.

- **What was ruled out, with evidence.** One key (`Sill.trackpadInFullScreen`) written by the Settings row and read
  by both panes, through `UserDefaults.standard` (no other store, no idiom or size gate); both picture panes
  (StreamScreen's content area and PortraitStreamScreen's, every layout goes through one) lay the pad. In the agents'
  Duo simulator the pad is on top and takes the touch: a live session with the synthetic host, sideways, a real
  touch event's view the pad (switch off: the input view); the harness's cover upright and sideways, `hitTest`
  the pad; the switch turned on at run time, then full screen, the pad.
- **What the device says.** Build 5 drew no pointer at all, and a stroke on the pad draws one: the pad's fingers
  never reached it there. So on the glass the input view under the pad still took the touches, which the simulator
  never shows; the stacking of two sibling SwiftUI representables, one put in later, is the only difference left,
  and was not seen directly.
- **What it does now.** The decision no longer rests on the stacking: `FullScreenPolicy.surface(for:padOverPicture:)`,
  asked by both surfaces' `point(inside:with:)`. With the pad over the picture, touches and hovering are the pad's
  (the input view refuses them, `InputOverlayView.padOverPicture`), so nothing puts the pointer where a finger is;
  a scroll and a pinch stay the input view's. Hovering changed: an iPad's pointer or Pencil hovering over the
  picture no longer moves the Mac's pointer while the switch is on (before, it fell through).
- **Verified.** full-screen check (101,652 checks; 75 of 75 mutants, four new), pointer-presence, trackpad-taps,
  repo-rules; the simulator (both switch states). Not run: the touch rig (it makes a simulator of its own), the
  Duo's and the iPad's glass.

**Use Picture as Trackpad: every layout, the controls shown or hidden (2026-11-27, same branch, for build 9).**
Kamal, on the Duo (build 8): it worked as built, a trackpad only in Full Screen; he had tested the cover and open
flat with the bar shown, and wants the switch to hold everywhere.

- **What it does now.** Settings › This iPhone (This iPad) › **Use Picture as Trackpad** (was "Trackpad in Full
  Screen"; the key is still `Sill.trackpadInFullScreen`, so the choice carries over; off unless turned on). On, the
  picture is the relative trackpad in every layout whose picture takes touches, the bar shown or in full screen:
  `FullScreenPolicy.pictureIsTrackpad(switchOn:)` (no `hidden` any more), read by both picture panes as before, so
  `surface(for:padOverPicture:)` and the always-drawn pointer (`PointerPresence.padOverPicture`) hold in every
  layout. The laptop pose keeps its pad below the fold; its picture above follows the switch too. Off: direct
  touch, as always. The footnote: "The whole picture works as a trackpad, with the controls shown or in full
  screen: …". No tour card names the switch, so the tour's words are unchanged.
- **The cost.** With the switch on the pad takes the pinch, so the picture's own zoom is not offered (it resets as
  the switch comes on), now with the bar shown too; an iPad's pointer or Pencil hovering over the picture moves
  nothing.
- **Verified.** `Tests/checks/run-all.sh full-screen pointer-presence`; full-screen's mutants. Not run: the glass.
