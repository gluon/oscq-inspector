//! Address preference: IPv4 first, loopback and the Docker default bridge last, nothing discarded.

use std::net::{IpAddr, Ipv4Addr};

use oscq_core::{AddressClass, InterfaceRef, ServerAddress};

/// Docker's default bridge network, `docker0`, 172.17.0.0/16. The rest of 172.16.0.0/12 is
/// ordinary private address space, common on venue and lab LANs, and is not demoted.
fn is_docker_bridge(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 172 && b == 17
}

pub fn classify(ip: IpAddr) -> AddressClass {
    match ip {
        IpAddr::V4(v4) if v4.is_loopback() || is_docker_bridge(v4) => {
            AddressClass::LoopbackOrBridge
        }
        IpAddr::V4(_) => AddressClass::Ipv4,
        IpAddr::V6(v6) if v6.is_loopback() => AddressClass::LoopbackOrBridge,
        IpAddr::V6(v6) if v6.is_unicast_link_local() => AddressClass::Ipv6LinkLocal,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => classify(IpAddr::V4(v4)),
            None => AddressClass::Ipv6,
        },
    }
}

/// Merges newly seen addresses into a list, then orders it best first: by class, then by address,
/// so that the order does not depend on arrival order. Interfaces of a known address are merged.
pub fn merge(
    into: &mut Vec<ServerAddress>,
    seen: impl IntoIterator<Item = (IpAddr, Vec<InterfaceRef>)>,
) {
    for (ip, interfaces) in seen {
        match into.iter_mut().find(|a| a.ip == ip) {
            Some(known) => {
                for i in interfaces {
                    if !known.interfaces.contains(&i) {
                        known.interfaces.push(i);
                    }
                }
            }
            None => into.push(ServerAddress {
                ip,
                class: classify(ip),
                interfaces,
            }),
        }
    }
    for a in into.iter_mut() {
        a.interfaces
            .sort_by(|x, y| x.index.cmp(&y.index).then_with(|| x.name.cmp(&y.name)));
    }
    into.sort_by(|a, b| a.class.cmp(&b.class).then(a.ip.cmp(&b.ip)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn iface(name: &str, index: u32) -> InterfaceRef {
        InterfaceRef {
            name: name.into(),
            index,
        }
    }

    #[test]
    fn classes() {
        assert_eq!(classify(ip("192.168.1.20")), AddressClass::Ipv4);
        assert_eq!(classify(ip("169.254.3.4")), AddressClass::Ipv4);
        assert_eq!(classify(ip("172.16.0.5")), AddressClass::Ipv4);
        assert_eq!(classify(ip("172.18.0.5")), AddressClass::Ipv4);
        assert_eq!(classify(ip("172.17.0.1")), AddressClass::LoopbackOrBridge);
        assert_eq!(classify(ip("127.0.0.1")), AddressClass::LoopbackOrBridge);
        assert_eq!(classify(ip("::1")), AddressClass::LoopbackOrBridge);
        assert_eq!(classify(ip("fe80::1")), AddressClass::Ipv6LinkLocal);
        assert_eq!(classify(ip("fd00::1")), AddressClass::Ipv6);
        assert_eq!(
            classify(ip("::ffff:127.0.0.1")),
            AddressClass::LoopbackOrBridge
        );
        assert_eq!(classify(ip("::ffff:10.0.0.1")), AddressClass::Ipv4);
    }

    #[test]
    fn ipv4_first_fallbacks_kept() {
        let mut list = Vec::new();
        merge(
            &mut list,
            [
                (ip("127.0.0.1"), vec![iface("lo0", 1)]),
                (ip("fe80::1"), vec![iface("en0", 14)]),
                (ip("172.17.0.1"), vec![iface("docker0", 5)]),
                (ip("fd00::2"), vec![iface("en0", 14)]),
                (ip("192.168.1.20"), vec![iface("en0", 14)]),
            ],
        );
        let order: Vec<String> = list.iter().map(|a| a.ip.to_string()).collect();
        assert_eq!(
            order,
            [
                "192.168.1.20",
                "fd00::2",
                "fe80::1",
                "127.0.0.1",
                "172.17.0.1"
            ]
        );
    }

    #[test]
    fn same_address_on_two_interfaces_merges() {
        let mut list = Vec::new();
        merge(&mut list, [(ip("192.168.1.20"), vec![iface("en1", 14)])]);
        merge(&mut list, [(ip("192.168.1.20"), vec![iface("lo0", 1)])]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].interfaces, vec![iface("lo0", 1), iface("en1", 14)]);
    }
}
