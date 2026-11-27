# Sill wire protocol (as implemented upstream)

Source of truth: upstream `Sources/StreamProtocol/` at `22ab5a3e`. This file describes it for
the Windows port; when they disagree, **the Swift source wins** and this file is fixed.
Upstream's own rules: `docs/COMPATIBILITY.md`. Status of each item here: read from source, **not
yet verified against a device** unless marked. Items marked **(vectors)** were measured by running
upstream's Swift (`windows/vectors/`, 2026-11-27, Swift 6.4) and are enforced by `cargo test`.

## 1. Transport

- TCP only. No UDP, no QUIC.
- **Home door**: any free port, advertised by Bonjour `_sill._tcp`, instance name = the host's
  name (`StreamServer.swift:456-466, 670-671`). TCP: no Nagle, keepalive idle 5 s / interval 2 s /
  3 probes (`:651-658`).
- **Remote door**: TCP **7455** by default (`HostConfig.swift:138,158`); no Bonjour. Same TCP
  options plus a 15 s drop for unacknowledged data (`RemoteTLS.swift:93-103`). Runs while Remote
  Access is on or a remote pairing window is open. A typed address without a port means 7455 on
  the client (`StreamClient+Remote.swift:999,1211`).
- Plain-TCP door: development only. A release client never dials it (`DiscoveryPolicy.swift:1081-1092`).

### Bonjour TXT

| Key | Value | Meaning |
|---|---|---|
| `p` | `"1"` | TLS, paired devices only (Require pairing on). Any value but `"0"`, or empty, reads as `"1"`. |
| `p` | `"0"` | TLS, any device may open a session. |
| (no `p`) | — | Plain door; a release client refuses it. |
| `r` | base64url(12 bytes) = `n(6 random) ‖ HMAC-SHA256(recognitionKey, "sill-tag-v1" ‖ n)[0..6]` | Recognises a saved host without a stable identifier. Fresh `n` per registration. Hint only, never trust (`Pairing.swift:143-176`). |

## 2. TLS (`RemoteTLS.swift`, `RemoteIdentity.swift`)

- TLS **1.3 only**; tickets and resumption **off**. Measured suite upstream: 0x1302.
- Server ALPNs: **`sill/1`** (session) and **`sill-pair/1`** (pairing), both offered for good.
  The client offers exactly one; SNI is `"sill"` (ignored by host).
- Both sides present a **self-signed X.509 v3, P-256, ECDSA-SHA256** certificate, no extensions,
  random CN, validity 2026-01-01 … 9999-12-31. The host rebuilds it each launch from its stored key.
- Server **requires** a client certificate.
- **Trust is the pin only**: fingerprint = SHA-256 of the DER SubjectPublicKeyInfo
  (`id-ecPublicKey` + `prime256v1`). Non-P-256 keys give no fingerprint and fail. Names, dates,
  chains are never evaluated.
- **MacID** = Crockford base32 of fingerprint[0..10] (16 chars).
- Host admission, decided inside the handshake with the negotiated ALPN (`DoorPolicy.trusts`,
  `DoorPolicy.swift:69-80`):
  - `sill/1`: key is paired; or home door with Require pairing off.
  - `sill-pair/1`: always at the home door; at the remote door only while a pairing window is open.
- **Refusal semantics the client depends on**: the client goes `.ready` and its first read fails
  with **-9825** (peer bad certificate) or **-9829** (peer cert unknown). It then marks the saved
  host "Not paired" and stops redialing (`iOSClient/DiscoveryPolicy.swift:1303-1324`,
  `RemoteDialPolicy.swift:234-242`). A Windows host must send the TLS alert `certificate_unknown`
  (46), as the Mac host does: Apple's client reports it as -9825. `bad_certificate` (42) is
  reported as -9808, which the device reads as "isn't Sill" (R1, measured 2026-11-27 with
  `windows/tools/tlsprobe`; TESTING.md).

## 3. Framing (`StreamMessage.swift`)

