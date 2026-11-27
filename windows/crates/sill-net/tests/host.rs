//! The host over loopback with a rustls client that does what the device does: TLS 1.3, its own
//! P-256 certificate, one ALPN, the host pinned by its SPKI fingerprint.

use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{AlertDescription, ClientConfig, DigitallySignedStruct, SignatureScheme};
use sill_net::{Clip, Config, Host, Running};
use sill_pairing::{base64url, link::PairLink, proof, spki, PAIRING_ALPN, SESSION_ALPN};
use sill_store::Store;
use sill_wire::{decode, encode, FrameReader, Goodbye, Kind, Limits, Message, PairRequest, PairResult, WindowList};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

const WAIT: Duration = Duration::from_secs(5);

struct Device {
    cert: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    fp: [u8; 32],
}

fn device() -> Device {
    let kp = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = rcgen::CertificateParams::default().self_signed(&kp).unwrap();
    let point = sill_net::cert::p256_point(cert.der()).unwrap();
    Device {
        cert: cert.der().clone(),
        key: PrivatePkcs8KeyDer::from(kp.serialize_der()),
        fp: spki::spki_fingerprint(&point),
    }
}

/// Takes any server key (the code's proofs bind it) and remembers its fingerprint.
#[derive(Debug, Default)]
struct Pin(Mutex<Option<[u8; 32]>>);

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let p = sill_net::cert::p256_point(cert).ok_or(rustls::Error::General("not P-256".into()))?;
        *self.0.lock().unwrap() = Some(spki::spki_fingerprint(&p));
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            m,
            c,
            d,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
}

struct Conn {
    s: TlsStream<TcpStream>,
    reader: FrameReader,
    host_fp: [u8; 32],
}

