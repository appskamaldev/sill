# `trackpad-taps`

**Checks:** the trackpad's clicks and its drag (TrackpadTaps, 2026-11-27): the constants (3 pt, 0.35 s, 0.3 s); a one-finger tap at, under and over its travel and its time, each finger's travel on its own, a cancel; the tap's left button down at the lift and up when the 0.3 s window closes (at its edge, and a touch at the edge itself still drags); tap, then touch within the window, anywhere, the button kept down through the moves and up at the lift, with no second down; a touch after the window a stroke of its own; two taps a double-click (up, then down again); a slow or moving stroke, a rest before moving included, never a button; a two-finger tap a right click at the last lift, under and over its travel and time; three fingers nothing; a cancel mid-drag the button up; `reset` letting a held button up; then 20,000 random strokes against a model (never two downs in a row, never an up without a down, no button left down)

**Compiles:** `iOSClient/TrackpadTaps.swift`
