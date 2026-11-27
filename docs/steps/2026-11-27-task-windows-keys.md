**Windows keys for a PC host (2026-11-27, branch `task/windows-keys` from `task/home-gallery`; Kamal approved it
that day). Our fork only (Sill Dev), not for upstream.** Client and wire (two optional fields, fork extensions).

- **Wire.** `WindowList.hostPlatform: String?` ("windows" from the Windows host; Mac hosts never send it; missing
  or unknown is a Mac) and `Hello.pcKeys: Int?` (1 from this device, always). The Mac host is untouched.
- **Literal PC keys** (`iOSClient/HostKeys.swift`, pure, `Tests/checks/host-keys`): with a PC host, Win is 0xE3
  with command, Ctrl 0xE0 with control, Alt 0xE2 with option, Shift as ever (the bytes KeyModifiers.keys sends
  already; only names and shortcuts change). A hardware keyboard's ⌘ goes as Ctrl (0xE0, control) behind "Use ⌘
  as Ctrl on PCs" (`Sill.commandAsControl`, on by default; Settings › This iPhone/iPad › Keys, shown while the
  host is a PC), applied in InputOverlayView's `sendHardwareKey`.
- **Key row on a PC** (PortraitStreamScreen's KeyRow, `platform`): ⊞ Start, Ctrl, Alt, Shift, Tab, Esc, Del, then a
  Shortcuts menu (Search Win+S, Alt+Tab, Win+D, Win+Tab, Win+E, Ctrl+Shift+Esc, Win+L, Alt+F4). The phone's row
  always scrolls on a PC (eight caps; every cap ≥ 44 pt by PhoneKeyRow.Strip). The halves: seven, then arrows,
  keyboard, Shortcuts. Start: a tap latches Win; a key within 0.8 s (`HostKeys.startAloneAfter`) joins it, else
  Win goes alone (down and up) and the Start menu opens. Paste right away sends Ctrl+V to a PC.