async fn dial(port: u16, alpn: &str, d: &Device) -> std::io::Result<Conn> {
    let pin = Arc::new(Pin::default());
    let mut c = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(pin.clone())
        .with_client_auth_cert(vec![d.cert.clone()], PrivateKeyDer::Pkcs8(d.key.clone_key()))
        .unwrap();
    c.alpn_protocols = vec![alpn.as_bytes().to_vec()];
    let tcp = TcpStream::connect(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await?;
    let s = tokio_rustls::TlsConnector::from(Arc::new(c)).connect(ServerName::try_from("sill").unwrap(), tcp).await?;
    let host_fp = pin.0.lock().unwrap().unwrap();
    Ok(Conn { s, reader: FrameReader::new(Limits::Device), host_fp })
}

impl Conn {
    async fn send(&mut self, kind: Kind, payload: Vec<u8>) {
        self.s.write_all(&Message::new(kind, 1.0, false, payload).serialized()).await.unwrap();
        self.s.flush().await.unwrap();
    }

    /// The next message; `Err` with the read error, `Ok(None)` at the end.
    async fn next(&mut self) -> std::io::Result<Option<Message>> {
        let mut buf = vec![0u8; 65536];
        tokio::time::timeout(WAIT, async {
            loop {
                if let Some(m) = self.reader.next_message().unwrap() {
                    return Ok(Some(m));
                }
                let n = self.s.read(&mut buf).await?;
                if n == 0 {
                    return Ok(None);
                }
                self.reader.push(&buf[..n]).unwrap();
            }
        })
        .await
        .expect("timed out waiting for a message")
    }

    /// The next message of `kind`, skipping others (video, ticks).
    async fn next_of(&mut self, kind: Kind) -> Message {
        loop {
            let m = self.next().await.unwrap().expect("connection ended");
            if m.kind == kind {
                return m;
            }
        }
    }

    /// Reads until the host closes; the kinds seen on the way.
    async fn until_closed(&mut self) -> Vec<Kind> {
        let mut kinds = Vec::new();
        while let Ok(Some(m)) = self.next().await {
            kinds.push(m.kind);
        }
        kinds
    }
}

fn temp_dir() -> std::path::PathBuf {
    let mut b = [0u8; 8];
    sill_store::random(&mut b);
    std::env::temp_dir().join(format!("sill-net-test-{}", base64url::encode(&b)))
}

async fn start_with(f: impl FnOnce(&mut Config)) -> (Arc<Host>, Running) {
    let mut c = Config::new("Test PC");
    c.bind = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    c.remote_port = 0;
    c.bonjour = false;
    c.close_wait = Duration::from_millis(300);
    f(&mut c);
    let host = Host::new(c, Store::open(temp_dir()).unwrap(), Clip::bundled()).unwrap();
    let running = sill_net::start(host.clone()).await.unwrap();
    (host, running)
}

async fn start() -> (Arc<Host>, Running) {
    start_with(|_| {}).await
}

fn alert(e: &std::io::Error) -> Option<AlertDescription> {
    match e.get_ref()?.downcast_ref::<rustls::Error>()? {
        rustls::Error::AlertReceived(a) => Some(*a),
        _ => None,
    }
}

fn request(method: &str, proof: &[u8]) -> Vec<u8> {
    encode(&PairRequest {
        v: 1,
        method: method.into(),
        proof: base64url::encode(proof),
        name: "Test iPhone".into(),
        model: Some("iPhone".into()),
        cable: None,
    })
}

async fn pair_once(port: u16, d: &Device, method: &str, key: &[u8]) -> PairResult {
    let mut c = dial(port, PAIRING_ALPN, d).await.unwrap();
    let p = proof::device_proof(key, &d.fp, &c.host_fp);
    c.send(Kind::PairRequest, request(method, &p)).await;
    let m = c.next_of(Kind::PairResult).await;
    assert!(c.until_closed().await.is_empty(), "the host closes after kind 20");
    decode::<PairResult>(&m.payload).unwrap()
}

/// Pairs `d` through the QR path (no PBKDF2), over the remote door.
async fn pair_qr(host: &Arc<Host>, r: &Running, d: &Device) {
    let offer = host.open_pairing().unwrap();
    let secret = PairLink::parse(&offer.link).unwrap().secret;
    assert!(pair_once(r.remote_port, d, PairRequest::QR, &secret).await.ok);
}

#[tokio::test]
async fn pairing_by_code_end_to_end() {
    let (host, r) = start().await;
    let d = device();
    let offer = host.open_pairing().unwrap();
    let key = proof::code_key(&offer.code, &host.fingerprint).unwrap();
    // Until the host has its own K (derived off the network threads) a proof is answered busy,
    // with nothing counted.
    let mut result = pair_once(r.remote_port, &d, PairRequest::CODE, &[0u8; 32]).await;
    for _ in 0..50 {
        if result.reason.as_deref() != Some(PairResult::BUSY) {
            break;
        }
        assert_eq!(result.retry_after, Some(1.0));
        tokio::time::sleep(Duration::from_millis(100)).await;
        result = pair_once(r.remote_port, &d, PairRequest::CODE, &[0u8; 32]).await;
    }
    assert_eq!(result.reason.as_deref(), Some(PairResult::CODE), "a wrong proof: {result:?}");
    assert_eq!(result.tries_left, Some(4));
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        result = pair_once(r.remote_port, &d, PairRequest::CODE, &key).await;
        if result.reason.as_deref() != Some(PairResult::BUSY) {
            break;
        }
    }
    assert!(result.ok, "{result:?}");
    let fp_host = host.fingerprint;
    let proof_m = base64url::decode(result.proof.as_deref().unwrap()).unwrap();
    assert!(proof::is_valid_mac_proof(&proof_m, &key, &fp_host, &d.fp));
    assert_eq!(result.mac_id.as_deref(), Some(spki::mac_id(&fp_host).as_str()));
    assert_eq!(result.name.as_deref(), Some("Test PC"));
    assert_eq!(base64url::decode(result.recognition_key.as_deref().unwrap()).unwrap(), host.identity.recognition_key);
    // Saved, and a session is admitted at once.
    assert_eq!(host.store.paired().unwrap()[0].fingerprint, base64url::encode(&d.fp));
    assert_eq!(host.store.paired().unwrap()[0].method, "code");
    let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    assert_eq!(s.host_fp, fp_host);
    let list = decode::<WindowList>(&s.next_of(Kind::WindowList).await.payload).unwrap();
    assert_eq!(list.mac_name, "Test PC");
    // The window was used: the remote door takes no more pairing connections (the TLS alert), and
    // the home door's pairing ALPN, which always admits, answers a proof closed.
    let mut late = dial(r.remote_port, PAIRING_ALPN, &device()).await.unwrap();
    assert_eq!(alert(&late.next().await.unwrap_err()), Some(AlertDescription::BadCertificate));
    assert_eq!(
        pair_once(r.home_port, &device(), PairRequest::CODE, &key).await.reason.as_deref(),
        Some(PairResult::CLOSED)
    );
}

