//! HEVC for the Windows host (ADR-4). `annexb` is portable and tested everywhere; `mf` (Windows)
//! drives Media Foundation's hardware HEVC encoder on the capture's D3D11 device.

pub mod annexb;
#[cfg(windows)]
pub mod mf;
pub mod sps;
