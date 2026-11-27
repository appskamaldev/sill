//! Secrets at rest. Windows: DPAPI (`CryptProtectData`, current user, no UI). Elsewhere: none at
//! all (DEV ONLY), which `PROTECTED` says so callers can label the files.

/// Whether `protect` really encrypts (DPAPI); false on the macOS development build.
pub const PROTECTED: bool = cfg!(windows);

#[cfg(windows)]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// Copies DPAPI's output and frees it.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: freed once, as CryptProtectData's documentation says.
        unsafe {
            LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        }
        v
    }

    pub fn protect(plain: &[u8]) -> std::io::Result<Vec<u8>> {
        let input = blob(plain);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `plain`, alive for the call; `out` is written by the call.
        unsafe { CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
            .map_err(|e| std::io::Error::other(format!("CryptProtectData: {e}")))?;
        Ok(take(out))
    }

    pub fn unprotect(sealed: &[u8]) -> std::io::Result<Vec<u8>> {
        let input = blob(sealed);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`.
        unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
            .map_err(|e| std::io::Error::other(format!("CryptUnprotectData: {e}")))?;
        Ok(take(out))
    }
}

#[cfg(not(windows))]
mod imp {
    /// DEV ONLY: stored as is.
    pub fn protect(plain: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(plain.to_vec())
    }

    /// DEV ONLY: read as is.
    pub fn unprotect(sealed: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(sealed.to_vec())
    }
}

/// Seals `plain` for the current Windows user (DPAPI); a copy elsewhere.
pub fn protect(plain: &[u8]) -> std::io::Result<Vec<u8>> {
    imp::protect(plain)
}

/// Opens what `protect` sealed; fails for another user's or a damaged blob.
pub fn unprotect(sealed: &[u8]) -> std::io::Result<Vec<u8>> {
    imp::unprotect(sealed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let secret = b"0123456789abcdef0123456789abcdef";
        let sealed = super::protect(secret).unwrap();
        if super::PROTECTED {
            assert_ne!(&sealed[..], &secret[..]);
        }
        assert_eq!(super::unprotect(&sealed).unwrap(), secret);
    }
}
