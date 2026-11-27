//! The few Windows API calls the host makes outside the video path: DPAPI for the stored identity
//! and Bonjour (`DnsServiceRegister`) for the home door. Everything here is behind `cfg(windows)`;
//! other platforms get stand-ins so the portable crates build and test on macOS.

pub mod adapters;
pub mod bonjour;
pub mod protect;
