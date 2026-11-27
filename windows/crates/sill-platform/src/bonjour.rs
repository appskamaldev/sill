//! Advertising the home door as `_sill._tcp` (upstream `StreamServer.swift:456-466`): Windows'
//! own mDNS responder through `DnsServiceRegister` (Windows 10 1809+). On other platforms nothing
//! is advertised; the caller logs that.

/// The service type every Sill host advertises.
pub const SERVICE_TYPE: &str = "_sill._tcp";

/// The outcome of the asynchronous registration, once Windows reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Registered,
    Failed(u32),
}

#[cfg(windows)]
mod imp {
    use super::Status;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::NetworkManagement::Dns::{
        DnsServiceConstructInstance, DnsServiceDeRegister, DnsServiceFreeInstance, DnsServiceRegister,
        DNS_QUERY_REQUEST_VERSION1, DNS_SERVICE_INSTANCE, DNS_SERVICE_REGISTER_REQUEST,
    };

    const DNS_REQUEST_PENDING: u32 = 9506;
    const PENDING: u32 = u32::MAX;

    pub struct Registration {
        request: Box<DNS_SERVICE_REGISTER_REQUEST>,
        /// Written by the completion callback; leaked on drop, as a late callback may still write it.
        status: &'static AtomicU32,
    }

    // SAFETY: the request is only touched by this owner and the system's callback, which writes
    // only `status` (an atomic).
    unsafe impl Send for Registration {}

    unsafe extern "system" fn completed(
        status: u32,
        context: *const core::ffi::c_void,
        instance: *const DNS_SERVICE_INSTANCE,
    ) {
        if !context.is_null() {
            // SAFETY: the context is the leaked `&'static AtomicU32` passed in `register`.
            unsafe { &*(context as *const AtomicU32) }.store(status, Ordering::SeqCst);
        }
        if !instance.is_null() {
            // SAFETY: the callback owns the instance it is handed (DnsServiceRegister's documentation).
            unsafe { DnsServiceFreeInstance(instance) };
        }
    }

    pub fn register(instance_name: &str, port: u16, txt: &[(&str, &str)]) -> std::io::Result<Registration> {
        let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".into());
        let service = HSTRING::from(format!("{instance_name}.{}.local", super::SERVICE_TYPE));
        let host = HSTRING::from(format!("{host}.local"));
        let keys: Vec<HSTRING> = txt.iter().map(|(k, _)| HSTRING::from(*k)).collect();
        let values: Vec<HSTRING> = txt.iter().map(|(_, v)| HSTRING::from(*v)).collect();
        let key_ptrs: Vec<PCWSTR> = keys.iter().map(|k| PCWSTR(k.as_ptr())).collect();
        let value_ptrs: Vec<PCWSTR> = values.iter().map(|v| PCWSTR(v.as_ptr())).collect();
        // SAFETY: every string outlives the call, which copies them into the new instance.
        let instance = unsafe {
            DnsServiceConstructInstance(
                &service,
                &host,
                None,
                None,
                port,
                0,
                0,
                key_ptrs.len() as u32,
                key_ptrs.as_ptr(),
                value_ptrs.as_ptr(),
            )
        };
        if instance.is_null() {
            return Err(std::io::Error::other("DnsServiceConstructInstance failed"));
        }
        let status: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(PENDING)));
        let request = Box::new(DNS_SERVICE_REGISTER_REQUEST {
            Version: DNS_QUERY_REQUEST_VERSION1.0,
            InterfaceIndex: 0,
            pServiceInstance: instance,
            pRegisterCompletionCallback: Some(completed),
            pQueryContext: status as *const AtomicU32 as *mut core::ffi::c_void,
            ..Default::default()
        });
        // SAFETY: `request` is boxed and kept alive by the returned Registration.
        let r = unsafe { DnsServiceRegister(&*request, None) };
        if r != DNS_REQUEST_PENDING {
            // SAFETY: not registered, so the instance is still ours.
            unsafe { DnsServiceFreeInstance(instance) };
            return Err(std::io::Error::other(format!("DnsServiceRegister: error {r}")));
        }
        Ok(Registration { request, status })
    }

    impl Registration {
        pub fn status(&self) -> Status {
            match self.status.load(Ordering::SeqCst) {
                PENDING => Status::Pending,
                0 => Status::Registered,
                e => Status::Failed(e),
            }
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: the same request that registered it; the instance it names is still ours.
            unsafe {
                DnsServiceDeRegister(&*self.request, None);
                DnsServiceFreeInstance(self.request.pServiceInstance);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Status;

    /// Never made: `register` returns an error on this platform.
    pub struct Registration;

    pub fn register(_instance_name: &str, _port: u16, _txt: &[(&str, &str)]) -> std::io::Result<Registration> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Bonjour registration is Windows-only in this build"))
    }

    impl Registration {
        pub fn status(&self) -> Status {
            Status::Failed(0)
        }
    }
}

pub use imp::Registration;

/// Advertises `instance_name` (the host's name) on `port` with these TXT pairs, until the returned
/// value is dropped. Unsupported off Windows.
pub fn register(instance_name: &str, port: u16, txt: &[(&str, &str)]) -> std::io::Result<Registration> {
    imp::register(instance_name, port, txt)
}

/// Whether this build can advertise at all.
pub const SUPPORTED: bool = cfg!(windows);
