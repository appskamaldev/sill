//! Sill's wire format for the Windows host, ported from upstream `Sources/StreamProtocol/`
//! (the specification) and checked against golden vectors generated from it
//! (`windows/vectors/`). Portable: no platform APIs.
#![forbid(unsafe_code)]

pub mod blobs;
pub mod header;
pub mod payloads;
pub mod swift_json;
pub mod version;

pub use blobs::{decode_icon, decode_thumbnail, encode_icon, encode_thumbnail, CursorShape, ParameterSets};
pub use header::{
    parse_header, FrameReader, Header, Kind, Limits, Message, TooLarge, HEADER_LENGTH, MAX_CLIENT_PAYLOAD,
    MAX_FRAME_PAYLOAD, MAX_OTHER_HOST_PAYLOAD, MAX_PAIRING_PAYLOAD,
};
pub use payloads::*;
pub use version::{SillVersion, PROTOCOL_CURRENT};
