//! Application state, owned by the UI thread, changed only by applying network events and by the
//! user. No I/O here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use oscq_core::document::parse_value;
use oscq_core::ws::{is_within, renamed};
use oscq_core::{
    AttributeReply, Diagnostic, DiscoveredServer, Event, HostInfo, Node, Origin, OscDestination,
    SendBlocked, ServerId, TransportState, UserFacingError,
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
}

impl TreeState {
    fn reindex(&mut self) {
        self.paths = tree::index_paths(&self.root);
    }

    pub fn node(&self, path: &str) -> Option<&Node> {
        self.paths
            .get(path)
            .and_then(|i| tree::node_at(&self.root, i))
    }
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

    /// Why nothing can be sent to this server, if anything prevents it.
    pub fn send_blocked(&self) -> Option<String> {
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
                }
            }
            Event::HostInfoUnavailable { server_id, reason } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.host_info = None;
                    e.destination = None;
                    e.host_info_error = Some(reason);
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
                    // A new or closed connection starts with no WebSocket and no listens.
                    if !matches!(e.conn, ConnState::Connected) {
                        e.ws = WsState::Absent;
                        e.listening.clear();
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