#[tokio::test]
async fn an_unpaired_key_gets_the_bad_certificate_alert() {
    let (host, r) = start().await;
    for (port, alpn) in [(r.remote_port, SESSION_ALPN), (r.remote_port, PAIRING_ALPN), (r.home_port, SESSION_ALPN)] {
        // TLS 1.3: the client's handshake completes; its first read carries the host's alert.
        let mut c = dial(port, alpn, &device()).await.expect("the client's side of the handshake completes");
        let e = c.next().await.expect_err("the host refuses the key");
        assert_eq!(alert(&e), Some(AlertDescription::BadCertificate), "{alpn} on {port}: {e}");
    }
    // The home door's pairing ALPN admits any key.
    let mut c = dial(r.home_port, PAIRING_ALPN, &device()).await.unwrap();
    c.send(Kind::PairRequest, request(PairRequest::ASK, &[0; 32])).await;
    let res = decode::<PairResult>(&c.next_of(Kind::PairResult).await.payload).unwrap();
    assert_eq!(res.reason.as_deref(), Some(PairResult::OPEN_ON_MAC));
    assert!(host.stats.refused.load(std::sync::atomic::Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn session_catalog_order_then_a_keyframe() {
    let (host, r) = start().await;
    let d = device();
    pair_qr(&host, &r, &d).await;
    let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    s.send(Kind::Hello, br#"{"appVersion":"1.0","protocol":1,"device":"test"}"#.to_vec()).await;
    let first = s.next().await.unwrap().unwrap();
    assert_eq!(first.kind, Kind::WindowList);
    let list = decode::<WindowList>(&first.payload).unwrap();
    assert!(list.windows.is_empty());
    assert_eq!(list.active, sill_wire::StreamSource::Desktop);
    assert_eq!(list.protocol, Some(1));
    assert_eq!(list.launch_id.as_deref(), Some(host.launch_id.as_str()));
    assert!(list.host_version.is_some());
    let second = s.next().await.unwrap().unwrap();
    assert_eq!(second.kind, Kind::HostSettings);
    let hs = decode::<sill_wire::HostSettingsState>(&second.payload).unwrap();
    assert!(!hs.persistent && !hs.virtual_display_available && !hs.software_encoder);
    // The picture on join: parameter sets, then a keyframe, before any delta.
    let mut kinds = Vec::new();
    loop {
        let m = s.next().await.unwrap().unwrap();
        if m.kind == Kind::Tick {
            continue;
        }
        kinds.push((m.kind, m.is_keyframe));
        if m.kind == Kind::Frame {
            break;
        }
    }
    assert_eq!(kinds, vec![(Kind::ParameterSets, false), (Kind::Frame, true)]);
}

#[tokio::test]
async fn ping_pong_junk_and_a_keyframe_request() {
    let (host, r) = start().await;
    let d = device();
    pair_qr(&host, &r, &d).await;
    let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    s.next_of(Kind::HostSettings).await;
    // Unknown and reserved kinds are skipped; a ping right after is still answered.
    s.send(Kind::Unknown(200), vec![1, 2, 3]).await;
    s.send(Kind::Unknown(30), Vec::new()).await;
    s.send(Kind::ChangeSettings, b"{}".to_vec()).await;
    let ping = Message::new(Kind::Ping, 12345.678, false, 0.25f64.to_be_bytes().to_vec());
    s.s.write_all(&ping.serialized()).await.unwrap();
    let pong = s.next_of(Kind::Pong).await;
    assert_eq!(pong.payload, ping.payload);
    assert_eq!(pong.timestamp.to_bits(), ping.timestamp.to_bits());
    // Ticks while streaming.
    s.next_of(Kind::Tick).await;
    // A keyframe request restarts the clip at its IDR, parameter sets first.
    s.next_of(Kind::Frame).await;
    s.send(Kind::KeyframeRequest, Vec::new()).await;
    let mut saw = None;
    for _ in 0..40 {
        let m = s.next().await.unwrap().unwrap();
        if m.kind == Kind::ParameterSets {
            let next = s.next_of(Kind::Frame).await;
            saw = Some(next.is_keyframe);
            break;
        }
    }
    assert_eq!(saw, Some(true));
}

#[tokio::test]
async fn an_oversized_header_closes_the_session() {
    let (host, r) = start().await;
    let d = device();
    pair_qr(&host, &r, &d).await;
    let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    s.next_of(Kind::HostSettings).await;
    let mut h = Message::new(Kind::Input, 1.0, false, Vec::new()).serialized();
    h[10..14].copy_from_slice(&((sill_wire::MAX_CLIENT_PAYLOAD + 1) as u32).to_be_bytes());
    s.s.write_all(&h).await.unwrap();
    s.until_closed().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(host.sessions(), (0, 0));
}

#[tokio::test]
async fn the_ninth_remote_session_gets_busy() {
    let (host, r) = start().await;
    let d = device();
    pair_qr(&host, &r, &d).await;
    let mut held = Vec::new();
    for _ in 0..8 {
        let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
        s.next_of(Kind::WindowList).await;
        held.push(s);
    }
    assert_eq!(host.sessions(), (0, 8));
    let mut ninth = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    let m = ninth.next().await.unwrap().unwrap();
    assert_eq!(m.kind, Kind::Goodbye);
    assert_eq!(decode::<Goodbye>(&m.payload).unwrap().reason, Goodbye::BUSY);
    assert!(ninth.until_closed().await.is_empty());
    drop(held);
}

#[tokio::test]
async fn quit_says_goodbye_and_unpair_says_removed() {
    let (host, r) = start().await;
    let (a, b) = (device(), device());
    pair_qr(&host, &r, &a).await;
    pair_qr(&host, &r, &b).await;
    let mut sa = dial(r.remote_port, SESSION_ALPN, &a).await.unwrap();
    let mut sb = dial(r.home_port, SESSION_ALPN, &b).await.unwrap();
    sa.next_of(Kind::WindowList).await;
    sb.next_of(Kind::WindowList).await;
    // Unpair a (by fingerprint prefix): its session gets goodbye removed, its next dial the alert.
    let fp_a = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(a.fp);
    assert_eq!(host.unpair(&fp_a[..10]).unwrap().len(), 1);
    let g = decode::<Goodbye>(&sa.next_of(Kind::Goodbye).await.payload).unwrap();
    assert_eq!(g.reason, Goodbye::REMOVED);
    let mut again = dial(r.remote_port, SESSION_ALPN, &a).await.unwrap();
    assert_eq!(alert(&again.next().await.unwrap_err()), Some(AlertDescription::BadCertificate));
    // Quit: goodbye quit, reconnect true, then the host closes.
    r.quit().await;
    let g = decode::<Goodbye>(&sb.next_of(Kind::Goodbye).await.payload).unwrap();
    assert_eq!((g.reason.as_str(), g.reconnect), (Goodbye::QUIT, Some(true)));
    sb.until_closed().await;
}

#[tokio::test]
async fn a_silent_remote_client_is_dropped() {
    let (host, r) = start_with(|c| c.remote_silence = Duration::from_secs(1)).await;
    let d = device();
    pair_qr(&host, &r, &d).await;
    let mut s = dial(r.remote_port, SESSION_ALPN, &d).await.unwrap();
    let started = std::time::Instant::now();
    s.until_closed().await;
    assert!(started.elapsed() >= Duration::from_millis(900));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(host.sessions(), (0, 0));
}
