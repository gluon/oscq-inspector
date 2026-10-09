//! Application state, owned by the UI thread, changed only by applying network events and by the
//! user. No I/O here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use oscq_core::document::parse_value;
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
    /// Values sent and not yet confirmed by a refresh, drawn differently.
    pub pending: HashMap<String, Vec<OscType>>,
    /// Paths whose value query answered 204 or an empty object.
    pub unavailable: HashSet<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Bumped on every structural change, so derived views know to rebuild.
    pub version: u64,
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
                    let previous = e.tree.take();
                    let version = previous.as_ref().map_or(0, |t| t.version + 1);
                    let expanded = previous.map(|t| t.expanded).unwrap_or_default();
                    e.tree = Some(TreeState {
                        root: report.root,
                        raw,
                        diagnostics: report.diagnostics,
                        expanded,
                        version,
                        ..TreeState::default()
                    });
                }
            }
            Event::SubtreeLoaded {
                server_id,
                path,
                raw,
                report,
            } => {
                if let Some(t) = self.entry_mut(server_id).and_then(|e| e.tree.as_mut())
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
                    t.pending
                        .retain(|p, _| *p != path && !p.starts_with(&prefix));
                    t.unavailable
                        .retain(|p| *p != path && !p.starts_with(&prefix));
                    t.version += 1;
                }
            }
            Event::ValueChanged {
                server_id,
                path,
                value,
            } => {
                if let Some(t) = self.entry_mut(server_id).and_then(|e| e.tree.as_mut()) {
                    t.pending.remove(&path);
                    let node =
                        tree::find(&t.root, &path).and_then(|i| tree::node_at_mut(&mut t.root, &i));
                    match (value, node) {
                        (AttributeReply::Value(v), Some(node)) => {
                            node.value = Some(parse_value(v));
                            t.unavailable.remove(&path);
                        }
                        (AttributeReply::Unsupported | AttributeReply::NoContent, _) => {
                            t.unavailable.insert(path);
                        }
                        (AttributeReply::Value(_), None) => {}
                    }
                }
            }
            Event::TransportState { server_id, state } => {
                if let Some(e) = self.entry_mut(server_id) {
                    e.conn = match state {
                        TransportState::Connecting => ConnState::Connecting,
                        TransportState::HttpOnly => ConnState::Connected,
                        TransportState::Disconnected => ConnState::Disconnected,
                        TransportState::Failed(r) => ConnState::Failed(r),
                    };
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