14-byte header, big-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | kind (u8); unknown kinds are read and skipped (Swift maps them, and the byte 255, to its `unknown = 255` placeholder) |
| 1 | 8 | timestamp: IEEE-754 f64 bit pattern, host wall clock seconds since 1970 |
| 9 | 1 | isKeyframe: 1 or 0; on reading, only exactly 1 is a keyframe **(vectors)** |
| 10 | 4 | payload length (u32) |

Caps (close the connection when exceeded):

| Direction | Cap |
|---|---|
| Any client → host message | 1 MiB (`maxClientPayload`) |
| Pairing request (kind 19) | 4096 B |
| Video frame, as the device accepts | 32 MiB |
| Any other host → device message | 4 MiB |

## 4. Kinds

| Kind | Name | Dir | Payload | Windows MVP |
|---|---|---|---|---|
| 0 | parameterSets | H→D | `nalLen u8, count u8, {len u16, bytes}×count` (VPS, SPS, PPS) | ✅ |
| 1 | frame | H→D | one HEVC access unit, NALs with `nalLen`-byte (4) BE length prefixes | ✅ |
| 2 | windowList | H→D | JSON `WindowList` | ✅ (empty windows) |
| 3 | thumbnail | H→D | `windowID u32 BE ‖ JPEG` (≤208×132, q 0.6) | Phase 3 |
| 4 | appIcon | H→D | `len u16 BE ‖ bundleID UTF-8 ‖ PNG` (64×64) | Phase 3 |
| 5 | appList | H→D | JSON `[AppInfo{name,bundleID}]` | Phase 3 |
| 6 | selectSource | D→H | JSON `StreamSource` | ✅ |
| 7 | launchApp | D→H | JSON `{bundleID}` | Phase 3 |
| 8 | input | D→H | JSON `InputEvent` | ✅ |
| 9 | viewport | D→H | JSON `Viewport{width,height,scale?,localCursor?,fps?}`; sides 1…16384, scale 0.25…4 else refuse; a non-whole `fps` fails the decode **(vectors)** | ✅ (fps; size ignored for Desktop) |
| 10 | ping | D→H | 8 bytes, client clock f64 BE; ~every 0.25 s | ✅ |
| 11 | pong | H→D | ping payload and timestamp unchanged, immediately | ✅ |
| 12 | clientStats | D→H | JSON `{fps,frameAgeMs,rttMs,device,frameAgeMaxMs?,rttMaxMs?,audioBehindMs?,audioLate?}` 1/s | ✅ log |
| 13 | tick | H→D | empty; every 30 ms while streaming or input in last 3 s (skipped if anything sent within 30 ms) | ✅ |
| 14 | cursorShape | H→D | `hotX,hotY,width,height` f32 BE (points) ‖ PNG | Phase 3 |
| 15 | windowCommand | D→H | JSON `{id, action: close|minimize|fullScreen}` | Phase 3 |
| 16 | hostSettings | H→D | JSON `HostSettingsState` | ✅ minimal |
| 17 | changeSettings | D→H | JSON `HostSettingsChange{token, …optional}` | Phase 2 |
| 18 | macInfo | H→D | JSON `SignedMacInfo{info,key,sig}` | Phase 2 |
| 19 | pairRequest | D→H | JSON `PairRequest` — only on `sill-pair/1` | ✅ |
| 20 | pairResult | H→D | JSON `PairResult` — then close | ✅ |
| 21 | pairingWanted | D→H | empty; home door, own networks, ≤1/30 s | Phase 2 |
| 22 | goodbye | H→D | JSON `{reason, message?, minimumVersion?, reconnect?}` | ✅ |
| 23 | hello | D→H | JSON `Hello` — first message of every session | ✅ |
| 24/25/27 | macMenu / pressMenuItem / fetchMenu | both | MacMenu.swift | Later (optional) |
| 26 | macPointer | H→D | JSON `{x?,y?,inside,seen}` | Phase 3 |
| 28 | gesture | D→H | JSON `{gesture, fingers}`; only if list says `gestures ≥ 1` | Phase 4 |
| 32 | clipboard | both | JSON `{text?, image?, stop?}`; only if list says `clipboard ≥ 1` | Phase 4 |
| 43 | audio | H→D | type 1 JSON AudioFormat / type 2 binary packets | Phase 4 |
| 44 | keyframeRequest | D→H | empty; ≤1/s honoured | ✅ |
| 47 | nextDisplay | D→H | empty; only if list says `desktopDisplays ≥ 2` | Phase 4 |

