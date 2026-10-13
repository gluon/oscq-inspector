//! Manual `host:port` entry, the escape hatch for VPNs, routed subnets and firewalled labs.

use std::collections::VecDeque;
use std::fmt;
use std::net::IpAddr;

use oscq_core::{DiscoveredServer, Origin, ServerId, ServiceKind};

use crate::address;

/// Number of manual entries remembered.
pub const HISTORY_LEN: usize = 10;

/// A host and port typed by the user. The host is a name, an IPv4 address or an IPv6 address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualEntry {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManualEntryError {
    #[error("expected host:port, or [IPv6]:port")]
    Format,
    #[error("the port must be a number from 1 to 65535")]
    Port,
    #[error("the host is empty")]
    EmptyHost,
}

impl ManualEntry {
    pub fn new(host: &str, port: u16) -> Result<Self, ManualEntryError> {
        let host = host.trim();
        if host.is_empty() {
            return Err(ManualEntryError::EmptyHost);
        }
        if port == 0 {
            return Err(ManualEntryError::Port);
        }
        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }

    /// Parses `host:port`, `a.b.c.d:port` or `[v6]:port`. Surrounding spaces are ignored.
    pub fn parse(text: &str) -> Result<Self, ManualEntryError> {
        let text = text.trim();
        let (host, port) = if let Some(rest) = text.strip_prefix('[') {
            let (host, port) = rest.split_once("]:").ok_or(ManualEntryError::Format)?;
            host.parse::<std::net::Ipv6Addr>()
                .map_err(|_| ManualEntryError::Format)?;
            (host, port)
        } else {
            let (host, port) = text.rsplit_once(':').ok_or(ManualEntryError::Format)?;
            if host.contains(':') {
                // A bare IPv6 address is ambiguous with its port; brackets are required.
                return Err(ManualEntryError::Format);
            }
            (host, port)
        };
        let port: u16 = port.trim().parse().map_err(|_| ManualEntryError::Port)?;
        Self::new(host, port)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn same_target(&self, other: &Self) -> bool {
        self.port == other.port && self.host.eq_ignore_ascii_case(&other.host)
    }

    /// A server entry for this target, always treated as an OSCQuery server.
    pub fn to_server(&self) -> DiscoveredServer {
        let mut addresses = Vec::new();
        if let Ok(ip) = self.host.parse::<IpAddr>() {
            address::merge(&mut addresses, [(ip, Vec::new())]);
        }
        DiscoveredServer {
            id: ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: self.to_string(),
            host: self.host.clone(),
            port: self.port,
            addresses,
            origin: Origin::Manual,
            stale: false,
        }
    }
}

impl fmt::Display for ManualEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

/// The last [`HISTORY_LEN`] manual entries, most recent first, without duplicates. Persisting it
/// is the application's job, through [`ManualHistory::entries`] and [`ManualHistory::from_entries`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManualHistory {
    entries: VecDeque<ManualEntry>,
}

impl ManualHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restores a saved history, most recent first. Duplicates and entries beyond the limit are
    /// dropped.
    pub fn from_entries(entries: impl IntoIterator<Item = ManualEntry>) -> Self {
        let mut h = Self::new();
        let mut all: Vec<ManualEntry> = entries.into_iter().collect();
        all.reverse();
        for e in all {
            h.remember(e);
        }
        h
    }

    /// Puts an entry first, removing an earlier copy of it and the oldest entry beyond the limit.
    pub fn remember(&mut self, entry: ManualEntry) {
        self.entries.retain(|e| !e.same_target(&entry));
        self.entries.push_front(entry);
        self.entries.truncate(HISTORY_LEN);
    }

    /// Removes an entry, compared by target as `remember` does. Says whether it was there.
    pub fn forget(&mut self, entry: &ManualEntry) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| !e.same_target(entry));
        self.entries.len() != before
    }

    pub fn entries(&self) -> impl Iterator<Item = &ManualEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_and_ports() {
        let e = ManualEntry::parse(" studio.local:2345 ").unwrap();
        assert_eq!((e.host(), e.port()), ("studio.local", 2345));
        let e = ManualEntry::parse("10.0.0.2:5678").unwrap();
        assert_eq!(e.to_string(), "10.0.0.2:5678");
        let e = ManualEntry::parse("[fe80::1]:80").unwrap();
        assert_eq!((e.host(), e.port()), ("fe80::1", 80));
        assert_eq!(e.to_string(), "[fe80::1]:80");
    }

    #[test]
    fn rejects_bad_entries() {
        assert_eq!(
            ManualEntry::parse("studio.local"),
            Err(ManualEntryError::Format)
        );
        assert_eq!(
            ManualEntry::parse("fe80::1:80"),
            Err(ManualEntryError::Format)
        );
        assert_eq!(
            ManualEntry::parse("[nope]:80"),
            Err(ManualEntryError::Format)
        );
        assert_eq!(ManualEntry::parse("h:0"), Err(ManualEntryError::Port));
        assert_eq!(ManualEntry::parse("h:70000"), Err(ManualEntryError::Port));
        assert_eq!(ManualEntry::parse("h:x"), Err(ManualEntryError::Port));
        assert_eq!(ManualEntry::parse(":80"), Err(ManualEntryError::EmptyHost));
    }

    #[test]
    fn forgetting_an_entry_removes_it_by_target() {
        let mut h = ManualHistory::new();
        h.remember(ManualEntry::parse("localhost:30339").unwrap());
        h.remember(ManualEntry::parse("10.0.0.2:2345").unwrap());
        assert!(h.forget(&ManualEntry::parse("LOCALHOST:30339").unwrap()));
        assert!(!h.forget(&ManualEntry::parse("localhost:30339").unwrap()));
        let left: Vec<String> = h.entries().map(ToString::to_string).collect();
        assert_eq!(left, ["10.0.0.2:2345"]);
    }

    #[test]
    fn remembers_last_ten_most_recent_first() {
        let mut h = ManualHistory::new();
        for i in 1..=12u16 {
            h.remember(ManualEntry::new("h", i).unwrap());
        }
        let ports: Vec<u16> = h.entries().map(ManualEntry::port).collect();
        assert_eq!(ports, [12, 11, 10, 9, 8, 7, 6, 5, 4, 3]);
    }

    #[test]
    fn reusing_an_entry_moves_it_first_without_duplicate() {
        let mut h = ManualHistory::new();
        h.remember(ManualEntry::parse("a:1").unwrap());
        h.remember(ManualEntry::parse("b:2").unwrap());
        h.remember(ManualEntry::parse("A:1").unwrap());
        let all: Vec<String> = h.entries().map(ToString::to_string).collect();
        assert_eq!(all, ["A:1", "b:2"]);
    }

    #[test]
    fn restores_saved_history() {
        let saved: Vec<ManualEntry> = (1..=12u16)
            .map(|p| ManualEntry::new("h", p).unwrap())
            .collect();
        let h = ManualHistory::from_entries(saved);
        let ports: Vec<u16> = h.entries().map(ManualEntry::port).collect();
        assert_eq!(ports, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn manual_server_is_connectable() {
        let s = ManualEntry::parse("10.0.0.2:2345").unwrap().to_server();
        assert_eq!(s.origin, Origin::Manual);
        assert_eq!(s.http_base().unwrap().as_str(), "http://10.0.0.2:2345/");
        let s = ManualEntry::parse("[::1]:2345").unwrap().to_server();
        assert_eq!(s.http_base().unwrap().as_str(), "http://[::1]:2345/");
    }
}
