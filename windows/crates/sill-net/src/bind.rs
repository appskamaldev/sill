//! Which local addresses the doors listen on. Default: loopback, private LAN and Tailscale, never
//! a public address unless internet access is on (WINDOWS_PORT.md §8, SECURITY.md), and never an
//! address on a virtual adapter (Hyper-V/WSL vEthernet, VirtualBox, VMware, Docker:
//! `sill_platform::adapters`), which no device can reach. `--bind` still takes one explicitly.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddrClass {
    Loopback,
    /// RFC 1918, or an IPv6 unique local address outside Tailscale's.
    Lan,
    /// 100.64.0.0/10 (Tailscale's CGNAT range) or fd7a:115c:a1e0::/48.
    Tailscale,
    /// IPv4 169.254/16, IPv6 fe80::/10: never bound (an IPv6 one needs a scope).
    LinkLocal,
    /// Anything else: reachable from the internet.
    Public,
    /// 0.0.0.0, ::, multicast, broadcast.
    Unusable,
}

pub fn classify(ip: IpAddr) -> AddrClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return classify_v4(v4);
            }
            classify_v6(v6)
        }
    }
}

fn classify_v4(ip: Ipv4Addr) -> AddrClass {
    let o = ip.octets();
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        AddrClass::Unusable
    } else if ip.is_loopback() {
        AddrClass::Loopback
    } else if o[0] == 100 && (o[1] & 0xC0) == 64 {
        AddrClass::Tailscale
    } else if ip.is_private() {
        AddrClass::Lan
    } else if ip.is_link_local() {
        AddrClass::LinkLocal
    } else {
        AddrClass::Public
    }
}

fn classify_v6(ip: Ipv6Addr) -> AddrClass {
    let s = ip.segments();
    if ip.is_unspecified() || ip.is_multicast() {
        AddrClass::Unusable
    } else if ip.is_loopback() {
        AddrClass::Loopback
    } else if s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0 {
        AddrClass::Tailscale
    } else if (s[0] & 0xFE00) == 0xFC00 {
        AddrClass::Lan
    } else if (s[0] & 0xFFC0) == 0xFE80 {
        AddrClass::LinkLocal
    } else {
        AddrClass::Public
    }
}

/// Whether the doors may listen on `ip`.
pub fn allowed(ip: IpAddr, allow_internet: bool) -> bool {
    match classify(ip) {
        AddrClass::Loopback | AddrClass::Lan | AddrClass::Tailscale => true,
        AddrClass::Public => allow_internet,
        AddrClass::LinkLocal | AddrClass::Unusable => false,
    }
}

/// This machine's addresses the doors may listen on now, loopback first.
pub fn default_addresses(allow_internet: bool) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        let virtual_guids = sill_platform::adapters::virtual_adapter_guids();
        for i in ifs {
            let ip = i.ip();
            if allowed(ip, allow_internet) && !out.contains(&ip) && !on_virtual_adapter(&i, &virtual_guids) {
                out.push(ip);
            }
        }
    }
    out
}

#[cfg(windows)]
fn on_virtual_adapter(i: &if_addrs::Interface, virtual_guids: &std::collections::HashSet<String>) -> bool {
    virtual_guids.contains(&i.adapter_name)
}

#[cfg(not(windows))]
fn on_virtual_adapter(i: &if_addrs::Interface, _: &std::collections::HashSet<String>) -> bool {
    sill_platform::adapters::is_virtual(&i.name, "")
}

/// Up to `n` addresses a device could type, Tailscale first, then LAN (for the pairing link).
pub fn reachable_addresses(bound: &[IpAddr], n: usize) -> Vec<IpAddr> {
    let mut v: Vec<IpAddr> = bound.iter().copied().filter(|ip| classify(*ip) == AddrClass::Tailscale).collect();
    v.extend(bound.iter().copied().filter(|ip| classify(*ip) == AddrClass::Lan));
    v.extend(bound.iter().copied().filter(|ip| classify(*ip) == AddrClass::Public));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> AddrClass {
        classify(s.parse().unwrap())
    }

    #[test]
    fn classes() {
        assert_eq!(c("127.0.0.1"), AddrClass::Loopback);
        assert_eq!(c("::1"), AddrClass::Loopback);
        assert_eq!(c("100.64.0.1"), AddrClass::Tailscale);
        assert_eq!(c("100.127.255.254"), AddrClass::Tailscale);
        assert_eq!(c("100.128.0.1"), AddrClass::Public);
        assert_eq!(c("100.63.255.255"), AddrClass::Public);
        assert_eq!(c("fd7a:115c:a1e0::1234"), AddrClass::Tailscale);
        assert_eq!(c("fd7a:115c:a1e1::1"), AddrClass::Lan);
        assert_eq!(c("10.0.0.5"), AddrClass::Lan);
        assert_eq!(c("172.16.0.1"), AddrClass::Lan);
        assert_eq!(c("172.32.0.1"), AddrClass::Public);
        assert_eq!(c("192.168.1.20"), AddrClass::Lan);
        assert_eq!(c("169.254.1.1"), AddrClass::LinkLocal);
        assert_eq!(c("fe80::1"), AddrClass::LinkLocal);
        assert_eq!(c("8.8.8.8"), AddrClass::Public);
        assert_eq!(c("2001:db8::1"), AddrClass::Public);
        assert_eq!(c("::ffff:192.168.0.1"), AddrClass::Lan);
        assert_eq!(c("0.0.0.0"), AddrClass::Unusable);
        assert_eq!(c("::"), AddrClass::Unusable);
    }

    #[test]
    fn public_only_with_internet() {
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!allowed(ip, false));
        assert!(allowed(ip, true));
        assert!(!allowed("0.0.0.0".parse().unwrap(), true));
        assert!(default_addresses(false).iter().all(|ip| classify(*ip) != AddrClass::Public));
    }
}