Reserved by upstream for open PRs: 29–31, 33–42, 45, 46. Never use them.

## 5. JSON conventions (Swift `JSONEncoder`/`JSONDecoder`, default strategies)

- Missing optional fields decode as nil; extra keys are ignored. A missing **non-optional**
  field fails the whole message.
- Enums with associated values use Swift's synthesized shape: `{"case":{"label":value}}`, unlabeled
  values as `"_0"`. Cases without values: `{"case":{}}`.
  - `StreamSource`: `{"none":{}}`, `{"desktop":{}}`, `{"window":{"_0":1234}}`.
  - `InputEvent`:
    - `{"pointer":{"_0":"leftDown","x":0.25,"y":0.5}}` (actions `move,leftDown,leftUp,rightDown,rightUp`)
    - `{"scroll":{"x":0.5,"y":0.5,"dx":0.0,"dy":0.03}}` (fractions of frame, natural sign)
    - `{"scrollGesture":{"_0":"began","x":0.5,"y":0.5}}` (`began,ended,momentumBegan,momentumEnded`)
    - `{"text":{"_0":"hé"}}` — `\n`,`\t`,`\u0008` mean Return, Tab, Backspace
    - `{"key":{"hidUsage":4,"down":true,"modifiers":1048576}}`
- `Data` fields are standard base64 (not url-safe) unless the field is documented as base64url
  (fingerprints, secrets, recognition key in links and PairResult).
- Confirmed **(vectors)**, risk R2 closed for these types:
  - `text` is `{"text":{"_0":"hé"}}`, `scroll` is `{"scroll":{"x":…,"y":…,"dx":…,"dy":…}}`, `key`'s
    `modifiers` is a plain JSON integer up to 2^64−1.
  - Key order is **not stable**: it follows Swift's per-process hash seed (an enum's `"_0"` moves
    between runs). Never compare bytes; the vector tool fixes the seed only to keep files stable.
  - Swift writes a whole Double without a fraction (`"x":0`, `"captureScale":1`), large ones as
    `1e+300`, and escapes `/` as `\/`. Rust writes `0.0`; each side reads the other's form.
  - An **integer** field takes any JSON number whose value is whole and in range: `42`, `42.0`,
    `1e3`; it refuses `42.5`, `"42"`, `-1` for unsigned, `4294967296` for a UInt32.
  - An enum is an object holding **exactly one** known case key (other keys are ignored:
    `{"desktop":{},"x":1}` decodes; `{"none":{},"desktop":{}}` does not); the case's value must be
    an object (`{"none":null}`, `{"none":5}` fail); unknown keys inside it are ignored.
  - A struct never decodes from an array (`"settings":[…]` fails).
  - `Data` (Clipboard `image`) is standard base64 with padding: length a multiple of 4, no
    whitespace; Foundation also accepts non-zero trailing bits (`Zh==`) and extra `=` (`Zm9v====`).
  - nil optionals are left out on encode (`HostSettingsState` included) and `null` reads as nil.

## 6. Session sequence

