# Compatibility and feature parity

Legend for **Windows status**: `planned` (not written), `implemented` (code exists, untested on
hardware), `tested` (passed on real hardware, with the evidence in TESTING.md). Today everything is
`planned`: a feature's row moves only when the feature works end to end. Phase 1 task A
(2026-11-27) added the portable logic several rows need (wire format, pairing crypto and window,
device gate, door trust), tested against Swift-generated vectors on macOS and Windows CI; no row
changes for that, and none of it is hardware-tested.

## 1. Can the unmodified iOS client use a Windows host?

**Yes, for everything except the word "Mac" and audio (needs a third-party encoder).** Evidence
(upstream paths):

- The client links only `Sources/StreamProtocol` for the wire; nothing in it is Apple-only in
  *behaviour* (TLS 1.3, P-256, SHA-256, PBKDF2, HMAC, JSON, HEVC).
- No OS or platform field exists in `Hello`, `WindowList` or `MacInfo`; the client cannot tell, and
  does not care, what OS the host runs.
- Keys are USB HID usages (`Input.swift:32-34`), pointers are frame fractions (`Input.swift:3-5`):
  host geometry and keyboard layout stay host-side.
- Every optional feature is gated by a field the host sends (`gestures`, `clipboard`, `keyRepeat`,
  `desktopDisplays`) or by a kind the host sends (16, 18, 24, 14, 26, 43). Absent → hidden or
  default (`HostSettingsPanel.swift:496,732`, `MacMenuState.swift:150`, `HEVCDisplayView.swift:214-235`).
- Manual add by address accepts Tailscale IPv4/IPv6 and MagicDNS names (`AddressParser.swift`,
  `AddMacCard.swift:116-173`), so no Bonjour is needed over Tailscale.

What the client cannot do without a change:

| # | Limitation | Why it can't be fixed host-side | Smallest client change (backward compatible) |
|---|---|---|---|
| C1 | UI says "Mac" ("Add a Mac", "Update Sill on your Mac", "This Mac is locked") | ≈98 literal strings; no OS field | Optional `hostPlatform` in WindowList/MacInfo; client picks "PC" copy when it says `"windows"`. Mac hosts never send it → unchanged. |
| C2 | Audio only as AAC-ELD | `Hello.audio` is `["aac-eld"]` today | Client adds `"opus"` to its list (decode via AudioToolbox). Mac hosts still pick `aac-eld`. Only if FDK-AAC is rejected. |
| C3 | "Mission Control", "App Exposé", "Spotlight" in tour/help copy | Copy only | Same `hostPlatform` switch. |
| C4 | Menus button labelled for Mac menus | Feature is optional; hidden if host never sends kind 24 | None needed. |

None of C1–C4 is needed for the MVP. Any client change waits for Kamal's OK and must keep working
with the original Mac host (verified by connecting to Sill.app on the Mac).

## 2. Feature parity matrix

