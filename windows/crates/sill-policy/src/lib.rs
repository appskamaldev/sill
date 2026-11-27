//! Host policies for the Windows host, ported from upstream `Sources/SillHost/` (DoorPolicy,
//! PairingWindow, DeviceGate) and `StreamProtocol/Viewport.swift` (ViewportBounds). Pure: no
//! clock, no I/O; time is passed in. Phase 1 subset.
#![forbid(unsafe_code)]

pub mod device_gate;
pub mod door;
pub mod pairing_window;
pub mod viewport;
