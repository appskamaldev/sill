//! Ports of upstream's checks: Tests/checks/door-policy (verify block, .ready, kind 19),
//! Tests/checks/addresses (PairingWindow), Tests/checks/device-gate, and the viewport-bounds
//! vectors generated from Swift.

use serde_json::Value;
use sill_pairing::{base64url, proof};
use sill_policy::device_gate as gate;
use sill_policy::door::{self, AtReady, Door, Pairing, Trust};
use sill_policy::pairing_window::{CloseReason, PairingWindow, Verdict, LIFETIME};
use sill_policy::viewport;
use sill_wire::{Goodbye, Hello, SillVersion, Viewport};

// MARK: - door-policy

const SESSION: &str = "sill/1";
const PAIRING: &str = "sill-pair/1";
const ALPNS: [Option<&str>; 7] =
    [Some(SESSION), Some(PAIRING), None, Some("h2"), Some("sill/2"), Some("SILL/1"), Some("")];

fn all_trusts() -> Vec<Trust> {
    (0..128u32)
        .map(|b| Trust {
            has_key: b & 1 != 0,
            paired: b & 2 != 0,
            require_pairing: b & 4 != 0,
            remote_access: b & 8 != 0,
            internet_access: b & 16 != 0,
            pairing_open: b & 32 != 0,
            remote_pairing_open: b & 64 != 0,
        })
        .collect()
}

fn trust_oracle(d: Door, alpn: Option<&str>, t: &Trust) -> bool {
    if !t.has_key {
        return false;
    }
    match alpn {
        Some("sill/1") => {
            if d == Door::Home {
                t.paired || !t.require_pairing
            } else {
                t.paired
            }
        }
        Some("sill-pair/1") => d == Door::Home || t.remote_pairing_open,
        _ => false,
    }
}

fn ready_oracle(d: Door, alpn: Option<&str>, t: &Trust, n: usize) -> AtReady {
    if !t.has_key {
        return AtReady::Refuse("unpaired");
    }
    match (d, alpn) {
        (Door::Home, Some("sill/1")) => {
            if t.paired || !t.require_pairing {
                AtReady::Session
            } else {
                AtReady::Refuse("unpaired")
            }
        }
        (Door::Home, Some("sill-pair/1")) => AtReady::Pairing,
        (Door::Remote, Some("sill/1")) => {
            if !t.paired {
                AtReady::Refuse("unpaired")
            } else if !t.remote_access {
                AtReady::Goodbye("remoteOff")
            } else if n >= 8 {
                AtReady::Goodbye("busy")
            } else {
                AtReady::Session
            }
        }
        (Door::Remote, Some("sill-pair/1")) if t.remote_pairing_open => AtReady::Pairing,
        _ => AtReady::Refuse("unpaired"),
    }
}

fn t(f: impl FnOnce(&mut Trust)) -> Trust {
    let mut x = Trust::default();
    f(&mut x);
    x
}

#[test]
fn verify_block_matches_the_table() {
    let mut n = 0;
    for d in [Door::Home, Door::Remote] {
        for a in ALPNS {
            for tr in all_trusts() {
                n += 1;
                assert_eq!(door::trusts(d, a, &tr), trust_oracle(d, a, &tr), "{d:?} {a:?} {tr:?}");
            }
        }
    }
    assert_eq!(n, 2 * 7 * 128);
    assert!(door::trusts(Door::Home, Some(SESSION), &t(|x| x.paired = true)));
    assert!(!door::trusts(Door::Home, Some(SESSION), &t(|x| x.require_pairing = true)));
    assert!(door::trusts(Door::Home, Some(SESSION), &t(|x| x.require_pairing = false)));
    assert!(!door::trusts(
        Door::Home,
        Some(SESSION),
        &t(|x| {
            x.has_key = false;
            x.require_pairing = false
        })
    ));
    assert!(door::trusts(Door::Home, Some(PAIRING), &t(|x| x.pairing_open = false)));
    assert!(!door::trusts(
        Door::Remote,
        Some(SESSION),
        &t(|x| {
            x.require_pairing = false;
            x.remote_access = true
        })
    ));
    assert!(door::trusts(Door::Remote, Some(SESSION), &t(|x| x.paired = true)));
    assert!(!door::trusts(Door::Remote, Some(PAIRING), &t(|x| x.pairing_open = true)));
    assert!(door::trusts(
        Door::Remote,
        Some(PAIRING),
        &t(|x| {
            x.pairing_open = true;
            x.remote_pairing_open = true
        })
    ));
    let everything = Trust {
        has_key: true,
        paired: true,
        require_pairing: false,
        remote_access: true,
        internet_access: false,
        pairing_open: true,
        remote_pairing_open: true,
    };
    for a in [None, Some("h2"), Some("sill/2"), Some("SILL/1"), Some("")] {
        assert!(!door::trusts(Door::Home, a, &everything) && !door::trusts(Door::Remote, a, &everything));
    }
}

