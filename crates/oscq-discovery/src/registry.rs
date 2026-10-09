//! Discovery state, independent of the mDNS backend: deduplication by host and port, stale
//! marking, revival, address merging.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

use oscq_core::{DiscoveredServer, InterfaceRef, Origin, ServerId, ServiceKind};

use crate::DiscoveryEvent;
use crate::address;

/// One resolved advertisement, as delivered by the backend.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub kind: ServiceKind,
    /// Service type with domain, for example `_oscjson._tcp.local.`.
    pub service_type: String,
    /// Full service instance name, for example `OSCQuery Helper._oscjson._tcp.local.`.
    pub fullname: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<(IpAddr, Vec<InterfaceRef>)>,
}

/// Deduplication key: one logical server per service kind, host and port. Never the instance
/// name, which two different servers may share.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Key {
    kind: KindKey,
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum KindKey {
    OscQuery,
    OscOnly,
}

impl From<ServiceKind> for KindKey {
    fn from(k: ServiceKind) -> Self {
        match k {
            ServiceKind::OscQuery => KindKey::OscQuery,
            ServiceKind::OscOnly => KindKey::OscOnly,
        }
    }
}

struct Entry {
    server: DiscoveredServer,
    /// Instance full names currently advertising this server.
    live_sources: BTreeSet<String>,
}

#[derive(Default)]
pub struct Registry {
    entries: HashMap<Key, Entry>,
    by_source: HashMap<String, BTreeSet<Key>>,
}

/// Host names compare case insensitively and with or without the trailing root dot.
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The instance name from a full name, by removing the known service type suffix. Works with
/// spaces and dots inside the instance name.
pub fn instance_name(fullname: &str, service_type: &str) -> String {
    fullname
        .strip_suffix(service_type)
        .and_then(|s| s.strip_suffix('.'))
        .filter(|s| !s.is_empty())
        .unwrap_or(fullname)
        .to_owned()
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an advertisement. Returns `Found` for a new server, `Changed` when a known server
    /// gained addresses or came back from stale, nothing when the observation adds nothing.
    pub fn observe(&mut self, obs: Observation) -> Option<DiscoveryEvent> {
        let key = Key {
            kind: obs.kind.into(),
            host: normalize_host(&obs.host),
            port: obs.port,
        };
        self.by_source
            .entry(obs.fullname.clone())
            .or_default()
            .insert(key.clone());

        if let Some(entry) = self.entries.get_mut(&key) {
            let before = entry.server.clone();
            entry.live_sources.insert(obs.fullname);
            address::merge(&mut entry.server.addresses, obs.addresses);
            entry.server.stale = false;
            return (entry.server != before).then(|| DiscoveryEvent::Changed(entry.server.clone()));
        }

        let instance = instance_name(&obs.fullname, &obs.service_type);
        let mut addresses = Vec::new();
        address::merge(&mut addresses, obs.addresses);
        let server = DiscoveredServer {
            id: ServerId::next(),
            kind: obs.kind,
            name: instance.clone(),
            host: key.host.clone(),
            port: obs.port,
            addresses,
            origin: Origin::Mdns {
                instance,
                service_type: obs.service_type,
            },
            stale: false,
        };
        self.entries.insert(
            key,
            Entry {
                server: server.clone(),
                live_sources: BTreeSet::from([obs.fullname]),
            },
        );
        Some(DiscoveryEvent::Found(server))
    }

    /// Records that an instance is no longer advertised: goodbye, cache expiry or failed liveness
    /// check, which the backend cannot tell apart. Servers left with no live advertisement become
    /// stale and are kept.
    pub fn source_removed(&mut self, fullname: &str) -> Vec<DiscoveryEvent> {
        let Some(keys) = self.by_source.remove(fullname) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for key in keys {
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.live_sources.remove(fullname);
                if entry.live_sources.is_empty() && !entry.server.stale {
                    entry.server.stale = true;
                    events.push(DiscoveryEvent::Stale(entry.server.clone()));
                }
            }
        }
        events
    }

    /// Full names currently advertised, for the liveness check.
    pub fn live_sources(&self) -> Vec<String> {
        self.by_source.keys().cloned().collect()
    }

    pub fn servers(&self) -> impl Iterator<Item = &DiscoveredServer> {
        self.entries.values().map(|e| &e.server)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names() {
        assert_eq!(
            instance_name(
                "OSCQuery Helper._oscjson._tcp.local.",
                "_oscjson._tcp.local."
            ),
            "OSCQuery Helper"
        );
        assert_eq!(
            instance_name("v1.2 rig._osc._udp.local.", "_osc._udp.local."),
            "v1.2 rig"
        );
        assert_eq!(instance_name("odd", "_osc._udp.local."), "odd");
    }

    #[test]
    fn hosts() {
        assert_eq!(normalize_host("Studio-Mac.local."), "studio-mac.local");
        assert_eq!(normalize_host("studio-mac.local"), "studio-mac.local");
    }
}
