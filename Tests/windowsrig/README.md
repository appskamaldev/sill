# windowsrig

TEST ONLY, a fork extension (appskamaldev/sill, not upstream). Runs Sill Dev (Debug) on the
simulator **Sill-Duo** (iPhone Duo, iOS 27.1) against a host with no hands: the Windows host on
Kamal's laptop, or a synthetic host on this Mac. Debug-only code; Release builds are unchanged.

## Commands

```
nc -z -G 3 100.90.47.26 7455                            # is the Windows host up? (LAN: 192.168.100.67)
Tests/windowsrig/run.sh pair 100.90.47.26 123456789012  # pair by the host's 12-digit code, save it, connect
Tests/windowsrig/run.sh connect                         # connect to the saved computer at 100.90.47.26
Tests/windowsrig/run.sh connect KAMAL-LAPTOP            # ... or by its saved name, or another address
Tests/windowsrig/run.sh input                           # connect, then send the Windows input sequence
Tests/windowsrig/run.sh shot                            # launch plainly, one screenshot
Tests/windowsrig/run.sh window Notepad                  # connect, pick the first window whose title or app has "notepad"
Tests/windowsrig/run.sh window Notepad 1.25             # ... with Aa at 1.25×: the host fits the real window to it
RIG_INPUT=1 Tests/windowsrig/run.sh window Notepad      # ... and send the input sequence (it types into what streams)
Tests/windowsrig/run.sh cmd Notepad minimize            # send minimize|close|fullScreen for that window (kind 15)
RIG_HOST=KAMAL-LAPTOP Tests/windowsrig/run.sh window …  # window and cmd: another saved computer
RIG_NO_BUILD=1 Tests/windowsrig/run.sh connect          # skip the build (reuse the last one)
```

Each run takes the shared lock (`xcode-lock wait "sill/rig"`, released on exit), makes Sill-Duo
only if it is missing, builds into `.build/windowsrig/DerivedData` (`xcodebuild -quiet`, ad hoc
signed, no project change), boots Sill-Duo, installs, launches, waits for the result, takes
screenshots, and shuts Sill-Duo down. Results: `.build/windowsrig/results/<time>-<mode>/`:
`rig.log` (the app's log), `console.*.log`, `*.png`, `summary.txt`. The last line says
`RESULT: OK` or `RESULT: FAILED`.

Pairing saves the computer in the app's container on Sill-Duo, which stays across runs. So pair
once, then use `connect` and `input`.

## What the app does (DEBUG launch arguments)

- `-SillPairAddress <host[:port]> -SillPairCode <12 digits>`: the typed address-and-code path of
  Add a Mac… (`pairTyped`), at launch. It saves the computer and connects as a person's pairing
  does. The port defaults to 7455.
- `-SillAutoConnect <saved name or address>`: dials that saved computer as a tap on its row does,
  pinned to its saved key, at its remote door (`dialSaved`).
- `-SillInputScript windows`: 4 s after the first window list (time for the Desktop to start streaming), sends a step every 1.5 s through
  the client's `sendInput`: the pointer to the centre, a left click, the text "sill rig 123", Return,
  Start (Win down and up), Esc, Ctrl+A, Backspace, a right click, and three wheel steps down. It
  runs only on a loopback session whose host has no version (a synthetic host), as every input
  script does, or with `-SillAllowRemoteInputTest 1` on a host whose list says `hostPlatform`
  "windows". The rig's `input` mode passes both.
- `-SillPickWindow <text>`: on the window lists, the first window whose title or app name contains the
  text (any case) is picked 1.5 s after the list that shows it (after the device's own Desktop request),
  with `select`, as a tap on its thumbnail in the window bar. The log lists the windows (`windows:   id …
  "title" (app) w×h`) whenever the list changes, then `pick: matched …` and `pick: OK: selecting id …`, or
  `pick: FAILED` when no list within 10 s of the first one has a match. `window` mode waits for it, then
  about 5 s, then takes the screenshots.
- `-SillTextScale <number>`: Aa's value for the run, set as the ruler's release sets it (the nearest of
  0.5× to 1.5×, no Off) as soon as the stream screen shows, so every viewport carries `scale`. Each
  viewport sent is logged as its JSON (`viewport: sent {"width":…,"scale":1.25,…}`).
- `-SillCommandWindow <text> -SillCommandAction minimize|close|fullScreen`: that window's command (kind
  15), 2 s after the list that shows it, through the long-press menu's `command`. `cmd: OK` or `cmd: FAILED`.
  It acts on the host's real window: not for a host someone is using.
- `-SillRigLog 1`: writes the log with no other rig argument.

The log: `Documents/sill-rig.log` in the app's data container, os_log subsystem `sill.rig`, and
the console as `rig: …`. Each line starts with a UTC timestamp to the millisecond. There is a line
for every pairing phase, every status line, the session (`session: CONNECTED to … hostPlatform …`),
and every input event sent (`input: t=… <step>: key 0x28 down mods 0x0`), to match against the
host's log. It never logs the pairing code.

## Dry run on this Mac

A synthetic host (no screen, no hardware encoder, no router, a test service type):

```
SILL_TEST_LOOPBACK=1 SILL_TEST_SOFTWARE_ENCODER=1 SILL_TEST_NO_ROUTER=1 SILL_TEST_NO_SCREEN=1 \
  SILL_TEST_SERVICE_TYPE=_silltest._tcp SILL_TEST_REMOTE_ORIGIN=vpn \
  .build/release/SillHost --synthetic --remote=17455      # prints the pairing code
Tests/windowsrig/run.sh pair 127.0.0.1:17455 <code>
RIG_NO_BUILD=1 Tests/windowsrig/run.sh connect 127.0.0.1
RIG_NO_BUILD=1 Tests/windowsrig/run.sh input 127.0.0.1   # allowed by the loopback rule; the host counts input dry
```

The pure parts (`iOSClient/WindowsRig.swift`) are checked by `Tests/checks/windows-rig/run.sh`.