| Feature | Mac host (upstream) | Windows approach | Phase | Windows status |
|---|---|---|---|---|
| TLS 1.3 home door + Bonjour `_sill._tcp` (`p`, `r`) | ✅ | rustls + `DnsServiceRegister` | 1 | planned |
| Remote door :7455 (Tailscale, LAN, VPN) | ✅ | rustls, bind-address config | 1 | planned |
| Pairing: typed code | ✅ | port of `Pairing.swift` | 1 | planned |
| Pairing: QR / `sill://pair` link | ✅ | QR in settings window | 2 | planned |
| Pairing: ask from device / "Pair This iPad…" (kind 21) | ✅ | toast + window | 2 | planned |
| Pairing: USB cable auto-pair | ✅ (IOKit cable check) | not planned (no Apple cable link to a PC) | — | n/a |
| Require pairing on/off | ✅ | same | 2 | planned |
| Device gate / goodbye / update refusal | ✅ | port `DeviceGate`, `Goodbye` | 1 | planned |
| Signed host info (kind 18) | ✅ | ECDSA P-256 over info bytes | 2 | planned |
| Full desktop streaming | ✅ | WGC monitor capture | 2 | planned |
| Multiple displays (kind 47 cycle) | ✅ | monitor order main → right | 4 | planned |
| Individual window streaming | ✅ (primary feature) | WGC window capture | 3 | planned |
| Window list, thumbnails, icons, app list, launch app | ✅ | EnumWindows, WGC snapshot/JPEG, shell icons, Start-menu apps; `bundleID` = AppUserModelID or exe path | 3 | planned |
| Fit window to device (viewport scale) | ✅ (AX resize) | `SetWindowPos`, restore on stop | 3 | planned |
| Window commands close/minimize/full screen | ✅ | `WM_SYSCOMMAND` | 3 | planned |
| Covered window keeps streaming | ❌ on Mac (SCK freezes covered windows) | ✅ WGC captures occluded windows | 3 | planned |
| Minimized window streaming | ❌ | ❌ (WGC gives no frames) — same | — | — |
| Virtual display | ✅ optional, private API | IddCx driver (signing needed) | 4+ | planned |
| Hardware HEVC encode | ✅ VideoToolbox | MF hardware MFT (NVENC/AMF/QSV) | 2 | planned |
| Software encoder fallback | ✅ VT software, 60 fps max | open (x265 GPL); MVP: hardware required | 5 | open |
| Adaptive bitrate | ❌ (report only) | optional improvement, host-side | 4 | planned |
| Home/away quality pair | ✅ | same, plus "treat Tailscale as home" | 2 | planned |
| Frame rate follows device (24–120) | ✅ | same | 2 | planned |
| Pointer, click, drag, right click | ✅ | SendInput | 2 | planned |
| Scroll with phases / momentum | ✅ (trackpad-like) | wheel deltas (phases ignored); precision touchpad later | 2 | planned |
| Text and keys, modifiers | ✅ | SendInput Unicode + scan codes, Cmd→Ctrl | 2 | planned |
| Key repeat (`keyRepeat:1`) | ✅ | system delay/rate | 3 | planned |
| Trackpad gestures (kind 28) | ✅ Mission Control etc. | Win+Tab, Ctrl+Win+←/→, Win+D, Win | 4 | planned |
| Cursor shape (kind 14) | ✅ | `GetCursorInfo` + icon → PNG | 3 | planned |
| Remote pointer position (kind 26) | ✅ | `GetCursorPos` polling | 3 | planned |
| Clipboard text (kind 32) | ✅ | Win32 clipboard listener | 4 | planned |
| Audio (kind 43) | ✅ beta, AAC-ELD | WASAPI loopback + encoder TBD (R5) | 4 | open |
| App menus (kinds 24/25/27) | ✅ AX | UI Automation | 5 | optional |
| Lock screen streaming + password typing | ✅ | secure desktop needs SYSTEM helper (R6) | 4 | planned |
| Locked / off-console list (`locked`, `offConsole`) | ✅ | WTS session notifications | 2 | planned |
| Privacy mode (lock after last device leaves) | ✅ | `LockWorkStation`, monitor off | 4 | planned |
| Direct Wireless (AWDL) | ✅ | not possible on Windows; field omitted | — | n/a |
| Menu bar / tray app, Settings, Log window | ✅ | tray + Win32 settings window | 2–3 | planned |
| Start at login | ✅ login item | Run key / scheduled task | 2 | planned |
| Firewall rule | ✅ socketfilterfw | `INetFwPolicy2`, Private + Tailscale only | 2 | planned |
| One-click updates | ✅ GitHub manifest | later; no external service by default | 5 | planned |

## 3. iPhone Duo features

All Duo behaviour is client-side (`DuoPosture.swift`, `DuoPostureReader.swift`, layouts,
trackpad). The host's only part: honour viewport changes (kind 9) on fold/rotation — for Desktop
the size is ignored (as on the Mac), for windows the host refits — and keep fps from `fps`.
`DuoPostureReader.swift:203-208` reads `hostSettings.stream.onVirtualDisplay`; the Windows host
sends `false`. Nothing Duo-specific is lost.

## 4. Compatibility rules the Windows host keeps (from upstream `docs/COMPATIBILITY.md`)

- Offer `sill/1` and `sill-pair/1` for good.
- Additive only; never reuse kind numbers; never send a kind/field/case a peer did not say it takes.
- Refuse a device with kind 22 `update` (`reconnect:false`) and nothing before it — never serve it wrong.
- `SillProtocol.current` stays 1. A Windows-only extension, if ever needed, is an optional field
  that Mac-era clients skip, and is proposed upstream first.
- The Windows host claims the **same compatibility floor** as Sill.app 1.0: it serves every device
  build from the first public one on.
