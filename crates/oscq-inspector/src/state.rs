//! Application state, owned by the UI thread, changed only by applying network events and by the
//! user. No I/O here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use oscq_core::document::parse_value;
use oscq_core::ws::{is_within, renamed};
use oscq_core::{
    AttributeReply, Diagnostic, DiscoveredServer, Event, HostInfo, Node, Origin, OscDestination,
    PeerClient, Peers, SendBlocked, ServerId, TransportState, UserFacingError,
};
use rosc::OscType;

use crate::traffic::TrafficLog;
use crate::tree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Idle,
    Connecting,
    /// Tree loaded over HTTP; values on refresh only.
    Connected,
    Failed(String),
    Disconnected,
}

/// The WebSocket of a connected server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WsState {
    /// None: values on refresh only.
    #[default]
    Absent,
    /// Accepted by the server, nothing received on it yet. A server may accept and never
    /// notify (Max 9): values are on refresh only as far as has been observed.
    Open,
    /// The server has sent at least one notification or value on this socket.
    Live,
    /// Lost or refused, retried after `retry_in`.
    Lost { retry_in: Duration, reason: String },
}

#[derive(Debug, Default)]
pub struct TreeState {
    pub root: Node,
    /// The body of the root document, exactly as received.
    pub raw: Arc<str>,
    /// Bodies of subtrees fetched on their own, by full path, exactly as received.
    pub raw_subtrees: HashMap<String, Arc<str>>,
    pub expanded: HashSet<String>,
    /// Containers whose subtree has been fetched since the tree loaded.
    pub fetched: HashSet<String>,
    /// Values sent and not yet replaced by a value read from the server, drawn as unconfirmed.
    /// On a node that cannot be read, they stay until the next send.
    pub pending: HashMap<String, Vec<OscType>>,
    /// Paths whose value query answered 204 or an empty object.
    pub unavailable: HashSet<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Bumped on every structural change, so derived views know to rebuild.
    pub version: u64,
    /// Every node by full path, rebuilt on each structural change: a pushed value finds its node
    /// without walking the tree.
    pub paths: HashMap<String, tree::NodeIndex>,
    /// Every node, the root included, counted on each structural change rather than per frame.
    pub nodes: usize,
}

impl TreeState {
    fn reindex(&mut self) {
        self.paths = tree::index_paths(&self.root);
        self.nodes = self.root.count();
    }

    pub fn node(&self, path: &str) -> Option<&Node> {
        self.paths
            .get(path)
            .and_then(|i| tree::node_at(&self.root, i))
    }
}

/// One reading of a server's `?PEERS` (PRD 7.6): what it published, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct PeersSnapshot {
    /// When the answer arrived, on the monotonic clock: what its age is counted from.
    pub at: Instant,
    /// The same moment on the wall clock, for the places that say a time rather than an age.
    pub wall: SystemTime,
    /// The reply, each client's `LISTEN` sorted and without repeats, so that one address is
    /// looked up in it without walking it.
    pub peers: Peers,
    /// The client that is this application's own WebSocket on that server, when it is there.
    pub this_client: Option<usize>,
}

impl PeersSnapshot {
    /// The clients other than this application that listen to exactly `path`. `LISTEN` is not
    /// recursive (PRD 7.4): a listener of `/foo` is not one of `/foo/bar`, and an address that
    /// only starts like `path` is another address.
    pub fn listeners<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a PeerClient> + 'a {
        self.peers
            .clients
            .iter()
            .enumerate()
            .filter(move |(i, c)| {
                Some(*i) != self.this_client
                    && c.listen
                        .as_ref()
                        .is_some_and(|l| l.binary_search_by(|p| p.as_str().cmp(path)).is_ok())
            })
            .map(|(_, c)| c)
    }
}

/// How a client is called where it is listed: by the name it gave, or, having given none, as
/// an unnamed client at its address and port. It is never given another's name (PRD 9.10).
pub fn client_label(c: &PeerClient) -> String {
    match &c.name {
        Some(name) => name.clone(),
        None => format!("unnamed client {}", ip_port(&c.ip, c.port)),
    }
}