```
device                                   host
  │ TCP connect, TLS 1.3 (ALPN sill/1, both certs, pin check)
  │──── kind 23 hello ───────────────────►│  DeviceGate: floor "0" → admit
  │                                        │  (below floor: kind 22 update, reconnect:false, close)
  │◄─── kind 2 windowList ─────────────────│  must arrive ≤10 s or device gives up
  │◄─── kind 16 hostSettings ──────────────│  (immediately after)
  │◄─── kind 18 macInfo (if identity) ─────│
  │◄─── kind 4 icons…, kind 5 apps, 14 cursor
  │──── kind 6 selectSource / kind 9 viewport / kind 27 menus sub…
  │◄─── kind 0 parameterSets, kind 1 keyframe, kind 1 deltas…
  │──── kind 10 ping (0.25 s) ────────────►│──► kind 11 pong
  │◄─── kind 13 tick (30 ms while live) ───│
  │──── kind 8 input, kind 12 stats (1 s), kind 44 keyframe request
  │◄─── kind 22 goodbye {reason, message, reconnect} ── half-close, ≤1 s drain
```

- If `active` is `none`, the client selects Desktop by itself.
- The device drops a link silent for 6 s; the host drops a remote client silent for 8 s, or not
  draining for 4 s (home) / 15 s (remote).
- Goodbye reasons: `removed, remoteOff, internetOff, quit, busy, pairingRequired, update, restarting`.
- Remote door: at most 8 sessions; the 9th gets `busy`.

## 7. Pairing (`sill-pair/1`)

