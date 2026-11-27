//! sill-pairing against the golden vectors generated from upstream Swift (windows/vectors/).

use serde_json::Value;
use sill_pairing::*;
use std::collections::HashMap;

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
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

fn address_json(a: &ParsedAddress) -> Value {
    serde_json::json!({"host": a.host, "port": a.port, "kind": a.kind.as_str(), "text": a.text()})
}

#[test]
fn pairing_code() {
    let p = load("pairing.json");
    assert_eq!(p["codeLength"], code::LENGTH);
    for e in arr(&p, "damm") {
        let digits: Vec<u8> = arr(e, "digits").iter().map(|d| d.as_u64().unwrap() as u8).collect();
        assert_eq!(code::check_digit(&digits) as u64, e["check"].as_u64().unwrap(), "{digits:?}");
    }
    for e in arr(&p, "check") {
        let got = code::check(s(&e["input"]));
        match e.get("ok") {
            Some(ok) => assert_eq!(got.as_deref(), Ok(s(ok)), "{:?}", e["input"]),
            None => {
                let want = if s(&e["problem"]) == "length" { CodeProblem::Length } else { CodeProblem::Typo };
                assert_eq!(got, Err(want), "{:?}", e["input"]);
            }
        }
    }
    for e in arr(&p, "grouped") {
        assert_eq!(code::grouped(s(&e["code"])), s(&e["grouped"]));
    }
}

#[test]
fn proofs_and_code_keys() {
    let p = load("pairing.json");
    assert_eq!(p["rounds"], proof::ROUNDS);
    for e in arr(&p, "codeKeys") {
        let got = proof::code_key(s(&e["code"]), &unhex(s(&e["fpMac"])));
        match e["key"].as_str() {
            Some(k) => assert_eq!(hex(&got.expect("a key")), k, "{}", e["code"]),
            None => assert!(got.is_none(), "{}", e["code"]),
        }
    }
    for e in arr(&p, "proofs") {
        let (key, dev, mac) = (unhex(s(&e["key"])), unhex(s(&e["fpDevice"])), unhex(s(&e["fpMac"])));
        let d = proof::device_proof(&key, &dev, &mac);
        let m = proof::mac_proof(&key, &mac, &dev);
        assert_eq!(hex(&d), s(&e["deviceProof"]), "{}", e["name"]);
        assert_eq!(hex(&m), s(&e["macProof"]), "{}", e["name"]);
        assert_eq!(base64url::encode(&d), s(&e["deviceProofB64"]));
        assert_eq!(base64url::encode(&m), s(&e["macProofB64"]));
        assert_eq!(proof::is_valid_device_proof(&d, &key, &dev, &mac), e["deviceValid"].as_bool().unwrap());
        assert_eq!(proof::is_valid_device_proof(&d, &key, &mac, &dev), e["swappedValid"].as_bool().unwrap());
        assert_eq!(proof::is_valid_device_proof(&m, &key, &dev, &mac), e["macAsDeviceValid"].as_bool().unwrap());
        assert_eq!(proof::is_valid_device_proof(&d[..31], &key, &dev, &mac), e["truncatedValid"].as_bool().unwrap());
        assert_eq!(proof::is_valid_mac_proof(&m, &key, &mac, &dev), e["macValid"].as_bool().unwrap());
    }
    // docs/remote-access-plan.md's vectors, as Tests/checks/addresses checks them.
    let qr = arr(&p, "proofs").iter().find(|e| e["name"] == "qr").unwrap();
    assert_eq!(s(&qr["macProofB64"]), "XAX-uZPsq_Ax8fvI_sV7Ex0a-ROi19rI-FW-ykPh9TE");
    let code = arr(&p, "proofs").iter().find(|e| e["name"] == "code").unwrap();
    assert_eq!(s(&code["macProofB64"]), "6BmGWC9XT8J8OY8phhq9bASgoSnoWpRxBeqZtvRrWmk");
}