#[test]
fn at_ready_matches_the_table() {
    let mut n = 0;
    for d in [Door::Home, Door::Remote] {
        for a in ALPNS {
            for tr in all_trusts() {
                for s in [0, 7, 8, 9] {
                    n += 1;
                    assert_eq!(door::at_ready(d, a, &tr, s), ready_oracle(d, a, &tr, s), "{d:?} {a:?} {tr:?} {s}");
                }
            }
        }
    }
    assert_eq!(n, 2 * 7 * 128 * 4);
    let paired_off = t(|x| x.paired = true);
    assert_eq!(door::at_ready(Door::Home, Some(SESSION), &paired_off, 100), AtReady::Session);
    assert_eq!(door::at_ready(Door::Remote, Some(SESSION), &paired_off, 8), AtReady::Goodbye(Goodbye::REMOTE_OFF));
    let paired_on = t(|x| {
        x.paired = true;
        x.remote_access = true
    });
    assert_eq!(door::at_ready(Door::Remote, Some(SESSION), &paired_on, 8), AtReady::Goodbye(Goodbye::BUSY));
    assert_eq!(door::at_ready(Door::Remote, Some(SESSION), &paired_on, 7), AtReady::Session);
}

#[test]
fn kind_19_by_door_and_generation() {
    assert_eq!(door::pairing(Door::Home, "ask", 1), Pairing::Ask);
    assert_eq!(door::pairing(Door::Home, "qr", 1), Pairing::Window);
    assert_eq!(door::pairing(Door::Home, "code", 1), Pairing::Window);
    for m in ["icloud", "", "ASK", "Ask", "ask "] {
        assert_eq!(door::pairing(Door::Home, m, 1), Pairing::Window, "{m}");
    }
    assert_eq!(door::pairing(Door::Remote, "ask", 1), Pairing::Closed);
    for m in ["qr", "code", "icloud", "", "ASK"] {
        assert_eq!(door::pairing(Door::Remote, m, 1), Pairing::Window, "{m}");
    }
    assert_eq!(door::pairing(Door::Home, "cancel", 1), Pairing::Withdraw);
    assert_eq!(door::pairing(Door::Remote, "cancel", 1), Pairing::Closed);
    assert_eq!(door::pairing(Door::Home, "cancel", 2), Pairing::Closed);
    for m in ["Cancel", "CANCEL", "cancel "] {
        assert_eq!(door::pairing(Door::Home, m, 1), Pairing::Window);
    }
    for d in [Door::Home, Door::Remote] {
        for m in ["ask", "qr", "code", "icloud", ""] {
            for v in [0, 2, 3, -1, i64::MAX] {
                assert_eq!(door::pairing(d, m, v), Pairing::Closed);
            }
        }
    }
}

// MARK: - PairingWindow (Tests/checks/addresses)

fn sha(s: &str) -> Vec<u8> {
    use sha2::Digest;
    sha2::Sha256::digest(s.as_bytes()).to_vec()
}