/// A failure's reason without the address of the request it names, for a place that already
/// says which server it is and which request failed: "answered HTTP 500" for
/// "http://10.0.0.2:5000/?PEERS answered HTTP 500". The reason whole stays on hover.
pub fn short_reason(reason: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for word in reason.split_whitespace() {
        if word.starts_with("http://") || word.starts_with("https://") {
            if matches!(words.last().map(String::as_str), Some("from" | "to")) {
                words.pop();
            }
            // Its punctuation stays, on the word before it.
            if let Some(last) = words.last_mut()
                && word.ends_with(':')
            {
                last.push(':');
            }
            continue;
        }
        words.push(word.to_owned());
    }
    if words.is_empty() {
        reason.to_owned()
    } else {
        words.join(" ")
    }
}

/// `ip:port`, with brackets around an IPv6 address.
pub fn ip_port(ip: &str, port: u16) -> String {
    if ip.contains(':') && !ip.starts_with('[') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    }
}

/// What this application holds of a server's peers.
#[derive(Debug, Default)]
pub struct PeersState {
    /// The last reading that succeeded.
    pub snapshot: Option<PeersSnapshot>,
    /// Why the last attempt failed, when it did. A later success clears it.
    pub failure: Option<String>,
    /// A reading was asked for and its answer has not arrived: no other is asked meanwhile.
    pub asked: bool,
}

/// What is known of a server's peers, and so what the map and the tree may say of them.
#[derive(Debug, PartialEq)]
pub enum PeersView<'a> {
    /// Not connected, and nothing was ever read: nothing is known, its declaration included.
    NotConnected,
    /// Connected, but its `HOST_INFO` is not in hand: what it declares is not known.
    HostInfoUnknown,
    /// Its `HOST_INFO` does not declare `PEERS`: it publishes none, and is asked for none.
    NotPublished,
    /// It declares `PEERS`, and no reading has been made.
    NotRead,
    /// It declares `PEERS`, and no reading has succeeded: why the last one failed.
    Failed(&'a str),
    /// The last reading that succeeded, and why a later one failed, if one did.
    Read {
        snapshot: &'a PeersSnapshot,
        failure: Option<&'a str>,
    },
}

#[derive(Debug)]
pub struct ServerEntry {
    pub server: DiscoveredServer,
    pub conn: ConnState,
    pub host_info: Option<HostInfo>,
    pub destination: Option<Result<OscDestination, SendBlocked>>,
    pub host_info_error: Option<String>,
    pub tree: Option<TreeState>,
    pub traffic: TrafficLog,
    pub last_error: Option<UserFacingError>,
    pub ws: WsState,
    /// Methods the network service has been asked to `LISTEN` to on this connection.
    pub listening: HashSet<String>,
    pub peers: PeersState,
}

impl ServerEntry {
    pub fn new(server: DiscoveredServer) -> Self {
        Self {
            server,
            conn: ConnState::Idle,
            host_info: None,
            destination: None,
            host_info_error: None,
            tree: None,
            traffic: TrafficLog::default(),
            last_error: None,
            ws: WsState::Absent,
            listening: HashSet::new(),
            peers: PeersState::default(),
        }
    }

    /// The server declares `PEERS` in the `HOST_INFO` of this connection (PRD 7.6). Only then
    /// is it asked for its peers, and only then is anything it said of them shown.
    pub fn publishes_peers(&self) -> bool {
        self.host_info
            .as_ref()
            .is_some_and(HostInfo::publishes_peers)
    }

    /// Its peers may be read now: connected, and declaring them.
    pub fn peers_readable(&self) -> bool {
        self.conn == ConnState::Connected && self.publishes_peers()
    }

