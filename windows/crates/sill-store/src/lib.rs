//! The host's stored state: its P-256 identity key, its 32-byte recognition key, the paired
//! devices and Require pairing (upstream `HostIdentity.swift`, `KeychainIdentityStore.swift`,
//! `RemoteAccess.PairedDevice`).
//!
//! Windows: `%LOCALAPPDATA%\Sill\` (or `--data-dir`), every secret sealed with DPAPI for the
//! current user. Other platforms (development only): the same files in plain text under the
//! directory given, with a `DEV-ONLY-UNENCRYPTED.txt` beside them.

use p256::SecretKey;
use serde::{Deserialize, Serialize};
use sill_pairing::{base64url, spki};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const IDENTITY: &str = "identity.bin";
const RECOGNITION: &str = "recognition.bin";
const DEVICES: &str = "devices.json.dpapi";
const SETTINGS: &str = "settings.json";
const DEV_MARK: &str = "DEV-ONLY-UNENCRYPTED.txt";

/// Fills `buf` from the OS CSPRNG (BCryptGenRandom on Windows, getentropy on macOS).
pub fn random(buf: &mut [u8]) -> bool {
    getrandom::getrandom(buf).is_ok()
}

/// The host's identity: what its certificate and fingerprint come from, and the key behind its
/// Bonjour tag and pairing results.
#[derive(Clone)]
pub struct Identity {
    pub key: SecretKey,
    pub recognition_key: [u8; 32],
}

impl Identity {
    /// X9.63 uncompressed public point (65 bytes).
    pub fn public_point(&self) -> Vec<u8> {
        self.key.public_key().to_sec1_bytes().to_vec()
    }

    /// SHA-256 of the SPKI: the pin devices keep.
    pub fn fingerprint(&self) -> [u8; 32] {
        spki::spki_fingerprint(&self.public_point())
    }

    pub fn mac_id(&self) -> String {
        spki::mac_id(&self.fingerprint())
    }
}

/// One paired device, in upstream's `PairedDevice` shape (`cableDevice` is not kept: no cable).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    /// base64url of the device key's SPKI fingerprint.
    pub fingerprint: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Seconds since 1970.
    pub paired_at: f64,
    /// "code" or "qr".
    pub method: String,
}