#[test]
fn pairing_window_sequence() {
    let (fp_mac, fp_dev) = (sha("mac"), sha("device"));
    let secret = [0x22u8; 16];
    let qr_d = proof::device_proof(&secret, &fp_dev, &fp_mac);
    let wrong = [7u8; 32];
    let mut w = PairingWindow::new();
    assert_eq!(w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "a", 0.0), Verdict::Closed(CloseReason::None));
    w.open(&secret, "482913557208", 100.0, LIFETIME, None, None, true);
    match w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "a", 101.0) {
        Verdict::Accept { proof_m } => {
            assert_eq!(base64url::encode(&proof_m), "XAX-uZPsq_Ax8fvI_sV7Ex0a-ROi19rI-FW-ykPh9TE")
        }
        v => panic!("QR not accepted: {v:?}"),
    }
    assert_eq!(w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "b", 102.0), Verdict::Closed(CloseReason::Used));
    w.open(&secret, "482913557208", 200.0, LIFETIME, Some("iPad".into()), None, true);
    assert_eq!(w.try_proof("qr", &qr_d, &fp_dev, &wrong, "a", 200.0), Verdict::Reject { tries_left: 4 });
    match w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "b", 200.5) {
        Verdict::Busy { retry_after } => assert!((retry_after - 0.5).abs() < 1e-9),
        v => panic!("{v:?}"),
    }
    assert!(matches!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "a", 203.0), Verdict::Busy { .. }));
    assert_eq!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "b", 201.1), Verdict::Reject { tries_left: 3 });
    assert!(matches!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "c", 202.9), Verdict::Busy { .. }));
    assert_eq!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "c", 203.2), Verdict::Reject { tries_left: 2 });
    assert!(matches!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "d", 207.0), Verdict::Busy { .. }));
    assert_eq!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "d", 207.3), Verdict::Reject { tries_left: 1 });
    assert!(matches!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "e", 215.0), Verdict::Busy { .. }));
    assert_eq!(w.try_proof("qr", &wrong, &fp_dev, &fp_mac, "e", 215.4), Verdict::Closed(CloseReason::Stopped));
    assert_eq!(w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "f", 230.0), Verdict::Closed(CloseReason::Stopped));
    w.open(&secret, "482913557208", 300.0, 5.0, None, None, true);
    assert_eq!(w.try_proof("qr", &qr_d, &fp_dev, &fp_mac, "a", 305.01), Verdict::Closed(CloseReason::Expired));
    w.open(&secret, "482913557208", 400.0, LIFETIME, None, None, true);
    assert!(!w.expire_if_due(699.9) && w.is_open());
    assert!(w.expire_if_due(700.0) && !w.is_open());
    // The typed code: busy before K, then accepted.
    w.open(&secret, "482913557208", 800.0, LIFETIME, None, None, true);
    let k = proof::code_key("482913557208", &fp_mac).unwrap();
    let code_d = proof::device_proof(&k, &fp_dev, &fp_mac);
    assert_eq!(w.try_proof("code", &code_d, &fp_dev, &fp_mac, "a", 800.05), Verdict::Busy { retry_after: 1.0 });
    w.set_code_key(k, "000000000000");
    assert_eq!(w.try_proof("code", &code_d, &fp_dev, &fp_mac, "a", 800.1), Verdict::Busy { retry_after: 1.0 });
    w.set_code_key(k, "482913557208");
    match w.try_proof("code", &code_d, &fp_dev, &fp_mac, "a", 800.2) {
        Verdict::Accept { proof_m } => {
            assert_eq!(base64url::encode(&proof_m), "6BmGWC9XT8J8OY8phhq9bASgoSnoWpRxBeqZtvRrWmk")
        }
        v => panic!("code not accepted: {v:?}"),
    }
    w.open(&secret, "482913557208", 900.0, LIFETIME, None, None, true);
    assert_eq!(w.try_proof("icloud", &qr_d, &fp_dev, &fp_mac, "a", 900.0), Verdict::Reject { tries_left: 4 });
    w.close(CloseReason::Cancelled);
    assert_eq!(w.close_reason(), Some(CloseReason::Cancelled));
    let mut x = PairingWindow::new();
    x.open(&secret, "482913557208", 0.0, LIFETIME, None, None, true);
    x.set_code_key(k, "482913557208");
    assert_eq!(x.try_proof("code", &qr_d, &fp_dev, &fp_mac, "a", 0.0), Verdict::Reject { tries_left: 4 });
}

#[test]
fn pairing_window_reasons_and_remote() {
    assert_eq!(PairingWindow::closed_reason(CloseReason::Stopped, true), "stopped");
    assert_eq!(PairingWindow::closed_reason(CloseReason::Stopped, false), "closed");
    assert_eq!(PairingWindow::closed_reason(CloseReason::Expired, false), "expired");
    for r in [CloseReason::Used, CloseReason::Cancelled, CloseReason::Withdrawn, CloseReason::None] {
        assert_eq!(PairingWindow::closed_reason(r, true), "closed");
    }
    let mut w = PairingWindow::new();
    assert!(!w.make_remote());
    w.open(&[0; 16], "482913557208", 0.0, LIFETIME, None, None, false);
    assert!(w.make_remote());
    assert!(!w.make_remote());
    assert!(w.current().unwrap().for_remote);
}

// MARK: - device-gate

fn h(v: Option<&str>, device: Option<&str>) -> Hello {
    Hello { app_version: v.map(String::from), device: device.map(String::from), ..Hello::default() }
}

