//! The 14-byte header, the kinds and the caps (upstream `StreamMessage.swift`).

/// Header bytes: kind(1) timestamp(8, f64 bits, BE) isKeyframe(1) payloadLength(4, BE).
pub const HEADER_LENGTH: usize = 14;
/// Any client → host message, on both doors.
pub const MAX_CLIENT_PAYLOAD: usize = 1 << 20;
/// The one message of a pairing connection (kind 19).
pub const MAX_PAIRING_PAYLOAD: usize = 4096;
/// A video frame, as a device accepts it.
pub const MAX_FRAME_PAYLOAD: usize = 32 << 20;
/// Any other host → device message.
pub const MAX_OTHER_HOST_PAYLOAD: usize = 4 << 20;

macro_rules! kinds {
    ($($name:ident = $raw:literal, $text:literal;)*) => {
        /// A message kind. A kind this build does not know is `Unknown(raw)`: read and skipped,
        /// never a reason to stop reading (an older peer must keep working against a newer one).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Kind {
            $($name,)*
            Unknown(u8),
        }

        impl Kind {
            pub fn from_u8(raw: u8) -> Kind {
                match raw {
                    $($raw => Kind::$name,)*
                    other => Kind::Unknown(other),
                }
            }

            pub fn as_u8(self) -> u8 {
                match self {
                    $(Kind::$name => $raw,)*
                    Kind::Unknown(raw) => raw,
                }
            }

            /// Upstream's case name ("parameterSets"), or "unknown".
            pub fn name(self) -> &'static str {
                match self {
                    $(Kind::$name => $text,)*
                    Kind::Unknown(_) => "unknown",
                }
            }

            /// Every kind this build knows.
            pub const KNOWN: &'static [Kind] = &[$(Kind::$name),*];
        }
    };
}

kinds! {
    ParameterSets = 0, "parameterSets";
    Frame = 1, "frame";
    WindowList = 2, "windowList";
    Thumbnail = 3, "thumbnail";
    AppIcon = 4, "appIcon";
    AppList = 5, "appList";
    SelectSource = 6, "selectSource";
    LaunchApp = 7, "launchApp";
    Input = 8, "input";
    Viewport = 9, "viewport";
    Ping = 10, "ping";
    Pong = 11, "pong";
    ClientStats = 12, "clientStats";
    Tick = 13, "tick";
    CursorShape = 14, "cursorShape";
    WindowCommand = 15, "windowCommand";
    HostSettings = 16, "hostSettings";
    ChangeSettings = 17, "changeSettings";
    MacInfo = 18, "macInfo";
    PairRequest = 19, "pairRequest";
    PairResult = 20, "pairResult";
    PairingWanted = 21, "pairingWanted";
    Goodbye = 22, "goodbye";
    Hello = 23, "hello";
    MacMenu = 24, "macMenu";
    PressMenuItem = 25, "pressMenuItem";
    MacPointer = 26, "macPointer";
    FetchMenu = 27, "fetchMenu";
    Gesture = 28, "gesture";
    Clipboard = 32, "clipboard";
    Audio = 43, "audio";
    KeyframeRequest = 44, "keyframeRequest";
    NextDisplay = 47, "nextDisplay";
}

/// A parsed header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub kind: Kind,
    /// Host wall clock, seconds since 1970 (any f64 bit pattern is kept as sent).
    pub timestamp: f64,
    /// Only a byte of exactly 1 is a keyframe.
    pub is_keyframe: bool,
    pub payload_length: u32,
}

/// Parses the first 14 bytes of `data`; `None` when there are fewer. Never fails on the kind.
pub fn parse_header(data: &[u8]) -> Option<Header> {
    if data.len() < HEADER_LENGTH {
        return None;
    }
    let mut ts = [0u8; 8];
    ts.copy_from_slice(&data[1..9]);
    let mut len = [0u8; 4];
    len.copy_from_slice(&data[10..14]);
    Some(Header {
        kind: Kind::from_u8(data[0]),
        timestamp: f64::from_bits(u64::from_be_bytes(ts)),
        is_keyframe: data[9] == 1,
        payload_length: u32::from_be_bytes(len),
    })
}

/// One message: header fields and its payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub kind: Kind,
    pub timestamp: f64,
    pub is_keyframe: bool,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(kind: Kind, timestamp: f64, is_keyframe: bool, payload: Vec<u8>) -> Message {
        Message { kind, timestamp, is_keyframe, payload }
    }

    /// Header and payload, as `StreamMessage.serialized()` writes them. Panics only for a payload
    /// over 4 GiB, which no cap allows.
    pub fn serialized(&self) -> Vec<u8> {
        let len = u32::try_from(self.payload.len()).expect("payload over 4 GiB");
        let mut d = Vec::with_capacity(HEADER_LENGTH + self.payload.len());
        d.push(self.kind.as_u8());
        d.extend_from_slice(&self.timestamp.to_bits().to_be_bytes());
        d.push(u8::from(self.is_keyframe));
        d.extend_from_slice(&len.to_be_bytes());
        d.extend_from_slice(&self.payload);
        d
    }
}

/// What a reader allows a header to announce, by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limits {
    /// The host reading a session: every message at most `MAX_CLIENT_PAYLOAD`.
    HostSession,
    /// The host reading a pairing connection: at most `MAX_PAIRING_PAYLOAD`.
    HostPairing,
    /// A device reading the host: frames `MAX_FRAME_PAYLOAD`, anything else `MAX_OTHER_HOST_PAYLOAD`.
    Device,
}