    /// What is known of its peers. A reading is shown only for a server that declares `PEERS`,
    /// or that declared it on a connection since closed: a server connected now whose
    /// `HOST_INFO` declares nothing has no peers to show, whatever was read before.
    pub fn peers_view(&self) -> PeersView<'_> {
        let connected = self.conn == ConnState::Connected;
        if connected && self.host_info.is_none() {
            return PeersView::HostInfoUnknown;
        }
        if connected && !self.publishes_peers() {
            return PeersView::NotPublished;
        }
        match (&self.peers.snapshot, &self.peers.failure) {
            (Some(snapshot), failure) => PeersView::Read {
                snapshot,
                failure: failure.as_deref(),
            },
            (None, _) if !connected => PeersView::NotConnected,
            (None, Some(reason)) => PeersView::Failed(reason),
            (None, None) => PeersView::NotRead,
        }
    }

    /// `host:port`, the key used in the config.
    pub fn key(&self) -> String {
        host_port(&self.server)
    }

    /// The proposal: absent `ACCESS` is readable only if `VALUE` is listed in `EXTENSIONS`.
    pub fn value_supported(&self) -> bool {
        self.host_info.as_ref().is_some_and(|h| h.supports("VALUE"))
    }

    /// Why nothing can be sent to this server, if anything prevents it. A connection that is not
    /// up blocks every send, whatever the nodes declare: their access stays the server's.
    pub fn send_blocked(&self) -> Option<String> {
        match &self.conn {
            ConnState::Connected => {}
            ConnState::Failed(reason) => return Some(format!("not connected: {reason}")),
            ConnState::Connecting => return Some("connecting".to_owned()),
            ConnState::Idle | ConnState::Disconnected => return Some("not connected".to_owned()),
        }
        match (&self.destination, &self.host_info_error) {
            (Some(Ok(d)), _) => d.check_sendable().err().map(|e| e.to_string()),
            (Some(Err(e)), _) => Some(e.to_string()),
            (None, Some(e)) => Some(format!("HOST_INFO unavailable: {e}")),
            (None, None) => Some("HOST_INFO not received yet".to_owned()),
        }
    }
}

/// Whether a freshly loaded document settles the value sent to `path`: only a value actually
/// read does, which a node that cannot be read never carries. A node gone from the tree drops it.
fn settled(root: &Node, path: &str, value_supported: bool) -> bool {
    root.find(path)
        .is_none_or(|n| n.effective_access(value_supported).readable() && n.value.is_some())
}

