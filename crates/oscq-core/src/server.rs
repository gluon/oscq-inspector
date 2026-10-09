//! Types shared between discovery and the rest of the application.

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use url::Url;

/// Opaque identifier of a known server, stable for the lifetime of the process. Never shown to
/// the user and never derived from the mDNS instance name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerId(u64);

impl ServerId {
    /// A new identifier, distinct from every other one created by this process.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        ServerId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// What a discovered service offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceKind {
    /// `_oscjson._tcp`: an OSCQuery server, browsable and controllable.
    OscQuery,
    /// `_osc._udp`: an OSC receiver with no query support. Shown, never connectable.
    OscOnly,
}

impl ServiceKind {
    pub fn connectable(self) -> bool {
        matches!(self, ServiceKind::OscQuery)
    }
}

/// Preference class of an address, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AddressClass {
    /// IPv4 on a real interface.
    Ipv4,
    /// IPv6 outside the link local range.
    Ipv6,
    /// IPv6 link local, usable only with its interface.
    Ipv6LinkLocal,
    /// Loopback or the Docker default bridge: kept as a fallback, never preferred.
    LoopbackOrBridge,
}

/// A network interface an address was seen on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceRef {
    pub name: String,
    pub index: u32,
}

/// One address of a server, with the interfaces it was advertised on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub ip: IpAddr,
    pub class: AddressClass,
    pub interfaces: Vec<InterfaceRef>,
}

/// Where a server entry comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// mDNS advertisement. `instance` is the instance name as advertised, spaces included.
    Mdns {
        instance: String,
        service_type: String,
    },
    /// Typed by the user as `host:port`.
    Manual,
}

/// A server known to the application, from mDNS or manual entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredServer {
    pub id: ServerId,
    pub kind: ServiceKind,
    /// Display name: the mDNS instance name, or `host:port` for a manual entry.
    pub name: String,
    /// Host name as advertised, lower case, without the trailing dot; or the manual host.
    pub host: String,
    pub port: u16,
    /// Every known address, best first. May be empty for a manual entry given by name.
    pub addresses: Vec<ServerAddress>,
    pub origin: Origin,
    /// The advertisement went away, or a liveness check failed. The entry is kept so the user
    /// can retry; it becomes live again if the server is seen again.
    pub stale: bool,
}

impl DiscoveredServer {
    /// The HTTP base URL to connect to: the best address that a URL can carry, or the host name
    /// when no such address is known. `None` for a service that is not connectable.
    pub fn http_base(&self) -> Option<Url> {
        if !self.kind.connectable() {
            return None;
        }
        let host = self
            .addresses
            .iter()
            .find(|a| a.class != AddressClass::Ipv6LinkLocal)
            .map(|a| match a.ip {
                IpAddr::V4(v4) => v4.to_string(),
                IpAddr::V6(v6) => format!("[{v6}]"),
            })
            .unwrap_or_else(|| match self.host.parse::<IpAddr>() {
                Ok(IpAddr::V6(v6)) => format!("[{v6}]"),
                _ => self.host.clone(),
            });
        Url::parse(&format!("http://{host}:{}", self.port)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(kind: ServiceKind, addrs: Vec<(IpAddr, AddressClass)>) -> DiscoveredServer {
        DiscoveredServer {
            id: ServerId::next(),
            kind,
            name: "x".into(),
            host: "studio.local".into(),
            port: 2345,
            addresses: addrs
                .into_iter()
                .map(|(ip, class)| ServerAddress {
                    ip,
                    class,
                    interfaces: Vec::new(),
                })
                .collect(),
            origin: Origin::Manual,
            stale: false,
        }
    }

    #[test]
    fn ids_are_distinct() {
        assert_ne!(ServerId::next(), ServerId::next());
    }

    #[test]
    fn http_base_uses_best_usable_address() {
        let s = server(
            ServiceKind::OscQuery,
            vec![
                ("fe80::1".parse().unwrap(), AddressClass::Ipv6LinkLocal),
                ("fd00::2".parse().unwrap(), AddressClass::Ipv6),
            ],
        );
        assert_eq!(s.http_base().unwrap().as_str(), "http://[fd00::2]:2345/");
        let s = server(ServiceKind::OscQuery, vec![]);
        assert_eq!(s.http_base().unwrap().as_str(), "http://studio.local:2345/");
        let s = server(
            ServiceKind::OscOnly,
            vec![("10.0.0.2".parse().unwrap(), AddressClass::Ipv4)],
        );
        assert_eq!(s.http_base(), None);
    }
}