#[test]
fn device_gate_checks() {
    let zero = SillVersion::ZERO;
    let f12 = SillVersion::parse("1.2").unwrap();
    let f01 = SillVersion::parse("0.1").unwrap();
    assert!(SillVersion::parse(gate::MINIMUM_DEVICE_VERSION).is_some());
    assert_eq!(gate::floor(), zero);
    let admitted_by_zero =
        [None, Some(h(None, None)), Some(h(Some(""), None)), Some(h(Some("junk"), None)), Some(h(Some("0"), None))];
    for hello in admitted_by_zero
        .iter()
        .chain([Some(h(Some("0.1"), None)), Some(h(Some("99.9.9"), None)), Some(h(Some("v2"), None))].iter())
    {
        assert!(gate::admits(hello.as_ref(), &zero));
    }
    for v in ["", "junk", "0.1", "1.1.9", "1.1", "1", "v1.1.99"] {
        assert!(!gate::admits(Some(&h(Some(v), None)), &f12), "{v}");
    }
    assert!(!gate::admits(None, &f12) && !gate::admits(Some(&h(None, None)), &f12));
    for v in ["1.2", "1.2.0", "1.10", "2", "v1.2", "1.2-beta.1", " 1.2.1 ", "1.2 (7)"] {
        assert!(gate::admits(Some(&h(Some(v), None)), &f12), "{v}");
    }
    assert!(!gate::admits(None, &f01) && !gate::admits(Some(&h(None, None)), &f01));
    assert_eq!(gate::version(None), zero);
    assert_eq!(gate::version(Some(&h(Some("dev"), None))), zero);
    assert_eq!(gate::version(Some(&h(Some("1.3"), None))), SillVersion::parse("1.3").unwrap());

    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("iPad (iPad14,1)")))), "iPad");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("iPhone (iPhone17,1)")))), "iPhone");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("iPad16,6 simulator")))), "iPad");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("sillclient")))), "device");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("sillclient iPhone")))), "device");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), Some("\u{202E}iPad")))), "iPad");
    assert_eq!(gate::device_word(Some(&h(Some("1.0"), None))), "device");
    assert_eq!(gate::device_word(None), "device");
    assert_eq!(
        gate::refusal(Some(&h(Some("1.1"), Some("iPad (iPad14,1)"))), &f12, "Mac mini"),
        Goodbye {
            reason: "update".into(),
            message: Some("Update Sill on your iPad to keep using Mac mini. It needs version 1.2 or later.".into()),
            minimum_version: Some("1.2".into()),
            reconnect: Some(false),
        }
    );
    assert_eq!(
        gate::message(
            Some(&h(Some("1.1"), Some("iPhone (iPhone17,1)"))),
            &SillVersion::parse("2").unwrap(),
            "Noah’s MacBook Pro"
        ),
        "Update Sill on your iPhone to keep using Noah’s MacBook Pro. It needs version 2.0 or later."
    );
    assert_eq!(
        gate::message(None, &SillVersion::parse("99").unwrap(), "Mac"),
        "Update Sill on your device to keep using Mac. It needs version 99.0 or later."
    );
    assert_eq!(gate::refusal(None, &SillVersion::parse("1.2.0").unwrap(), "M").minimum_version.as_deref(), Some("1.2"));
    assert!(gate::message(Some(&h(Some("1"), Some("iPad"))), &f12, "Mac mini").chars().count() <= 200);

    assert_eq!(
        gate::refused_line(Some(&h(Some("1.0"), Some("iPad (iPad14,1)"))), "192.168.1.23:52344", &f12),
        "Refused iPad (iPad14,1) (Sill 1.0): needs 1.2 or later."
    );
    assert_eq!(
        gate::refused_line(None, "192.168.1.23:52344", &f12),
        "Refused 192.168.1.23:52344 (an older Sill): needs 1.2 or later."
    );
    assert_eq!(
        gate::refused_line(Some(&h(None, None)), "127.0.0.1:5", &f12),
        "Refused 127.0.0.1:5 (no version): needs 1.2 or later."
    );
    assert!(!gate::refused_line(Some(&h(Some("1.0\nClient connected: forged"), Some("iPad\nforged"))), "e", &f12)
        .contains('\n'));
    assert_eq!(
        gate::count_line(14, "192.168.1.23", &f12),
        "Refused 14 more connections from 192.168.1.23 in the last minute: too old for this Mac (needs 1.2 or later)."
    );
    assert!(gate::count_line(1, "::1", &f12).starts_with("Refused 1 more connection from ::1 "));
    let full = Hello { build: Some("42".into()), protocol: Some(1), ..h(Some("1.0"), Some("iPad (iPad14,1)")) };
    assert_eq!(
        gate::hello_line(&full, "192.168.1.23:52344"),
        "Client hello: iPad (iPad14,1), Sill 1.0 (42), protocol 1 (192.168.1.23:52344)"
    );
    assert_eq!(gate::hello_line(&h(None, None), "192.168.1.23:52344"), "Client hello: 192.168.1.23:52344, no version");
    let no_build = Hello { protocol: Some(1), ..h(Some("1.0"), Some("iPad (iPad14,1)")) };
    assert_eq!(
        gate::hello_line(&no_build, "127.0.0.1:5"),
        "Client hello: iPad (iPad14,1), Sill 1.0, protocol 1 (127.0.0.1:5)"
    );
    let proto_only = Hello { protocol: Some(1), ..Hello::default() };
    assert_eq!(gate::hello_line(&proto_only, "e"), "Client hello: e, no version, protocol 1");
    assert_eq!(gate::hello_line(&h(Some(""), Some("iPad")), "e"), "Client hello: iPad, no version (e)");
    let forged = Hello { build: Some("4\r2".into()), ..h(Some("1\n2"), Some("a\u{2028}b")) };
    assert!(!gate::hello_line(&forged, "e").contains(['\n', '\r', '\u{2028}']));
    assert!(gate::hello_line(&h(Some(&"9".repeat(100)), Some("iPad")), "e").chars().count() < 100);
    assert!(!gate::slows(0) && !gate::slows(4));
    assert!(gate::slows(5) && gate::slows(50));
    assert_eq!((gate::FIRST_MESSAGE_DEADLINE, gate::LOOP_DELAY, gate::LOOP_WINDOW), (2.0, 2.0, 60.0));
}

