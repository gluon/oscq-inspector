//! Discovery of OSCQuery servers: mDNS browsing behind a trait, plus manual entry.
//!
//! Must never depend on `egui` or `eframe`.

pub mod address;
pub mod manual;
pub mod mdns;
pub mod registry;

use oscq_core::DiscoveredServer;

pub use manual::{HISTORY_LEN, ManualEntry, ManualEntryError, ManualHistory};
pub use mdns::{MdnsSdConfig, MdnsSdDiscovery};

/// OSCQuery service type, `_oscjson._tcp`, with the `.local.` suffix `mdns-sd` requires.
pub const OSCQUERY_SERVICE: &str = "_oscjson._tcp.local.";

/// Plain OSC service type, browsed to show devices with no query support.
pub const OSC_SERVICE: &str = "_osc._udp.local.";

/// What a query for the local network met, and what came back: [`oscq_core::LocalNetwork`],
/// re-exported here because discovery is what observes it.
///
/// macOS asks the user before an application may reach the local network, and until it has an
/// answer the queries of that build go nowhere: refused outright for the first seconds, then
/// accepted and silently dropped. `mdns-sd` reports none of it, its send failures being traced
/// rather than raised, so a browse that has reached nothing looks exactly like a browse that has
/// reached the network and heard nothing. Only the second may be shown as "none found"
/// (CLAUDE.md law 1), and only an answer tells them apart.
pub use oscq_core::LocalNetwork;

/// What discovery reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A server seen for the first time.
    Found(DiscoveredServer),
    /// A known server gained addresses or interfaces, or came back from stale.
    Changed(DiscoveredServer),
    /// A known server is no longer advertised. It is kept so the user can retry.
    Stale(DiscoveredServer),
    /// What this machine's queries met: sent as soon as it is known, and on every change.
    LocalNetwork(LocalNetwork),
}

impl DiscoveryEvent {
    /// The server this event is about, or `None` for an event about the machine itself.
    pub fn server(&self) -> Option<&DiscoveredServer> {
        match self {
            DiscoveryEvent::Found(s) | DiscoveryEvent::Changed(s) | DiscoveryEvent::Stale(s) => {
                Some(s)
            }
            DiscoveryEvent::LocalNetwork(_) => None,
        }
    }
}

/// Sends a query for the service type browsed, and returns what the system answered: `Ok` if it
/// took the packet, the error it gave if it refused. It is a real query, so it costs no packet a
/// browse would not send anyway, and an answer to it reaches the daemon on the port the daemon
/// already listens on. `Ok` says only that the system took the query, never that it left: that
/// is why the state it feeds is [`LocalNetwork::Asked`] and not an opening. A test gives its own
/// answer instead.
pub type AccessProbe = std::sync::Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("mDNS daemon error: {0}")]
    Daemon(String),
    #[error("discovery is already running")]
    AlreadyStarted,
}

pub type Result<T> = std::result::Result<T, DiscoveryError>;

/// A discovery backend. Events arrive on the returned receiver until [`Discovery::stop`].
pub trait Discovery: Send {
    fn start(&mut self) -> Result<flume::Receiver<DiscoveryEvent>>;
    fn stop(&mut self);
}
