use std::net::{IpAddr, Ipv4Addr};

/// Адреса интерфейсов (включая VPN), без loopback и link-local.
/// Частные адреса предлагаются первыми.
pub fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified()
                && !ip.is_link_local() && !ip.is_broadcast() && !ip.is_multicast() => Some(ip),
            _ => None,
        })
        .collect();
    // Типичный Wi-Fi/Ethernet 192.168.x.x обычно полезнее VPN/Docker 10.x/172.x.
    ips.sort_by_key(|ip| {
        let o = ip.octets();
        let rank = if o[0] == 192 && o[1] == 168 { 0 }
            else if o[0] == 10 { 1 }
            else if ip.is_private() { 2 }
            else { 3 };
        (rank, o)
    });
    ips.dedup();
    ips
}
