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

/// Whether a query can leave this machine at all.
///
/// macOS asks the user before an application may reach the local network, and refuses every
/// multicast send until it has an answer. `mdns-sd` reports nothing of this: its send failures
/// are traced, not raised, so a browse that has asked the network nothing looks exactly like a
/// browse that has asked and heard nothing. The difference matters, because only one of the two
/// may be shown as "none found" (CLAUDE.md law 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalNetwork {
    /// A query has left this machine. What is not found after this really is not answering.
    Open,
    /// The system refused to send, or has not decided yet. Nothing has been asked of the
    /// network, so nothing may be concluded about it. `reason` is the system's own words.
    Refused { reason: String },
}

/// What discovery reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A server seen for the first time.
    Found(DiscoveredServer),
    /// A known server gained addresses or interfaces, or came back from stale.
    Changed(DiscoveredServer),
    /// A known server is no longer advertised. It is kept so the user can retry.
    Stale(DiscoveredServer),
    /// Whether queries can leave this machine: sent as soon as it is known, and on every change.
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

/// Asks the system whether a query may leave this machine, returning its refusal when it may
/// not. The default sends a real query for the service type browsed, so the question costs no
/// packet that a browse would not send anyway; a test gives its own answer instead.
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