// MARK: - viewport bounds (vectors)

fn load(name: &str) -> Value {
    let path = format!("{}/../../vectors/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

fn bits(v: &Value) -> Option<f64> {
    v.as_str().map(|s| f64::from_bits(u64::from_str_radix(s, 16).unwrap()))
}

fn same_bits(a: Option<f64>, b: Option<f64>) -> bool {
    a.map(f64::to_bits) == b.map(f64::to_bits)
}

#[test]
fn viewport_bounds_vectors() {
    let t = load("text.json");
    let vb = &t["viewportBounds"];
    for e in vb["viewports"].as_array().unwrap() {
        let v = Viewport {
            width: bits(&e["widthBits"]).unwrap(),
            height: bits(&e["heightBits"]).unwrap(),
            scale: bits(&e["scaleBits"]),
            local_cursor: None,
            fps: None,
        };
        assert_eq!(viewport::refusal(&v).as_deref(), e["refusal"].as_str(), "{}", e["name"]);
        match (viewport::held(&v), e["held"].as_object()) {
            (None, None) => {}
            (Some(h), Some(w)) => {
                assert!(same_bits(Some(h.width), bits(&w["widthBits"])), "{}", e["name"]);
                assert!(same_bits(Some(h.height), bits(&w["heightBits"])), "{}", e["name"]);
                assert!(same_bits(h.scale, bits(&w["scaleBits"])), "{}", e["name"]);
            }
            (got, _) => panic!("{}: held {got:?}, Swift {}", e["name"], e["held"]),
        }
    }
    for e in vb["whole"].as_array().unwrap() {
        assert_eq!(viewport::whole(bits(&e["xBits"]).unwrap()), e["whole"].as_str().unwrap());
    }
    for e in vb["number"].as_array().unwrap() {
        assert_eq!(viewport::number(bits(&e["xBits"]).unwrap()), e["number"].as_str().unwrap());
    }
}

#[test]
fn swift_double_description() {
    // Measured with Swift 6.4's `print(Double)`.
    let cases: [(f64, &str); 22] = [
        (1.0, "1.0"),
        (0.1, "0.1"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e+16"),
        (123456789012345.6, "123456789012345.6"),
        (1234567890123456.7, "1234567890123456.8"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (1e-300, "1e-300"),
        (1e300, "1e+300"),
        (5e-324, "5e-324"),
        (-0.0, "-0.0"),
        (f64::NAN, "nan"),
        (f64::INFINITY, "inf"),
        (f64::NEG_INFINITY, "-inf"),
        (9007199254740992.0, "9007199254740992.0"),
        (9007199254740994.0, "9.007199254740994e+15"),
        (1e16 + 2.0, "1.0000000000000002e+16"),
        (0.30000000000000004, "0.30000000000000004"),
        (1.5e-5, "1.5e-05"),
        (9.999999999999999e-05, "9.999999999999999e-05"),
        (-9007199254740994.0, "-9.007199254740994e+15"),
    ];
    for (x, want) in cases {
        assert_eq!(viewport::swift_double(x), want, "{x:?}");
    }
}
