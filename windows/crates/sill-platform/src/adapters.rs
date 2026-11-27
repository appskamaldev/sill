//! Virtual network adapters, whose addresses no device can reach: Hyper-V's vEthernet switches
//! (WSL's among them), VirtualBox and VMware host-only networks, Docker. Their private addresses
//! (often 172.16/12, e.g. WSL's 172.28.16.1) would otherwise be listened on and put in the pairing
//! link's `a=` list (Checkpoint 1, 2026-11-27). The decision is by the adapter, never by the range:
//! a real LAN may use 172.16/12 too.
//!
//! Windows reads each adapter's description and friendly name (`GetAdaptersAddresses`) and keys
//! them by the adapter's GUID (if-addrs' `adapter_name`); elsewhere the interface name is all
//! there is.

/// Whether an adapter with this friendly name (or, outside Windows, interface name) and
/// description is a virtual machine's, container's or WSL's host-side network.
pub fn is_virtual(name: &str, description: &str) -> bool {
    let (n, d) = (name.to_ascii_lowercase(), description.to_ascii_lowercase());
    const DESCRIPTIONS: [&str; 6] = [
        "hyper-v virtual ethernet",
        "virtualbox",
        "vmware virtual ethernet",
        "vmware network adapter",
        "docker",
        "wsl",
    ];
    const NAMES: [&str; 4] = ["vethernet", "vmware network adapter", "virtualbox", "docker"];
    // Interface names on macOS and Linux.
    const UNIX_PREFIXES: [&str; 6] = ["vmnet", "vboxnet", "docker", "veth", "virbr", "br-"];
    DESCRIPTIONS.iter().any(|p| d.contains(p))
        || NAMES.iter().any(|p| n.starts_with(p))
        || n.contains("wsl")
        || UNIX_PREFIXES.iter().any(|p| n.starts_with(p))
}

/// The GUIDs (`adapter_name`) of this machine's virtual adapters. Empty outside Windows and when
/// the list cannot be read.
#[cfg(windows)]
pub fn virtual_adapter_guids() -> std::collections::HashSet<String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        GAA_FLAG_SKIP_UNICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    let flags = GAA_FLAG_SKIP_UNICAST | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut out = std::collections::HashSet::new();
    let mut size: u32 = 16 * 1024;
    for _ in 0..3 {
        // u64 words keep the buffer aligned for the structure.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let first = buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: `buf` holds `size` bytes, aligned; Windows writes a linked list inside it.
        let rc = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) };
        if rc == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if rc != 0 {
            return out;
        }
        let mut a = first as *const IP_ADAPTER_ADDRESSES_LH;
        while !a.is_null() {
            // SAFETY: every node and string lives in `buf`, which outlives this loop.
            unsafe {
                let name = (*a).FriendlyName.to_string().unwrap_or_default();
                let description = (*a).Description.to_string().unwrap_or_default();
                if is_virtual(&name, &description) {
                    if let Ok(guid) = (*a).AdapterName.to_string() {
                        out.insert(guid);
                    }
                }
                a = (*a).Next;
            }
        }
        return out;
    }
    out
}

#[cfg(not(windows))]
pub fn virtual_adapter_guids() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

#[cfg(test)]
mod tests {
    use super::is_virtual;

    #[test]
    fn virtual_adapters() {
        assert!(is_virtual("vEthernet (WSL (Hyper-V firewall))", "Hyper-V Virtual Ethernet Adapter"));
        assert!(is_virtual("vEthernet (Default Switch)", "Hyper-V Virtual Ethernet Adapter #2"));
        assert!(is_virtual("Ethernet 3", "VirtualBox Host-Only Ethernet Adapter"));
        assert!(is_virtual("VMware Network Adapter VMnet1", "VMware Virtual Ethernet Adapter for VMnet1"));
        assert!(is_virtual("vEthernet (DockerNAT)", "Hyper-V Virtual Ethernet Adapter #3"));
        assert!(is_virtual("vmnet8", ""));
        assert!(is_virtual("docker0", ""));
        assert!(is_virtual("vboxnet0", ""));
    }

    #[test]
    fn real_adapters() {
        assert!(!is_virtual("Wi-Fi", "Intel(R) Wi-Fi 7 BE200 320MHz"));
        assert!(!is_virtual("Ethernet", "Realtek PCIe GbE Family Controller"));
        assert!(!is_virtual("Tailscale", "Tailscale Tunnel"));
        assert!(!is_virtual("en0", ""));
        assert!(!is_virtual("utun4", ""));
        assert!(!is_virtual("bridge0", ""));
    }
}