One kind 19 within 4 s of `.ready` and at most 10 s after accept (`DoorPolicy.deadline`,
`DoorPolicy.swift:272-277,343-346`; `StreamMessage.swift:37`'s "10 s" is the second limit), one
kind 20, host closes (after send or 250 ms).

`PairRequest`: `{v:1, method:"qr"|"code"|"ask"|"cancel", proof, name, model?, cable?}`.
`PairResult`: `{ok, proof?, macID?, name?, recognitionKey?, reason?, triesLeft?, retryAfter?, method?, message?}`.
Reasons: `code, closed, expired, stopped, busy, shown, openOnMac, locked`. `v ≠ 1` → `closed`, no try counted.

Keys and proofs (fingerprints taken from the TLS session, never from a message):
- Code: 12 ASCII digits = 11 random + Damm check digit (`Pairing.swift:14-68`).
  `K = PBKDF2-HMAC-SHA256(code, "sill-pair-v1" ‖ fp_host, 600000, 32)`.
- QR: `K` = the link's 16-byte secret.
- `proof_D = HMAC-SHA256(K, "sill-pair-v1 device" ‖ 0x00 ‖ fp_dev ‖ fp_host)`
- `proof_M = HMAC-SHA256(K, "sill-pair-v1 mac" ‖ 0x00 ‖ fp_host ‖ fp_dev)`
- Ok result carries `recognitionKey` (32 bytes, base64url).

Window limits (`PairingWindow.swift`): lives 300 s; 5 wrong proofs end it; after wrong proof n
(1–4) wait 1/2/4/8 s; same source once per 5 s; else `busy` + `retryAfter`.

QR / link: `sill://pair?v=1&m=<macID>&k=<b64url fp>&s=<b64url secret>&n=<name>&p=<remote port>&a=<addr>…`
(≤5 addresses; percent-encode to RFC 3986 unreserved + `:`, upper-case hex). The host only builds
links. Parsing (the device's side) follows `URLComponents`, which is lenient **(vectors)**: user
info and a port after `pair` are ignored, a fragment is dropped, an invalid `%zz` is kept as typed,
a space is kept, `+` is not a space, an `a` with no `=` is refused as `address("")`, `p=07455`
is fine (≤5 digits).

Addresses (`AddressParser`, typed or in `a=`) depend on Apple's libc **(vectors)**: `inet_pton`
limits each IPv6 group by value, not digit count (`00001::` is `1::`; `12345::1` is refused) and
takes leading zeros in an embedded IPv4 (`::01.2.3.4`); `inet_ntop` prints IPv4-compatible
addresses dotted (`::1.2.3.4`, `::0.1.0.0`); `inet_aton` takes `0x.1` and `0x100000000` (so those
are refused as IPv4, not taken as names). `sill-pairing` ports these exactly.

## 8. Video

- HEVC **Main 8-bit**, auto level, no B-frames / no reordering, real-time.
- Bitrate fixed per session (default 15 Mbps home, 4 Mbps away; setting is "per 60 fps", stream
  gets `bitrate × fps / 60`), max keyframe interval fps×4, keyframes on demand.
- Capture colour: NV12 full range, sRGB. VUI should signal full range BT.709 (to verify on device).
- Parameter sets sent as kind 0 right before every keyframe; the device ignores identical repeats
  and rebuilds its decoder on new ones (resolution change = new kind 0 + keyframe).
- Frames have the header `isKeyframe` flag; the device discards everything until format + keyframe.
- Device displays immediately (no PTS). Cursor is **never** in the video.
- Windows: MF emits Annex-B (start codes). Convert to 4-byte length prefixes and strip VPS/SPS/PPS
  (and AUD) out of the AU into kind 0.

## 9. Input mapping (host-side, Windows)

- Pointer: `x,y ∈ [0,1]` of the frame → source rectangle in virtual-desktop pixels → `SendInput`
  `MOUSEEVENTF_ABSOLUTE|MOUSEEVENTF_VIRTUALDESK`. Move with a button held = drag. Click count is
  not on the wire; Windows derives double-clicks itself.
- Scroll: `dy × rect height` (points) → wheel delta, carrying the remainder; natural sign as sent.
- Keys: HID usage → Windows scan code (set 1), extended flag for navigation keys.
  Modifier bits (UIKeyModifierFlags): capsLock 1<<16, shift 1<<17, control 1<<18, option 1<<19,
  command 1<<20. Policy (configurable): **Command → Ctrl**, Option → Alt, Control → Ctrl,
  Command+Space → Win (search). A key's down carries the device's modifiers; releasing all held keys
  when a device leaves.
- `keyRepeat: 1` in the window list makes the client send hardware-keyboard characters as keys and
  expect host-side repeat (use `SPI_GETKEYBOARDDELAY/SPEED`). MVP omits it, so the client types
  them as text.

## 10. Host settings (kind 16), minimal MVP form

`HostSettingsState` required fields: `settings{maxFPS,bitrate,captureScale,prioritizeSpeed,virtualDisplay}`,
`persistent`, `virtualDisplayAvailable`, `softwareEncoder`. MVP sends fixed values with
`virtualDisplayAvailable:false` and omits `directWireless`, `sendAudio`, `away`, `link`.
Types (`HostSettings.swift`): `maxFPS`, `bitrate` Int; `captureScale` Double; the three flags Bool;
`settings.directWireless` and `settings.sendAudio` optional Bools. The MVP form
`{"settings":{"maxFPS":60,"bitrate":15000000,"captureScale":1,"prioritizeSpeed":true,"virtualDisplay":false},"persistent":false,"virtualDisplayAvailable":false,"softwareEncoder":false}`
decodes in Swift and is built by `sill-wire` **(vectors)**. Missing `softwareEncoder` or
`virtualDisplay` fails the whole message.

## 11. Audio (Phase 4)

Kind 43, first payload byte = type. Type 1: JSON `AudioFormat{codec,sampleRate,channels,
framesPerPacket,primingFrames,bitrate,epoch,cookie(base64 ASC),source,app}`. Type 2:
`2, epoch u16, seq u32, flags u8 (bit0 segment start), count u8, {len u16, bytes}×count`. Header
timestamp = wall clock of first sample. Caps: format ≤4 KB, packets ≤16 KB. Codec must be one the
hello lists; today the client lists only `"aac-eld"` (48 kHz stereo, 480/512 frames, 128 kbps).

## 12. Where the Windows host (Phase 1) differs from upstream, and why

- **Refusal alert:** an untrusted key gets `certificate_unknown` (46) from rustls' verifier
  (`CertificateError::Other`), the alert the Mac host sends and Apple's stack reports as -9825.
  Proved on loopback by `tlsprobe` (Apple's own stack: `.ready`, then `tls(-9825)` on the first
  receive, as against the Mac host), a rustls client (`AlertReceived(CertificateUnknown)`) and
  Python `ssl` (`TLS_ALERT_CERTIFICATE_UNKNOWN`). Checkpoint 1 showed that the first choice,
  `bad_certificate` (42), gives -9808 on the device ("isn't Sill"). The device's "Not paired"
  with the fix waits for the laptop re-run.
- **Addresses:** none on a virtual adapter (Hyper-V/WSL vEthernet, VirtualBox, VMware, Docker) is
  listened on by default or put in a pairing link's `a=`; upstream has no such adapters to skip.
- **`ask` at the home door:** no window is ever opened by an ask (this host has no window to show
  a code in, and a console the device's user cannot see is not one). A window already open
  (`--pair`) → `shown`; none → `openOnMac` (upstream's answer when it will not open one, e.g. an ask
  "quiet" or "often"). At the remote door → `closed`, as upstream.
- **`cancel`:** always `closed` (upstream's answer); there is never a device-opened window to withdraw.
- **Pairing window:** only `--pair` opens one, for both doors (as the Mac user's own window); it is
  used once. No `locked` answer (no lock detection until Phase 2).
- **Admission:** handshake deadline 4 s (10 s for an address that proved a paired key in the last
  24 h), kind 19 within 4 s of ready and 10 s of accept. Upstream's per-source places, back-off and
  failure counting (`DoorPolicy` places/backoff) are not ported yet.
- **Device gate:** floor "0": admitted at once, nothing read ahead (as upstream); the hello is logged
  when it comes. The catalog (kinds 2, 16) goes at once; no kind 18 yet (Phase 2).
- **Sessions:** at most 8 at the remote door (9th: `busy`); the home door has no cap (as upstream).
  A remote client silent 8 s is dropped; upstream's "not draining for 4 s / 15 s" and the 15 s TCP
  unacknowledged-data limit are not ported. Ticks run while the picture streams (not for input).
- **Video:** at most 2 frames in flight per client (the home rule, for both doors), else drop
  deltas and wait for a keyframe; a drop, a join or a kind 44 (≤ 1/s per session) restarts the clip
  at its IDR for every viewer. Upstream's remote backlog budgets are not ported.
- **Unpair while connected:** the running host rereads the paired list when its file changes
  (`--unpair` from another process) and sends goodbye `removed` to that key's sessions.

### Fork extensions (not upstream)

Additive fields only this fork's Windows host and iOS client know. Upstream's Swift types ignore
unknown keys, so a Mac host or an older client skips them unharmed. Vectors: `vectors/fork.json`,
written by hand (the Swift generator does not know these fields).

- `WindowList.hostPlatform` (String, optional): the Windows host always sends `"windows"`; a Mac
  host never sends it. A client reads absent or any other value as a Mac.
- `Hello.pcKeys` (Int, optional, generation 1; the `offConsoleView` pattern): a client that sends
  `pcKeys: 1` declares that, on a host whose list says `hostPlatform` "windows", its kind 8 keys are
  literal PC keys. The host then takes HID 0xE3/0xE7 and the Command flag (1<<20) as the Windows
  key, Control (1<<18) as Ctrl, Option (1<<19) as Alt and Shift (1<<17) as Shift, with no
  Command→Ctrl mapping and no Command+Space→Win. Without it (older clients, the App Store build),
  the Mac habits of §9 apply. In literal mode the client owns the hardware-keyboard mapping (for
  example an iPad keyboard's ⌘ sent as Ctrl, a client preference).
- Ctrl+Alt+Del is the secure attention sequence and cannot be injected with `SendInput`; the client
  offers Ctrl+Shift+Esc (Task Manager) instead, and the host does not call `SendSAS` (risk R6).
