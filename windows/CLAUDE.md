# Sill for Windows — rules for agents

Read `WINDOWS_PORT.md` first, then the document for your task. Kamal's global rules apply.

- **Upstream is the spec.** `Sources/StreamProtocol/` (upstream Swift) defines the wire. Never
  edit upstream files for the Windows port; a client change needs Kamal's OK and must keep the Mac
  host working.
- **Compatibility floor:** additive only, offer `sill/1` + `sill-pair/1` forever, never send a
  kind or field a peer did not say it takes, refuse with kind 22 `update` rather than serve wrong.
- **Vectors decide.** Any change to `sill-wire`/`sill-pairing` must pass the golden vectors;
  regenerate them after merging upstream.
- **Latency beats quality.** Drop frames, never queue them. No GPU→CPU copies on the video path.
- **Honest status.** COMPATIBILITY.md's matrix says planned / implemented / tested; only a pass on
  real hardware with recorded evidence counts as tested.
- **Security:** validate every incoming byte against upstream's caps; no telemetry, no external
  services, no hardcoded secrets, no UAC bypass; default bind never public.
- **Windows builds** don't take `xcode-lock`. Vector generation (Swift on the Mac) does not need it
  either (no xcodebuild, no simulator).
- Keep this file short; details belong in the other documents.
