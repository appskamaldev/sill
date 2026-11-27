**Windows rig (2026-11-27, branch `task/windows-rig` from `task/windows-keys`; Kamal approved it that day,
with one new simulator, Sill-Duo). Our fork only (Sill Dev), DEBUG only.**

- `Tests/windowsrig/run.sh pair HOST[:PORT] CODE | connect [NAME|ADDRESS] | input [NAME|ADDRESS] | shot`:
  the lock, Sill-Duo (made when missing: iPhone Duo, iOS 27.1), a Debug build into
  `.build/windowsrig/DerivedData`, boot, install, launch, screenshots, the app's log, a summary, shutdown.
- App side (`iOSClient/WindowsRig.swift`, pure parts checked by `Tests/checks/windows-rig`): the rig's log
  (Documents/sill-rig.log, os_log `sill.rig`, UTC ms stamps); `-SillAutoConnect` through `dialSaved`;
  `-SillInputScript windows` (no `-SillLive` needed), allowed on loopback as before, or on a
  `hostPlatform` "windows" host with `-SillAllowRemoteInputTest 1`. `-SillPairCode`/`-SillPairAddress`
  already ran the typed pairing path; the rig only logs its result.