pub fn host_port(s: &DiscoveredServer) -> String {
    if s.host.contains(':') {
        format!("[{}]:{}", s.host, s.port)
    } else {
        format!("{}:{}", s.host, s.port)
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    pub servers: Vec<ServerEntry>,
    pub selected: Option<ServerId>,
    /// Errors not tied to a server.
    pub global_error: Option<String>,
}

impl AppState {
    pub fn entry(&self, id: ServerId) -> Option<&ServerEntry> {
        self.servers.iter().find(|e| e.server.id == id)
    }

    pub fn entry_mut(&mut self, id: ServerId) -> Option<&mut ServerEntry> {
        self.servers.iter_mut().find(|e| e.server.id == id)
    }

    pub fn selected_entry(&self) -> Option<&ServerEntry> {
        self.selected.and_then(|id| self.entry(id))
    }

    /// Removes a server entry, its tree and traffic with it.
    pub fn remove(&mut self, id: ServerId) -> Option<ServerEntry> {
        let i = self.servers.iter().position(|e| e.server.id == id)?;
        Some(self.servers.remove(i))
    }

    /// Adds a manual server, or returns the existing manual entry for the same `host:port`.
    pub fn add_manual(&mut self, server: DiscoveredServer) -> ServerId {
        let key = host_port(&server);
        if let Some(e) = self
            .servers
            .iter()
            .find(|e| e.server.origin == Origin::Manual && e.key().eq_ignore_ascii_case(&key))
        {
            return e.server.id;
        }
        let id = server.id;
        self.servers.push(ServerEntry::new(server));
        id
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::ServerFound(s) | Event::ServerChanged(s) | Event::ServerStale(s) => {
                match self.entry_mut(s.id) {
                    Some(e) => e.server = s,
                    None => self.servers.push(ServerEntry::new(s)),
                }
            }
            Event::HostInfo {
                server_id,
                info,
                destination,
            } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.host_info = Some(info);
                    e.destination = Some(destination);
                    e.host_info_error = None;
                    // What a server no longer declares, it no longer publishes.
                    if !e.publishes_peers() {
                        e.peers = PeersState::default();
                    }
                }
            }
            Event::HostInfoUnavailable { server_id, reason } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.host_info = None;
                    e.destination = None;
                    e.host_info_error = Some(reason);
                    e.peers = PeersState::default();
                }
            }
            Event::Peers {
                server_id,
                at,
                result,
            } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.peers.asked = false;
                    // PRD 9.10: nothing is shown of the peers of a server that does not
                    // declare them, an answer that arrives all the same included.
                    if !e.publishes_peers() {
                        return;
                    }
                    match result {
                        Ok(reading) => {
                            let mut peers = reading.peers;
                            for c in &mut peers.clients {
                                if let Some(l) = c.listen.as_mut() {
                                    l.sort_unstable();
                                    l.dedup();
                                }
                            }
                            let wall = SystemTime::now()
                                .checked_sub(at.elapsed())
                                .unwrap_or_else(SystemTime::now);
                            e.peers.snapshot = Some(PeersSnapshot {
                                at,
                                wall,
                                peers,
                                this_client: reading.this_client,
                            });
                            e.peers.failure = None;
                        }
                        Err(reason) => e.peers.failure = Some(reason),
                    }
                }
            }
            Event::TreeLoaded {
                server_id,
                raw,
                report,
            } => {
                if let Some(e) = self.entry_mut(server_id) {
                    let value_supported = e.value_supported();
                    let previous = e.tree.take();
                    let version = previous.as_ref().map_or(0, |t| t.version + 1);
                    let (expanded, mut pending) = previous
                        .map(|t| (t.expanded, t.pending))
                        .unwrap_or_default();
                    pending.retain(|p, _| !settled(&report.root, p, value_supported));
                    let mut t = TreeState {
                        root: report.root,
                        raw,
                        diagnostics: report.diagnostics,
                        expanded,
                        pending,
                        version,
                        ..TreeState::default()
                    };
                    t.reindex();
                    e.tree = Some(t);
                }
            }
            Event::SubtreeLoaded {
                server_id,
                path,
                raw,
                report,
            } => {
                if let Some(e) = self.entry_mut(server_id)
                    && let value_supported = e.value_supported()
                    && let Some(t) = e.tree.as_mut()
                    && let Some(index) = tree::find(&t.root, &path)
                    && let Some(node) = tree::node_at_mut(&mut t.root, &index)
                {
                    *node = report.root;
                    t.raw_subtrees.insert(path.clone(), raw);
                    t.fetched.insert(path.clone());
                    let prefix = if path == "/" {
                        "/".to_owned()
                    } else {
                        format!("{path}/")
                    };
                    let root = &t.root;
                    t.pending.retain(|p, _| {
                        (*p != path && !p.starts_with(&prefix))
                            || !settled(root, p, value_supported)
                    });
                    t.unavailable
                        .retain(|p| *p != path && !p.starts_with(&prefix));
                    t.version += 1;
                    t.reindex();
                }
            }
            Event::ValueChanged {
                server_id,
                path,
                value,
            } => {
                if let Some(t) = self.entry_mut(server_id).and_then(|e| e.tree.as_mut()) {
                    let node = t
                        .paths
                        .get(&path)
                        .and_then(|i| tree::node_at_mut(&mut t.root, i));
                    match (value, node) {
                        (AttributeReply::Value(v), Some(node)) => {
                            node.value = Some(parse_value(v));
                            t.pending.remove(&path);
                            t.unavailable.remove(&path);
                        }
                        // No value read: what the user sent stays shown, unconfirmed.
                        (AttributeReply::Unsupported | AttributeReply::NoContent, _) => {
                            t.unavailable.insert(path);
                        }
                        (AttributeReply::Value(_), None) => {}
                    }
                }
            }
            Event::TransportState { server_id, state } => {
                if let Some(e) = self.entry_mut(server_id) {
                    match state {
                        TransportState::Connecting => e.conn = ConnState::Connecting,
                        TransportState::HttpOnly => e.conn = ConnState::Connected,
                        TransportState::WebSocketOpen => e.ws = WsState::Open,
                        TransportState::WebSocketLive => e.ws = WsState::Live,
                        TransportState::WebSocketLost { retry_in, reason } => {
                            e.ws = WsState::Lost { retry_in, reason }
                        }
                        TransportState::Disconnected => e.conn = ConnState::Disconnected,
                        TransportState::Failed(r) => e.conn = ConnState::Failed(r),
                    }
                    // A new or closed connection starts with no WebSocket and no listens, and
                    // with no reading on its way: an answer to the old one is dropped.
                    if !matches!(e.conn, ConnState::Connected) {
                        e.ws = WsState::Absent;
                        e.listening.clear();
                        e.peers.asked = false;
                    }
                }
            }
            // The parent's subtree arrives on its own; nothing to keep for the new path.
            Event::PathAdded { .. } => {}
            Event::PathRemoved { server_id, path } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.listening.retain(|p| !is_within(p, &path));
                    if let Some(t) = e.tree.as_mut() {
                        t.pending.retain(|p, _| !is_within(p, &path));
                        t.unavailable.retain(|p| !is_within(p, &path));
                        t.expanded.retain(|p| !is_within(p, &path));
                        t.fetched.retain(|p| !is_within(p, &path));
                        t.raw_subtrees.retain(|p, _| !is_within(p, &path));
                    }
                }
            }
            Event::PathRenamed {
                server_id,
                old,
                new,
            } => {
                if let Some(e) = self.entry_mut(server_id) {
                    let set = |s: &mut HashSet<String>| {
                        *s = s
                            .drain()
                            .map(|p| renamed(&p, &old, &new).unwrap_or(p))
                            .collect();
                    };
                    set(&mut e.listening);
                    if let Some(t) = e.tree.as_mut() {
                        set(&mut t.unavailable);
                        set(&mut t.expanded);
                        t.pending = t
                            .pending
                            .drain()
                            .map(|(p, v)| (renamed(&p, &old, &new).unwrap_or(p), v))
                            .collect();
                        // Reloaded from the parents, whose bodies replace these.
                        t.fetched.retain(|p| !is_within(p, &old));
                        t.raw_subtrees.retain(|p, _| !is_within(p, &old));
                    }
                }
            }
            Event::TrafficLogged(r) => {
                if let Some(e) = self.entry_mut(r.server_id) {
                    e.traffic.push(r);
                }
            }
            Event::Error { server_id, error } => {
                match server_id.and_then(|id| self.entry_mut(id)) {
                    Some(e) => e.last_error = Some(error),
                    None => {
                        self.global_error = Some(match &error.path {
                            Some(p) => format!("{p}: {}", error.message),
                            None => error.message,
                        })
                    }
                }
            }
            // PRD 9.6: an error the same operation has since overcome is no longer shown.
            Event::Succeeded {
                server_id,
                operation,
                path,
            } => {
                if let Some(e) = self.entry_mut(server_id)
                    && e.last_error.as_ref().is_some_and(|err| {
                        err.operation == operation && (path.is_none() || err.path == path)
                    })
                {
                    e.last_error = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oscq_core::{ServiceKind, parse_document};
    use serde_json::json;

    /// PRD 9.10: an answer about the peers of a server that does not declare them is not kept,
    /// even one that arrives: had it been, the server declaring them later would show, as its
    /// own, a reading made before it published anything.
    #[test]
    fn an_answer_from_a_server_that_does_not_declare_peers_is_not_kept() {
        let mut st = AppState::default();
        let s = server();
        let id = s.id;
        st.apply(Event::ServerFound(s));
        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::HttpOnly,
        });
        let host_info = |json: &str| {
            let (info, _) = HostInfo::parse(json.as_bytes()).unwrap();
            let destination = info.osc_destination("studio.local", 2345);
            Event::HostInfo {
                server_id: id,
                info,
                destination,
            }
        };
        let answer = || {
            let (peers, _) = oscq_core::Peers::parse(
                br#"{"CLIENTS":[{"IP":"10.0.0.9","PORT":1,"LISTEN":["/a"]}]}"#,
            )
            .unwrap();
            Event::Peers {
                server_id: id,
                at: Instant::now(),
                result: Ok(oscq_core::PeersReading {
                    peers,
                    this_client: None,
                }),
            }
        };
        st.apply(host_info(r#"{"EXTENSIONS":{}}"#));
        st.apply(answer());
        let e = st.entry(id).unwrap();
        assert_eq!(e.peers.snapshot, None, "not kept");
        assert_eq!(e.peers_view(), PeersView::NotPublished);
        st.apply(host_info(r#"{"EXTENSIONS":{"PEERS":true}}"#));
        assert_eq!(
            st.entry(id).unwrap().peers_view(),
            PeersView::NotRead,
            "declared now, and nothing read since"
        );
        st.apply(answer());
        assert!(matches!(
            st.entry(id).unwrap().peers_view(),
            PeersView::Read { .. }
        ));
        // Declared no more: what it published before is not shown, nor kept.
        st.apply(host_info(r#"{"EXTENSIONS":{"PEERS":false}}"#));
        let e = st.entry(id).unwrap();
        assert_eq!(e.peers_view(), PeersView::NotPublished);
        assert_eq!(e.peers.snapshot, None);
    }

    /// The reasons the client gives for a failed `?PEERS`, each without its address.
    #[test]
    fn a_short_reason_drops_the_address_and_keeps_the_rest() {
        let url = "http://10.0.0.2:5000/?PEERS";
        for (reason, short) in [
            (format!("{url} answered HTTP 500"), "answered HTTP 500"),
            (
                format!("{url} answered 204 with no document"),
                "answered 204 with no document",
            ),
            (
                format!("could not connect to {url}: tcp connect error: Connection refused"),
                "could not connect: tcp connect error: Connection refused",
            ),
            (
                format!("no answer from {url} within the timeout"),
                "no answer within the timeout",
            ),
            (
                format!("invalid PEERS reply from {url}: CLIENTS is not a list"),
                "invalid PEERS reply: CLIENTS is not a list",
            ),
            (
                format!("response from {url} is larger than 9 bytes"),
                "response is larger than 9 bytes",
            ),
            ("not connected".to_owned(), "not connected"),
            (url.to_owned(), url),
        ] {
            assert_eq!(short_reason(&reason), short, "{reason}");
        }
    }

    fn server() -> DiscoveredServer {
        DiscoveredServer {
            id: ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: "OSCQuery Helper".into(),
            host: "studio.local".into(),
            port: 2345,
            addresses: Vec::new(),
            origin: Origin::Mdns {
                instance: "OSCQuery Helper".into(),
                service_type: "_oscjson._tcp.local.".into(),
            },
            stale: false,
        }
    }

    /// PRD 9.6: an error is cleared by the success of the operation it comes from, on the same
    /// path, and by nothing else.
    #[test]
    fn an_error_is_cleared_only_by_the_same_operation_succeeding() {
        use oscq_core::{Operation, UserFacingError};
        let mut s = AppState::default();
        let srv = server();
        let id = srv.id;
        s.apply(Event::ServerFound(srv));
        let fail = |s: &mut AppState, operation, path: Option<&str>| {
            s.apply(Event::Error {
                server_id: Some(id),
                error: UserFacingError {
                    operation,
                    path: path.map(str::to_owned),
                    message: "failed".into(),
                },
            });
        };
        let succeed = |s: &mut AppState, operation, path: Option<&str>| {
            s.apply(Event::Succeeded {
                server_id: id,
                operation,
                path: path.map(str::to_owned),
            });
        };
        let shown = |s: &AppState| s.entry(id).unwrap().last_error.is_some();
        fail(&mut s, Operation::Connect, Some("/"));
        succeed(&mut s, Operation::LoadTree, Some("/"));
        succeed(&mut s, Operation::ReadValue, None);
        assert!(
            shown(&s),
            "another operation does not clear a connection error"
        );
        succeed(&mut s, Operation::Connect, None);
        assert!(
            !shown(&s),
            "a connection that succeeds clears the failed one"
        );

        for op in [Operation::LoadTree, Operation::ReadValue, Operation::Send] {
            fail(&mut s, op, Some("/a/x"));
            succeed(&mut s, op, Some("/b"));
            assert!(shown(&s), "{op:?} on another path does not clear it");
            for other in [Operation::LoadTree, Operation::ReadValue, Operation::Send] {
                if other != op {
                    succeed(&mut s, other, Some("/a/x"));
                }
            }
            assert!(
                shown(&s),
                "another operation on the path does not clear {op:?}"
            );
            succeed(&mut s, op, Some("/a/x"));
            assert!(!shown(&s), "{op:?} succeeding on the path clears it");
        }
    }

    fn loaded(s: &mut AppState, id: ServerId) {
        let raw = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","CONTENTS":{"x":{"FULL_PATH":"/a/x","TYPE":"f","VALUE":[0.1]}}},"b":{"FULL_PATH":"/b","TYPE":"i"}}}"#;
        s.apply(Event::TreeLoaded {
            server_id: id,
            raw: Arc::from(raw),
            report: parse_document(raw.as_bytes()).unwrap(),
        });
    }

    #[test]
    fn found_changed_stale_update_one_entry() {
        let mut st = AppState::default();
        let mut s = server();
        st.apply(Event::ServerFound(s.clone()));
        s.stale = true;
        st.apply(Event::ServerStale(s.clone()));
        assert_eq!(st.servers.len(), 1);
        assert!(st.servers[0].server.stale);
        s.stale = false;
        st.apply(Event::ServerChanged(s));
        assert!(!st.servers[0].server.stale);
    }

    #[test]
    fn tree_values_pending_and_subtrees() {
        let mut st = AppState::default();
        let s = server();
        let id = s.id;
        st.apply(Event::ServerFound(s));
        loaded(&mut st, id);
        let t = st.entry_mut(id).unwrap().tree.as_mut().unwrap();
        t.expanded.insert("/a".into());
        t.pending.insert("/a/x".into(), vec![OscType::Float(0.9)]);

        st.apply(Event::ValueChanged {
            server_id: id,
            path: "/a/x".into(),
            value: AttributeReply::Value(json!([0.9])),
        });
        let t = st.entry(id).unwrap().tree.as_ref().unwrap();
        assert!(t.pending.is_empty(), "confirmed by the server");
        let x = t.root.find("/a/x").unwrap();
        assert_eq!(crate::widget::value_f64(x.value.as_ref(), 0), Some(0.9));

        st.apply(Event::ValueChanged {
            server_id: id,
            path: "/b".into(),
            value: AttributeReply::NoContent,
        });
        assert!(
            st.entry(id)
                .unwrap()
                .tree
                .as_ref()
                .unwrap()
                .unavailable
                .contains("/b")
        );

        let sub = r#"{"FULL_PATH":"/a","CONTENTS":{"y":{"FULL_PATH":"/a/y","TYPE":"s"}}}"#;
        st.apply(Event::SubtreeLoaded {
            server_id: id,
            path: "/a".into(),
            raw: Arc::from(sub),
            report: parse_document(sub.as_bytes()).unwrap(),
        });
        let t = st.entry(id).unwrap().tree.as_ref().unwrap();
        assert!(t.root.find("/a/y").is_some() && t.root.find("/a/x").is_none());
        assert_eq!(t.raw_subtrees.get("/a").map(|r| &**r), Some(sub));
        assert!(t.fetched.contains("/a"));
        assert!(t.expanded.contains("/a"), "expansion kept");

        loaded(&mut st, id);
        assert!(
            st.entry(id)
                .unwrap()
                .tree
                .as_ref()
                .unwrap()
                .expanded
                .contains("/a"),
            "expansion kept across reload"
        );
    }

    #[test]
    fn a_reload_replaces_a_sent_value_only_with_a_value_read() {
        let mut st = AppState::default();
        let s = server();
        let id = s.id;
        st.apply(Event::ServerFound(s));
        loaded(&mut st, id);
        let t = st.entry_mut(id).unwrap().tree.as_mut().unwrap();
        for p in ["/w", "/r", "/r_none", "/gone"] {
            t.pending.insert(p.into(), vec![OscType::Int(7)]);
        }
        let raw = r#"{"FULL_PATH":"/","CONTENTS":{
            "w":{"FULL_PATH":"/w","TYPE":"i","ACCESS":2,"VALUE":[1]},
            "r":{"FULL_PATH":"/r","TYPE":"i","ACCESS":3,"VALUE":[1]},
            "r_none":{"FULL_PATH":"/r_none","TYPE":"i","ACCESS":3}}}"#;
        st.apply(Event::TreeLoaded {
            server_id: id,
            raw: Arc::from(raw),
            report: parse_document(raw.as_bytes()).unwrap(),
        });
        let t = st.entry(id).unwrap().tree.as_ref().unwrap();
        let mut kept: Vec<&str> = t.pending.keys().map(String::as_str).collect();
        kept.sort_unstable();
        assert_eq!(kept, ["/r_none", "/w"]);

        for value in [AttributeReply::Unsupported, AttributeReply::NoContent] {
            st.apply(Event::ValueChanged {
                server_id: id,
                path: "/w".into(),
                value,
            });
        }
        let t = st.entry(id).unwrap().tree.as_ref().unwrap();
        assert!(t.pending.contains_key("/w"), "no value read, still shown");
        assert!(t.unavailable.contains("/w"));
    }

    #[test]
    fn renames_and_removals_move_local_state() {
        let mut st = AppState::default();
        let s = server();
        let id = s.id;
        st.apply(Event::ServerFound(s));
        loaded(&mut st, id);
        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::HttpOnly,
        });
        let e = st.entry_mut(id).unwrap();
        e.listening.extend(["/a/x".to_owned(), "/b".to_owned()]);
        let t = e.tree.as_mut().unwrap();
        t.expanded.insert("/a".into());
        t.pending.insert("/a/x".into(), vec![OscType::Float(0.3)]);

        st.apply(Event::PathRenamed {
            server_id: id,
            old: "/a".into(),
            new: "/c".into(),
        });
        let e = st.entry(id).unwrap();
        let t = e.tree.as_ref().unwrap();
        assert!(e.listening.contains("/c/x") && !e.listening.contains("/a/x"));
        assert!(e.listening.contains("/b"));
        assert!(t.expanded.contains("/c") && !t.expanded.contains("/a"));
        assert_eq!(t.pending.get("/c/x"), Some(&vec![OscType::Float(0.3)]));

        st.apply(Event::PathRemoved {
            server_id: id,
            path: "/c".into(),
        });
        let e = st.entry(id).unwrap();
        let t = e.tree.as_ref().unwrap();
        assert_eq!(e.listening.iter().collect::<Vec<_>>(), [&"/b".to_owned()]);
        assert!(t.pending.is_empty() && t.expanded.is_empty());

        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::WebSocketLive,
        });
        assert_eq!(st.entry(id).unwrap().ws, WsState::Live);
        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::Disconnected,
        });
        let e = st.entry(id).unwrap();
        assert_eq!(e.ws, WsState::Absent);
        assert!(e.listening.is_empty());
    }

    #[test]
    fn send_blocked_reasons() {
        let mut st = AppState::default();
        let s = server();
        let id = s.id;
        st.apply(Event::ServerFound(s));
        assert_eq!(
            st.entry(id).unwrap().send_blocked().unwrap(),
            "not connected"
        );
        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::HttpOnly,
        });
        assert!(
            st.entry(id)
                .unwrap()
                .send_blocked()
                .unwrap()
                .contains("not received")
        );
        st.apply(Event::HostInfoUnavailable {
            server_id: id,
            reason: "404".into(),
        });
        assert!(
            st.entry(id)
                .unwrap()
                .send_blocked()
                .unwrap()
                .contains("404")
        );
        let (info, _) = HostInfo::parse(br#"{"OSC_PORT":1240,"OSC_TRANSPORT":"TCP"}"#).unwrap();
        let destination = info.osc_destination("studio.local", 2345);
        st.apply(Event::HostInfo {
            server_id: id,
            info,
            destination,
        });
        assert!(
            st.entry(id)
                .unwrap()
                .send_blocked()
                .unwrap()
                .contains("TCP")
        );
        st.apply(Event::TransportState {
            server_id: id,
            state: TransportState::Failed("refused".into()),
        });
        assert_eq!(
            st.entry(id).unwrap().send_blocked().unwrap(),
            "not connected: refused"
        );
    }

    #[test]
    fn manual_entries_are_not_duplicated() {
        let mut st = AppState::default();
        let mut a = server();
        a.origin = Origin::Manual;
        let mut b = a.clone();
        b.id = ServerId::next();
        b.host = "STUDIO.local".into();
        let ia = st.add_manual(a);
        let ib = st.add_manual(b);
        assert_eq!(ia, ib);
        assert_eq!(st.servers.len(), 1);
    }
}