#[test]
fn recognition_tag_and_door() {
    let p = load("pairing.json");
    let r = &p["recognitionTag"];
    let nonce: [u8; 6] = unhex(s(&r["nonce"])).try_into().unwrap();
    assert_eq!(tag::make_tag_with_nonce(&unhex(s(&r["key"])), nonce), s(&r["tag"]));
    assert_eq!(s(&r["tag"]), "oaKjpKWmL4V47Ctm");
    let nonce2: [u8; 6] = unhex(s(&r["nonce2"])).try_into().unwrap();
    assert_eq!(tag::make_tag_with_nonce(&unhex(s(&r["key2"])), nonce2), s(&r["tag2"]));
    assert_eq!(s(&r["txtKey"]), tag::TAG_TXT_KEY);
    for e in arr(r, "matches") {
        assert_eq!(tag::tag_matches(s(&e["tag"]), &unhex(s(&e["key"]))), e["matches"].as_bool().unwrap(), "{e}");
    }
    let made = tag::make_tag(&[0x11; 32], |b| {
        b.copy_from_slice(&[0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6]);
        true
    });
    assert_eq!(made.as_deref(), Some("oaKjpKWmL4V47Ctm"));
    let d = &p["homeDoorTXT"];
    assert_eq!(s(&d["key"]), tag::DOOR_TXT_KEY);
    assert_eq!(s(&d["valueOn"]), tag::door_value(true));
    assert_eq!(s(&d["valueOff"]), tag::door_value(false));
    for e in arr(d, "doors") {
        let txt: HashMap<String, String> =
            e["txt"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), s(v).to_string())).collect();
        let want = match s(&e["door"]) {
            "plain" => Door::Plain,
            "open" => Door::Open,
            _ => Door::PairingRequired,
        };
        assert_eq!(tag::door(&txt), want, "{txt:?}");
    }
}

#[test]
fn base64url_spki_and_mac_id() {
    let p = load("pairing.json");
    for e in arr(&p["base64url"], "encode") {
        assert_eq!(base64url::encode(&unhex(s(&e["bytes"]))), s(&e["text"]));
    }
    for e in arr(&p["base64url"], "decode") {
        assert_eq!(
            base64url::decode(s(&e["text"])).map(|d| hex(&d)),
            e["bytes"].as_str().map(String::from),
            "{:?}",
            e["text"]
        );
    }
    for e in arr(&p, "macID") {
        let id = mac_id(&unhex(s(&e["fingerprint"])));
        assert_eq!(id, s(&e["macID"]));
        assert_eq!(mac_id_is_well_formed(&id), e["wellFormed"].as_bool().unwrap());
    }
    for e in arr(&p, "macIDWellFormed") {
        assert_eq!(mac_id_is_well_formed(s(&e["text"])), e["wellFormed"].as_bool().unwrap(), "{}", e["text"]);
    }
    for e in arr(&p, "spki") {
        let point = unhex(s(&e["point"]));
        assert_eq!(hex(&spki_der(&point)), s(&e["der"]));
        let fp = spki_fingerprint(&point);
        assert_eq!(hex(&fp), s(&e["fingerprint"]));
        assert_eq!(mac_id(&fp), s(&e["macID"]));
        assert_eq!(spki::short_name(&fp), s(&e["shortName"]));
    }
    let a = &p["alpn"];
    assert_eq!(s(&a["session"]), SESSION_ALPN);
    assert_eq!(s(&a["pairing"]), PAIRING_ALPN);
    assert_eq!(a["server"], serde_json::json!(SERVER_ALPNS));
    assert_eq!(s(&a["serverName"]), SERVER_NAME);
    assert_eq!(p["pairRequestVersion"], sill_wire::PairRequest::VERSION);
    let known: Vec<&str> = arr(&p, "knownReasons").iter().map(s).collect();
    assert_eq!(known, sill_wire::PairResult::KNOWN_REASONS);
}