impl PairedDevice {
    pub fn fingerprint_bytes(&self) -> Option<Vec<u8>> {
        base64url::decode(&self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub require_pairing: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { require_pairing: true }
    }
}

/// The directory and its files.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `%LOCALAPPDATA%\Sill` on Windows; `None` elsewhere (a development build must be given a
    /// directory).
    pub fn default_dir() -> Option<PathBuf> {
        if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Sill"))
        } else {
            None
        }
    }

    /// Opens (creating) `dir`. Off Windows it marks the directory DEV ONLY.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Store> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        if !sill_platform::protect::PROTECTED {
            let mark = dir.join(DEV_MARK);
            if !mark.exists() {
                fs::write(
                    mark,
                    "DEV ONLY. The Sill host's private key, recognition key and paired devices are stored here \
                     UNENCRYPTED (this is not a Windows build, so there is no DPAPI). Never use this directory \
                     for a real pairing you want to keep private.\n",
                )?;
            }
        }
        Ok(Store { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read_sealed(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.dir.join(name)) {
            Ok(d) => sill_platform::protect::unprotect(&d).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn write_sealed(&self, name: &str, plain: &[u8]) -> io::Result<()> {
        self.write_atomic(name, &sill_platform::protect::protect(plain)?)
    }

    /// Written beside, then renamed over: a crash never leaves half a file.
    fn write_atomic(&self, name: &str, data: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, self.dir.join(name))
    }

    /// The stored identity, made (from the CSPRNG) and saved on first use. A stored file that does
    /// not open is an error, never silently replaced: replacing it would unpair every device.
    pub fn identity(&self) -> io::Result<Identity> {
        let key = match self.read_sealed(IDENTITY)? {
            Some(d) => SecretKey::from_slice(&d)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "identity.bin is not a P-256 key"))?,
            None => {
                let mut k = None;
                for _ in 0..8 {
                    let mut b = [0u8; 32];
                    if !random(&mut b) {
                        return Err(io::Error::other("the OS random generator failed"));
                    }
                    if let Ok(s) = SecretKey::from_slice(&b) {
                        k = Some(s);
                        break;
                    }
                }
                let k = k.ok_or_else(|| io::Error::other("no valid P-256 key from the random generator"))?;
                self.write_sealed(IDENTITY, &k.to_bytes())?;
                k
            }
        };
        let recognition_key = match self.read_sealed(RECOGNITION)? {
            Some(d) => <[u8; 32]>::try_from(&d[..])
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "recognition.bin is not 32 bytes"))?,
            None => {
                let mut r = [0u8; 32];
                if !random(&mut r) {
                    return Err(io::Error::other("the OS random generator failed"));
                }
                self.write_sealed(RECOGNITION, &r)?;
                r
            }
        };
        Ok(Identity { key, recognition_key })
    }

    pub fn paired(&self) -> io::Result<Vec<PairedDevice>> {
        match self.read_sealed(DEVICES)? {
            Some(d) => serde_json::from_slice(&d).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            None => Ok(Vec::new()),
        }
    }

    /// When the paired devices were last written (by this process or another `sill-host`).
    pub fn paired_modified(&self) -> Option<std::time::SystemTime> {
        fs::metadata(self.dir.join(DEVICES)).and_then(|m| m.modified()).ok()
    }

    pub fn save_paired(&self, devices: &[PairedDevice]) -> io::Result<()> {
        let d = serde_json::to_vec_pretty(devices).map_err(io::Error::other)?;
        self.write_sealed(DEVICES, &d)
    }

    pub fn settings(&self) -> io::Result<Settings> {
        match fs::read(self.dir.join(SETTINGS)) {
            Ok(d) => serde_json::from_slice(&d).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> io::Result<()> {
        let d = serde_json::to_vec_pretty(s).map_err(io::Error::other)?;
        self.write_atomic(SETTINGS, &d)
    }
}

/// Removes the devices whose name or fingerprint (base64url, or a prefix of 6+ characters of it)
/// is `which`; returns those removed.
pub fn remove_matching(devices: &mut Vec<PairedDevice>, which: &str) -> Vec<PairedDevice> {
    let hit = |d: &PairedDevice| {
        d.name == which || d.fingerprint == which || (which.len() >= 6 && d.fingerprint.starts_with(which))
    };
    let removed: Vec<PairedDevice> = devices.iter().filter(|d| hit(d)).cloned().collect();
    devices.retain(|d| !hit(d));
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut b = [0u8; 8];
        assert!(random(&mut b));
        std::env::temp_dir().join(format!("sill-store-{tag}-{}", base64url::encode(&b)))
    }

    #[test]
    fn identity_is_made_once_and_kept() {
        let dir = temp_dir("id");
        let s = Store::open(&dir).unwrap();
        let a = s.identity().unwrap();
        let b = Store::open(&dir).unwrap().identity().unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.recognition_key, b.recognition_key);
        assert_eq!(a.public_point().len(), 65);
        assert!(spki::mac_id_is_well_formed(&a.mac_id()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn devices_and_settings_round_trip() {
        let dir = temp_dir("dev");
        let s = Store::open(&dir).unwrap();
        assert!(s.paired().unwrap().is_empty());
        assert!(s.settings().unwrap().require_pairing);
        let d = PairedDevice {
            fingerprint: base64url::encode(&[7u8; 32]),
            name: "Kamal's iPhone".into(),
            model: Some("iPhone".into()),
            paired_at: 1.8e9,
            method: "code".into(),
        };
        s.save_paired(std::slice::from_ref(&d)).unwrap();
        s.save_settings(&Settings { require_pairing: false }).unwrap();
        assert_eq!(s.paired().unwrap(), vec![d.clone()]);
        assert!(!s.settings().unwrap().require_pairing);
        let mut list = vec![d.clone()];
        assert_eq!(remove_matching(&mut list, &d.fingerprint[..8]), vec![d]);
        assert!(list.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