impl Limits {
    pub fn cap(self, kind: Kind) -> usize {
        match self {
            Limits::HostSession => MAX_CLIENT_PAYLOAD,
            Limits::HostPairing => MAX_PAIRING_PAYLOAD,
            Limits::Device if kind == Kind::Frame => MAX_FRAME_PAYLOAD,
            Limits::Device => MAX_OTHER_HOST_PAYLOAD,
        }
    }
}

/// A header announced more than the reader takes: close the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge {
    pub kind: Kind,
    pub length: u32,
    pub cap: usize,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kind {} announces {} bytes, over the cap of {}", self.kind.as_u8(), self.length, self.cap)
    }
}

impl std::error::Error for TooLarge {}

/// An incremental reader: feed it bytes as they arrive, take messages out. A header over the
/// cap is refused as soon as its 14 bytes are in, before any of its payload is buffered, and the
/// reader then stays failed.
#[derive(Debug)]
pub struct FrameReader {
    limits: Limits,
    buffer: Vec<u8>,
    failed: Option<TooLarge>,
}

impl FrameReader {
    pub fn new(limits: Limits) -> FrameReader {
        FrameReader { limits, buffer: Vec::new(), failed: None }
    }

    /// Bytes held that do not make a whole message yet.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// Appends bytes and returns the first refusal, if the announced length is over the cap.
    /// Bytes are only kept up to what the current header may announce.
    pub fn push(&mut self, data: &[u8]) -> Result<(), TooLarge> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        self.buffer.extend_from_slice(data);
        self.check()
    }

    fn check(&mut self) -> Result<(), TooLarge> {
        if let Some(h) = parse_header(&self.buffer) {
            let cap = self.limits.cap(h.kind);
            if h.payload_length as usize > cap {
                let e = TooLarge { kind: h.kind, length: h.payload_length, cap };
                self.failed = Some(e);
                self.buffer = Vec::new();
                return Err(e);
            }
        }
        Ok(())
    }

    /// The next whole message, if one is in. Unknown kinds come out too (the caller skips them).
    pub fn next_message(&mut self) -> Result<Option<Message>, TooLarge> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        let Some(h) = parse_header(&self.buffer) else { return Ok(None) };
        let total = HEADER_LENGTH + h.payload_length as usize;
        if self.buffer.len() < total {
            return Ok(None);
        }
        let payload = self.buffer[HEADER_LENGTH..total].to_vec();
        self.buffer.drain(..total);
        self.check()?;
        Ok(Some(Message { kind: h.kind, timestamp: h.timestamp, is_keyframe: h.is_keyframe, payload }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, len: u32) -> Vec<u8> {
        Message::new(Kind::from_u8(kind), 1.0, false, Vec::new()).serialized()[..10]
            .iter()
            .copied()
            .chain(len.to_be_bytes())
            .collect()
    }

    #[test]
    fn refuses_over_cap_before_the_payload() {
        let mut r = FrameReader::new(Limits::HostSession);
        let h = header(8, (MAX_CLIENT_PAYLOAD + 1) as u32);
        assert!(r.push(&h[..13]).is_ok());
        assert!(r.push(&h[13..]).is_err());
        assert_eq!(r.buffered(), 0);
        assert!(r.push(b"more").is_err());
        assert!(r.next_message().is_err());
    }

    #[test]
    fn exactly_the_cap_is_taken() {
        let mut r = FrameReader::new(Limits::HostPairing);
        let mut m = header(19, MAX_PAIRING_PAYLOAD as u32);
        m.extend(std::iter::repeat(b'x').take(MAX_PAIRING_PAYLOAD));
        r.push(&m).unwrap();
        let got = r.next_message().unwrap().unwrap();
        assert_eq!(got.payload.len(), MAX_PAIRING_PAYLOAD);
        let mut r = FrameReader::new(Limits::HostPairing);
        assert!(r.push(&header(19, MAX_PAIRING_PAYLOAD as u32 + 1)).is_err());
    }

    #[test]
    fn device_caps_by_kind() {
        assert_eq!(Limits::Device.cap(Kind::Frame), MAX_FRAME_PAYLOAD);
        assert_eq!(Limits::Device.cap(Kind::WindowList), MAX_OTHER_HOST_PAYLOAD);
        assert_eq!(Limits::Device.cap(Kind::Unknown(200)), MAX_OTHER_HOST_PAYLOAD);
        let mut r = FrameReader::new(Limits::Device);
        assert!(r.push(&header(1, (MAX_OTHER_HOST_PAYLOAD + 1) as u32)).is_ok());
        let mut r = FrameReader::new(Limits::Device);
        assert!(r.push(&header(2, (MAX_OTHER_HOST_PAYLOAD + 1) as u32)).is_err());
    }

    #[test]
    fn byte_by_byte_and_back_to_back_with_unknown_kinds() {
        let a = Message::new(Kind::Ping, 2.5, false, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        let b = Message::new(Kind::Unknown(200), 3.0, true, vec![9; 3]);
        let c = Message::new(Kind::Tick, 4.0, false, Vec::new());
        let stream: Vec<u8> = [a.serialized(), b.serialized(), c.serialized()].concat();
        let mut r = FrameReader::new(Limits::HostSession);
        let mut out = Vec::new();
        for byte in stream {
            r.push(&[byte]).unwrap();
            while let Some(m) = r.next_message().unwrap() {
                out.push(m);
            }
        }
        assert_eq!(out, vec![a, b, c]);
        assert_eq!(r.buffered(), 0);
    }

    #[test]
    fn a_later_over_cap_header_in_the_same_push_is_refused() {
        let ok = Message::new(Kind::Ping, 1.0, false, vec![0; 8]).serialized();
        let bad = header(8, u32::MAX);
        let mut r = FrameReader::new(Limits::HostSession);
        r.push(&[ok, bad].concat()).unwrap();
        assert!(r.next_message().is_err());
    }
}
