//! sill-wire against the golden vectors generated from upstream Swift (windows/vectors/).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sill_wire::*;

fn load(name: &str) -> Value {
    let path = format!("{}/../../vectors/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a Vec<Value> {
    v[key].as_array().unwrap_or_else(|| panic!("no array {key}"))
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// JSON equality as a decoder sees it: numbers by value (Swift writes 1.0 as `1`), objects by
/// key set regardless of order.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64(), x.as_u64(), y.as_u64()) {
            (Some(p), Some(q), _, _) => p == q,
            (_, _, Some(p), Some(q)) => p == q,
            _ => x.as_f64() == y.as_f64(),
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

// MARK: - wire.json

#[test]
fn caps_and_kinds() {
    let w = load("wire.json");
    let c = &w["caps"];
    assert_eq!(c["headerLength"], HEADER_LENGTH);
    assert_eq!(c["maxClientPayload"], MAX_CLIENT_PAYLOAD);
    assert_eq!(c["maxPairingPayload"], MAX_PAIRING_PAYLOAD);
    assert_eq!(c["maxFramePayload"], MAX_FRAME_PAYLOAD);
    assert_eq!(c["maxOtherHostPayload"], MAX_OTHER_HOST_PAYLOAD);
    assert_eq!(w["protocol"]["current"], PROTOCOL_CURRENT);
    let kinds = arr(&w, "kinds");
    // Swift's list includes its `unknown = 255` placeholder; ours is Kind::Unknown(_).
    let swift: Vec<(u64, &str)> =
        kinds.iter().map(|k| (k["raw"].as_u64().unwrap(), s(&k["name"]))).filter(|(_, n)| *n != "unknown").collect();
    let ours: Vec<(u64, &str)> = Kind::KNOWN.iter().map(|k| (k.as_u8() as u64, k.name())).collect();
    assert_eq!(swift, ours);
    for raw in 0..=255u8 {
        assert_eq!(Kind::from_u8(raw).as_u8(), raw);
    }
}

#[test]
fn headers_serialize_as_swift() {
    let w = load("wire.json");
    for h in arr(&w, "headers") {
        let ts = f64::from_bits(u64::from_str_radix(s(&h["timestampBits"]), 16).unwrap());
        let kind = Kind::from_u8(h["kind"].as_u64().unwrap() as u8);
        let m = Message::new(kind, ts, h["keyframe"].as_bool().unwrap(), unhex(s(&h["payload"])));
        assert_eq!(hex(&m.serialized()), s(&h["serialized"]), "{}", h["name"]);
    }
}

#[test]
fn parse_header_as_swift() {
    let w = load("wire.json");
    for p in arr(&w, "parseHeader") {
        let got = parse_header(&unhex(s(&p["bytes"])));
        let name = &p["name"];
        if !p["ok"].as_bool().unwrap() {
            assert!(got.is_none(), "{name}");
            continue;
        }
        let h = got.unwrap_or_else(|| panic!("{name}"));
        let swift_kind = p["kind"].as_u64().unwrap() as u8;
        if swift_kind == 255 {
            assert!(matches!(h.kind, Kind::Unknown(_)), "{name}: {:?}", h.kind);
        } else {
            assert_eq!(h.kind, Kind::from_u8(swift_kind), "{name}");
        }
        assert_eq!(format!("{:016x}", h.timestamp.to_bits()), s(&p["timestampBits"]), "{name}");
        assert_eq!(h.is_keyframe, p["keyframe"].as_bool().unwrap(), "{name}");
        assert_eq!(h.payload_length as u64, p["payloadLength"].as_u64().unwrap(), "{name}");
    }
}

#[test]
fn parameter_sets() {
    let w = load("wire.json");
    for e in arr(&w["parameterSets"], "encode") {
        let p = ParameterSets {
            nal_unit_header_length: e["nalLen"].as_u64().unwrap() as u8,
            sets: arr(e, "sets").iter().map(|x| unhex(s(x))).collect(),
        };
        assert_eq!(hex(&p.encoded()), s(&e["encoded"]));
        assert_eq!(ParameterSets::decode(&p.encoded()), Some(p));
    }
    for e in arr(&w["parameterSets"], "decode") {
        let got = ParameterSets::decode(&unhex(s(&e["bytes"])));
        if e["ok"].as_bool().unwrap() {
            let p = got.unwrap_or_else(|| panic!("{}", e["name"]));
            assert_eq!(p.nal_unit_header_length as u64, e["nalLen"].as_u64().unwrap());
            let sets: Vec<String> = arr(e, "sets").iter().map(|x| s(x).to_string()).collect();
            assert_eq!(p.sets.iter().map(|x| hex(x)).collect::<Vec<_>>(), sets, "{}", e["name"]);
        } else {
            assert!(got.is_none(), "{}", e["name"]);
        }
    }
}

#[test]
fn image_blobs() {
    let w = load("wire.json");
    for e in arr(&w["thumbnail"], "encode") {
        let got = encode_thumbnail(e["windowID"].as_u64().unwrap() as u32, &unhex(s(&e["jpeg"])));
        assert_eq!(hex(&got), s(&e["encoded"]));
    }
    for e in arr(&w["thumbnail"], "decode") {
        let bytes = unhex(s(&e["bytes"]));
        match decode_thumbnail(&bytes) {
            Some((id, jpeg)) => {
                assert!(e["ok"].as_bool().unwrap(), "{}", e["bytes"]);
                assert_eq!(id as u64, e["windowID"].as_u64().unwrap());
                assert_eq!(hex(jpeg), s(&e["jpeg"]));
            }
            None => assert!(!e["ok"].as_bool().unwrap(), "{}", e["bytes"]),
        }
    }
    for e in arr(&w["appIcon"], "encode") {
        assert_eq!(hex(&encode_icon(s(&e["bundleID"]), &unhex(s(&e["png"])))), s(&e["encoded"]));
    }
    for e in arr(&w["appIcon"], "decode") {
        let bytes = unhex(s(&e["bytes"]));
        match decode_icon(&bytes) {
            Some((id, png)) => {
                assert!(e["ok"].as_bool().unwrap(), "{}", e["bytes"]);
                assert_eq!(id, s(&e["bundleID"]));
                assert_eq!(hex(png), s(&e["png"]));
            }
            None => assert!(!e["ok"].as_bool().unwrap(), "{}", e["bytes"]),
        }
    }
}

#[test]
fn cursor_shapes() {
    let w = load("wire.json");
    for e in arr(&w["cursorShape"], "encode") {
        let f = |k: &str| e[k].as_f64().unwrap() as f32;
        let c = CursorShape {
            hot_x: f("hotX"),
            hot_y: f("hotY"),
            width: f("width"),
            height: f("height"),
            png: unhex(s(&e["png"])),
        };
        assert_eq!(hex(&c.encoded()), s(&e["encoded"]));
    }
    for e in arr(&w["cursorShape"], "decode") {
        match CursorShape::decode(&unhex(s(&e["bytes"]))) {
            Some(c) => {
                assert!(e["ok"].as_bool().unwrap());
                for (v, k) in
                    [(c.hot_x, "hotXBits"), (c.hot_y, "hotYBits"), (c.width, "widthBits"), (c.height, "heightBits")]
                {
                    assert_eq!(format!("{:08x}", v.to_bits()), s(&e[k]));
                }
                assert_eq!(hex(&c.png), s(&e["png"]));
            }
            None => assert!(!e["ok"].as_bool().unwrap(), "{}", e["bytes"]),
        }
    }
}

// MARK: - json.json

/// Decodes `input` as Swift's type `name` would, then encodes it back: `None` when refused.
fn roundtrip(name: &str, input: &str) -> Option<Value> {
    fn rt<T: DeserializeOwned + Serialize>(input: &str) -> Option<Value> {
        let v: T = decode(input.as_bytes())?;
        let out = encode(&v);
        // Our own output must decode again.
        let again: T = decode(&out).expect("our output decodes");
        assert_eq!(encode(&again), out);
        Some(serde_json::from_slice(&out).unwrap())
    }
    match name {
        "StreamSource" => rt::<StreamSource>(input),
        "InputEvent" => rt::<InputEvent>(input),
        "Viewport" => rt::<Viewport>(input),
        "ClientStats" => rt::<ClientStats>(input),
        "Hello" => rt::<Hello>(input),
        "WindowInfo" => rt::<WindowInfo>(input),
        "AppInfo" => rt::<AppInfo>(input),
        "WindowList" => rt::<WindowList>(input),
        "LaunchApp" => rt::<LaunchApp>(input),
        "WindowCommand" => rt::<WindowCommand>(input),
        "StreamSettings" => rt::<StreamSettings>(input),
        "HostSettingsState" => rt::<HostSettingsState>(input),
        "HostSettingsChange" => rt::<HostSettingsChange>(input),
        "Goodbye" => rt::<Goodbye>(input),
        "PairRequest" => rt::<PairRequest>(input),
        "PairResult" => rt::<PairResult>(input),
        "MacInfo" => rt::<MacInfo>(input),
        "MacAddress" => rt::<MacAddress>(input),
        "SignedMacInfo" => rt::<SignedMacInfo>(input),
        "TrackpadGesture" => rt::<TrackpadGesture>(input),
        "Clipboard" => rt::<Clipboard>(input),
        "MacPointer" => rt::<MacPointer>(input),
        other => panic!("no Rust type for {other}"),
    }
}

#[test]
fn json_encodes_match_swift() {
    let j = load("json.json");
    let encodes = arr(&j, "encode");
    assert!(encodes.len() > 50);
    for e in encodes {
        let (ty, name) = (s(&e["type"]), s(&e["name"]));
        for key in ["json", "sorted"] {
            let swift: Value = serde_json::from_str(s(&e[key])).unwrap();
            let ours = roundtrip(ty, s(&e[key])).unwrap_or_else(|| panic!("{ty} {name}: Rust refused Swift's {key}"));
            assert!(same(&ours, &swift), "{ty} {name}\n swift {swift}\n rust  {ours}");
        }
    }
}

#[test]
fn json_decodes_match_swift() {
    let j = load("json.json");
    for e in arr(&j, "decode") {
        let (ty, input) = (s(&e["type"]), s(&e["input"]));
        let got = roundtrip(ty, input);
        if e["ok"].as_bool().unwrap() {
            let ours = got.unwrap_or_else(|| panic!("{ty}: Swift took, Rust refused {input}"));
            let swift: Value = serde_json::from_str(s(&e["sorted"])).unwrap();
            assert!(same(&ours, &swift), "{ty} {input}\n swift {swift}\n rust  {ours}");
        } else {
            assert!(got.is_none(), "{ty}: Swift refused, Rust took {input}");
        }
    }
}

#[test]
fn json_constructed_values_match_swift() {
    // What the Windows host itself will build, compared with Swift's encoding of the same values.
    let j = load("json.json");
    let swift = |ty: &str, name: &str| -> Value {
        let e = arr(&j, "encode").iter().find(|e| e["type"] == ty && e["name"] == name).unwrap();
        serde_json::from_str(s(&e["json"])).unwrap()
    };
    let settings = StreamSettings {
        max_fps: 60,
        bitrate: 15_000_000,
        capture_scale: 1.0,
        prioritize_speed: true,
        virtual_display: false,
        direct_wireless: None,
        send_audio: None,
    };
    let state = HostSettingsState {
        settings,
        persistent: false,
        virtual_display_available: false,
        virtual_display_note: None,
        software_encoder: false,
        stream: None,
        answering: None,
        away: None,
        link: None,
        audio_note: None,
        session_paired: None,
    };
    let cases: Vec<(Value, Value)> = vec![
        (serde_json::to_value(&state).unwrap(), swift("HostSettingsState", "the Windows MVP's")),
        (
            serde_json::to_value(WindowList::new("PC", vec![], StreamSource::None)).unwrap(),
            swift("WindowList", "minimal"),
        ),
        (serde_json::to_value(Goodbye::reason(Goodbye::QUIT)).unwrap(), swift("Goodbye", "quit")),
        (serde_json::to_value(StreamSource::Window(1234)).unwrap(), swift("StreamSource", "window")),
        (
            serde_json::to_value(InputEvent::Pointer { action: PointerAction::LeftDown, x: 0.25, y: 0.5 }).unwrap(),
            swift("InputEvent", "pointer leftDown"),
        ),
        (
            serde_json::to_value(InputEvent::Key { hid_usage: 4, down: true, modifiers: 1 << 20 }).unwrap(),
            swift("InputEvent", "key"),
        ),
        (
            serde_json::to_value(PairResult { tries_left: Some(4), ..PairResult::refusal(PairResult::CODE) }).unwrap(),
            swift("PairResult", "wrong code"),
        ),
    ];
    for (ours, theirs) in cases {
        assert!(same(&ours, &theirs), "swift {theirs}\n rust  {ours}");
    }
}

// MARK: - text.json: versions

#[test]
fn sill_versions() {
    let t = load("text.json");
    for e in arr(&t["versions"], "parse") {
        let got = SillVersion::parse(s(&e["text"]));
        match e["components"].as_array() {
            None => assert!(got.is_none(), "{:?} should not parse", e["text"]),
            Some(c) => {
                let v = got.unwrap_or_else(|| panic!("{:?} should parse", e["text"]));
                let want: Vec<u32> = c.iter().map(|x| x.as_u64().unwrap() as u32).collect();
                assert_eq!(v.components(), want.as_slice(), "{:?}", e["text"]);
                assert_eq!(v.to_string(), s(&e["description"]));
            }
        }
    }
    for e in arr(&t["versions"], "compare") {
        let (a, b) = (SillVersion::parse(s(&e["a"])).unwrap(), SillVersion::parse(s(&e["b"])).unwrap());
        let order = match a.cmp(&b) {
            std::cmp::Ordering::Less => "less",
            std::cmp::Ordering::Equal => "equal",
            std::cmp::Ordering::Greater => "greater",
        };
        assert_eq!(order, s(&e["order"]), "{} vs {}", e["a"], e["b"]);
        assert_eq!(a == b, order == "equal");
    }
    for e in arr(&t["versions"], "describe") {
        let c: Vec<u32> = arr(e, "components").iter().map(|x| x.as_u64().unwrap() as u32).collect();
        assert_eq!(SillVersion::from_components(c).to_string(), s(&e["description"]));
    }
}
