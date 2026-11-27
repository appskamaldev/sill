**The home screen as a gallery, direction A (2026-11-27, branch `task/home-gallery` from `task/ios16`; Kamal
approved the design that day). Our fork only (Sill Dev), not for upstream.** Client only, no wire change.

- **What it is.** ConnectScreen's rows became "Computers": a large title, a toolbar with Add Computer (+) and ⋯
  (Show as List/Tiles, Search Nearby, Settings, which opens Sill's page in the Settings app), then a banner if one
  is up, the status line (.subheadline), the hero (the most recently used computer a tap connects to:
  `SavedMac.lastConnectedAt`; its picture from `macPreviews`, else the desktop symbol on a soft tint; 16:10; a
  capsule Connect), then two columns of cards (22 pt continuous corners, the picture inset 8 pt at 14 pt,
  `secondarySystemBackground`, a light shadow on the hero in light mode). The list style, and every accessibility
  text size, use direction B's row. The groups and each card's context menu are as before.
- **State: always icon, colour and word** (`HomeGallery.look`, pure, `Tests/checks/home-gallery`): Here (Wired,
  Wi‑Fi, Direct, "Here"), Away (was "Remote", also in `FoundMac.word`), Connecting… (a ProgressView, a 2 pt accent
  ring, a trailing Cancel: `StreamClient.cancelConnecting`), Tap to pair, Removed (a saved computer that reads Not
  paired), Update Sill, Not available (a computer away whose tap's dial ended with no session and no banner;
  "Not answering. Tap to try again." for the latest; its picture desaturated). The card under the finger gives
  at once (`CardPress`) and shows Connecting… before the dial's own line. Which card dials is read from the
  status line (`HomeGallery.dialed`). VoiceOver: one element per card, "KAMALELTAYEB, computer, Away,
  Connecting", a hint per state, Cancel as an action. The kind is "computer": every host is a "Mac" to the app.
- **Banners** (`StreamClient.banner`, `HomeGallery.Banner`): raised where the status line gets the same words:
  a goodbye's removed, remoteOff and update (StreamClient+Remote), a remote dial's revoked, wrongMac, remoteOff
  and localNetwork, at home removed, the wrong Mac and a new key (StreamClient+Home); also Local Network refused
  (the idle line). A title, one line, one action (Pair Again: the row at home asks, else Add Computer; Update:
  the App Store, only with its address; Open Settings), × (44 pt). Spoken as it comes; the status line it stands
  for is neither shown nor spoken again. Cleared by × or a session. The "Update Sill in the App Store" link under
  the status line is gone (the banner's Update).
- **First run** (nothing saved): an 80 pt tile, "Connect to your computer", "Open Sill on your Mac or PC and show
  its pairing code.", Scan Pairing Code, Enter Address and Code, Nearby (the rows found), "Get Sill for Mac or
  Windows" (SillLinks.download).
- **Colours and type.** The connect screen follows the system: ContentView's `.preferredColorScheme` is `.dark`
  only while connected (the stream screen stays dark). The accent is the asset `SillAccent` (#2D5BD6 light,
  #9BB7FF dark, `Palette.accent`); `Palette.text`, `muted` and `control` follow the appearance (their dark values
  as before), so Add Computer's card reads in light mode. Dynamic Type styles throughout; `@ScaledMetric` for the
  thumbnails and symbols. Margins 20, gutter 12, at most 640 pt wide.
- **Copy.** "Computers", "Add Computer", "Mac or PC" in the add card and the footer, "Looking for computers…",
  the address placeholder "100.x.y.z (Tailscale) or name.local". Protocol and code names unchanged.
- **Duo.** Cover: the same with 20 pt margins. Open flat: up to 640 pt, the hero then two columns. Laptop pose:
  everything above the fold scrolls there, the footer below (Add is in the toolbar, so it is above too). Book
  pose: the gallery on the leading page (`columnX`); the trailing page's preview was not built (too big for now).
- **Harness.** `-SillConnectCase gallery|galleryconnecting|gallerybanner`, `-SillColorScheme light|dark` for a
  connect case (the system's without it).
- **Verified.** `Tests/checks/run-all.sh home-gallery repo-rules tour full-screen pointer-presence connection-list
  policy home-device goodbye remote-rules duo`: all passed (home-gallery 144). Debug for the simulator and
  Release for a device at 16.4. Harness photos on the passDuo-Duo simulator (borrowed, Sill removed after).
  Not run: VoiceOver on a device, iOS 16 itself, the real dial paths (Cancel, Not answering).