#[test]
fn pair_links() {
    let p = load("pairing.json");
    let l = &p["pairLink"];
    assert_eq!(l["maxAddresses"], link::MAX_ADDRESSES);
    assert_eq!(l["nameLimit"], link::NAME_LIMIT);
    for e in arr(l, "urls") {
        let addresses: Vec<ParsedAddress> =
            arr(e, "addresses").iter().map(|a| address::parse(s(&a["text"])).unwrap()).collect();
        // The vector's name is the cleaned one; clean again (idempotent) from it.
        let built = PairLink::new(
            &unhex(s(&e["fingerprint"])),
            &unhex(s(&e["secret"])),
            s(&e["name"]),
            e["port"].as_u64().unwrap() as u16,
            &addresses,
        );
        assert_eq!(built.url(), s(&e["url"]));
        assert_eq!(built.mac_id, s(&e["macID"]));
    }
    // The raw names the generator gave PairLink(…), cleaned as upstream cleans them.
    let fp = unhex(s(&arr(l, "urls")[0]["fingerprint"]));
    assert_eq!(PairLink::new(&fp, &[0x22; 16], "  \u{202E}\n", 1, &[]).name, "Mac");
    assert_eq!(PairLink::new(&fp, &[0x22; 16], &"N".repeat(70), 1, &[]).name, "N".repeat(64));
    for e in arr(l, "parse") {
        let text = s(&e["text"]);
        match (PairLink::parse(text), e.get("ok")) {
            (Ok(got), Some(ok)) => {
                assert_eq!(got.mac_id, s(&ok["macID"]), "{text}");
                assert_eq!(hex(&got.fingerprint), s(&ok["fingerprint"]));
                assert_eq!(hex(&got.secret), s(&ok["secret"]));
                assert_eq!(got.name, s(&ok["name"]), "{text}");
                assert_eq!(got.port as u64, ok["port"].as_u64().unwrap());
                let addrs: Vec<Value> = got.addresses.iter().map(address_json).collect();
                assert_eq!(Value::Array(addrs), ok["addresses"], "{text}");
            }
            (Err(f), None) => {
                let ours = match f {
                    LinkFailure::NotALink => "notALink".to_string(),
                    LinkFailure::Version => "version".to_string(),
                    LinkFailure::Field(k) => format!("field:{k}"),
                    LinkFailure::MacIdMismatch => "macIDMismatch".to_string(),
                    LinkFailure::Address(a) => format!("address:{a}"),
                };
                assert_eq!(ours, s(&e["failure"]), "{text}");
            }
            (got, _) => panic!("{text}: Rust {got:?}, Swift {e}"),
        }
    }
}

#[test]
fn signed_mac_info() {
    let j = load("json.json");
    let v = &j["signedMacInfo"];
    for case in ["valid", "tampered", "wrongKey", "rustSigned"] {
        let c = &v[case];
        let signed =
            sill_wire::SignedMacInfo { info: s(&c["info"]).into(), key: s(&c["key"]).into(), sig: s(&c["sig"]).into() };
        let got = signed_info::verify(&signed);
        assert_eq!(got.is_some(), c["verifies"].as_bool().unwrap(), "{case}");
        if let Some((info, fp)) = got {
            assert_eq!(hex(&fp), s(&v["valid"]["fingerprint"]));
            assert_eq!(info.remote_port, 7455);
        }
    }
    // The fixed key signs, and its signature verifies here; the public key is upstream's.
    let key = p256::ecdsa::SigningKey::from_slice(&unhex(s(&v["privateKey"]))).unwrap();
    assert_eq!(hex(key.verifying_key().to_encoded_point(false).as_bytes()), s(&v["publicKey"]));
    // Our signature over upstream's bytes is the one upstream's `verified()` took.
    let over_bytes = signed_info::sign_bytes(&unhex(s(&v["infoBytes"])), &key);
    assert_eq!(over_bytes.sig, s(&v["rustSigned"]["sig"]));
    assert!(v["rustSigned"]["verifies"].as_bool().unwrap());
    assert_eq!(over_bytes.key, s(&v["valid"]["key"]));
    let info: sill_wire::MacInfo = sill_wire::decode(&unhex(s(&v["infoBytes"]))).unwrap();
    let ours = signed_info::sign(&info, &key);
    let (back, _) = signed_info::verify(&ours).expect("our own signature verifies");
    assert_eq!(back, info);
    let mut flipped = ours.clone();
    flipped.info =
        sill_wire::swift_json::data_to_base64(&sill_wire::encode(&sill_wire::MacInfo { remote_port: 7456, ..info }));
    assert!(signed_info::verify(&flipped).is_none());
}

// MARK: - text.json

#[test]
fn addresses() {
    let t = load("text.json");
    let list = arr(&t, "addresses");
    assert!(list.len() > 140);
    for e in list {
        let input = s(&e["input"]);
        match (address::parse(input), e.get("ok")) {
            (Ok(a), Some(ok)) => assert_eq!(address_json(&a), *ok, "{input:?}"),
            (Err(f), None) => assert_eq!(f.as_str(), s(&e["failure"]), "{input:?}"),
            (got, _) => panic!("{input:?}: Rust {got:?}, Swift {e}"),
        }
    }
}

#[test]
fn safe_text_labels() {
    let t = load("text.json");
    assert_eq!(t["safeText"]["labelLimit"], safe_text::LABEL_LIMIT);
    for e in arr(&t["safeText"], "labels") {
        let text = s(&e["text"]);
        let got = match e["limit"].as_u64() {
            Some(l) => safe_text::label(text, l as usize),
            None => safe_text::label64(text),
        };
        assert_eq!(got, s(&e["label"]), "{text:?}");
    }
}
