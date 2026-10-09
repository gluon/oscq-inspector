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

/// What discovery reports. Each event carries a full snapshot of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A server seen for the first time.
    Found(DiscoveredServer),
    /// A known server gained addresses or interfaces, or came back from stale.
    Changed(DiscoveredServer),
    /// A known server is no longer advertised. It is kept so the user can retry.
    Stale(DiscoveredServer),
}

impl DiscoveryEvent {
    pub fn server(&self) -> &DiscoveredServer {
        match self {
            DiscoveryEvent::Found(s) | DiscoveryEvent::Changed(s) | DiscoveryEvent::Stale(s) => s,
        }
    }
}

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
