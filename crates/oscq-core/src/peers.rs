//! The `PEERS` and `CLIENT_NAME` extension (PRD 7.6): who is connected to a server, as that
//! server publishes it. An extension, not part of the proposal: a server declares it in
//! `HOST_INFO` `EXTENSIONS`, and nothing here is sent to or asked of a server that does not.
//!
//! The reply to `/?PEERS` is untrusted like every document: it is bounded before it is read, and
//! a reply that breaks a bound is an error, never a truncated list shown as the whole.

use std::net::{IpAddr, SocketAddr};

use serde_json::{Map, Value};

use crate::document::{Diagnostic, DiagnosticKind, DocumentError, parse_json};

/// The `EXTENSIONS` key by which a server declares that it answers `?PEERS`.
pub const EXT_PEERS: &str = "PEERS";
/// The `EXTENSIONS` key by which a server declares that it takes the `CLIENT_NAME` command.
pub const EXT_CLIENT_NAME: &str = "CLIENT_NAME";

/// A `?PEERS` reply above this size is refused before it is parsed.
pub const MAX_PEERS_BODY: usize = 4 << 20;
/// `CLIENTS` entries read at most. The servers themselves bound `SENDERS`; this client bounds
/// both, since it takes no server's word for it.
pub const MAX_CLIENTS: usize = 256;
/// `SENDERS` entries read at most.
pub const MAX_SENDERS: usize = 256;
/// Addresses in the `LISTEN` list of one client read at most.
pub const MAX_LISTEN: usize = 8192;
/// Bytes of a `NAME`, an `IP` or a listened address read at most.
pub const MAX_TEXT: usize = 1024;

/// One open WebSocket connection on a server, as the server describes it. A field the server
/// did not fill is `None`: it is absent, not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerClient {
    /// The address the server sees the client at, as the server wrote it.
    pub ip: String,
    /// The client's port on that connection. Ephemeral: it identifies a connection, not a device.
    pub port: u16,
    /// The name the client gave with `CLIENT_NAME`.
    pub name: Option<String>,
    /// From `START_OSC_STREAMING`: where the client receives OSC.
    pub osc_port: Option<u16>,
    /// From `START_OSC_STREAMING`: where the client sends OSC from.
    pub sender_port: Option<u16>,
    /// The addresses the client listens to. Not recursive: each is one method's full path.
    pub listen: Option<Vec<String>>,
    /// Values the server received from this client.
    pub packets: Option<u64>,
}

/// A UDP source the server matched to none of its WebSocket clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSender {
    pub ip: String,
    pub port: u16,
    pub packets: Option<u64>,
    /// Milliseconds between its last packet and the reply.
    pub last_ms: Option<u64>,
}

/// The reply to `/?PEERS`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Peers {
    pub clients: Vec<PeerClient>,
    pub senders: Vec<PeerSender>,
}

/// A `?PEERS` reply as the network service reports it: what the server published, and which of
/// its clients is this application's own WebSocket, when it is among them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeersReading {
    pub peers: Peers,
    /// Index in `peers.clients` of the connection this application holds on that server, found
    /// by the local address of its own socket: the one case where a client's port is known for
    /// certain. `None` when it holds no WebSocket there, or when the server sees it at another
    /// address, behind a translation.
    pub this_client: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum PeersError {
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("the reply holds neither CLIENTS nor SENDERS")]
    NotPeers,
    #[error("{0} is not a list")]
    NotAList(&'static str),
    #[error("{list} holds {received} entries, more than the {limit} this client reads")]
    TooMany {
        list: &'static str,
        limit: usize,
        received: usize,
    },
    #[error("{list} entry {index} is not an object with an IP and a PORT")]
    InvalidEntry { list: &'static str, index: usize },
    #[error("{list} entry {index}: {field} is longer than {MAX_TEXT} bytes")]
    TooLong {
        list: &'static str,
        index: usize,
        field: &'static str,
    },
}

impl Peers {
    /// Parses a `?PEERS` reply: an object holding the lists `CLIENTS` and `SENDERS`, at its top
    /// level or under a `PEERS` key, the form in which an attribute query answers (PRD 7.3). A
    /// reply holding neither list is an error, not an empty set of peers: nothing was published.
    /// An optional field of the wrong JSON type is left absent and returned as a diagnostic
    /// against the path `/`.
    pub fn parse(bytes: &[u8]) -> Result<(Peers, Vec<Diagnostic>), PeersError> {
        let Value::Object(mut obj) = parse_json(bytes)? else {
            return Err(DocumentError::NotAnObject.into());
        };
        if !obj.contains_key("CLIENTS")
            && !obj.contains_key("SENDERS")
            && let Some(Value::Object(inner)) = obj.remove(EXT_PEERS)
        {
            obj = inner;
        }
        if !obj.contains_key("CLIENTS") && !obj.contains_key("SENDERS") {
            return Err(PeersError::NotPeers);
        }
        let mut diags = Vec::new();
        let mut peers = Peers::default();
        for (index, entry) in list(&mut obj, "CLIENTS", MAX_CLIENTS)?
            .into_iter()
            .enumerate()
        {
            let mut e = Entry::new("CLIENTS", index, entry)?;
            peers.clients.push(PeerClient {
                ip: e.ip()?,
                port: e.required_port()?,
                name: e.text("NAME", &mut diags)?,
                osc_port: e.port("OSC_PORT", &mut diags),
                sender_port: e.port("SENDER_PORT", &mut diags),
                listen: e.listen(&mut diags)?,
                packets: e.count("PACKETS", &mut diags),
            });
        }
        for (index, entry) in list(&mut obj, "SENDERS", MAX_SENDERS)?
            .into_iter()
            .enumerate()
        {
            let mut e = Entry::new("SENDERS", index, entry)?;
            peers.senders.push(PeerSender {
                ip: e.ip()?,
                port: e.required_port()?,
                packets: e.count("PACKETS", &mut diags),
                last_ms: e.count("LAST_MS", &mut diags),
            });
        }
        Ok((peers, diags))
    }

    /// The client that is the connection whose own end is `local`: the same port, at the same
    /// address. A port alone would match a stranger at another address.
    pub fn client_at(&self, local: SocketAddr) -> Option<usize> {
        let ip = canonical(local.ip());
        self.clients
            .iter()
            .position(|c| c.port == local.port() && canonical_ip(&c.ip) == Some(ip))
    }
}

/// The entries of one list of the reply. Absent is empty: the other list was published.
fn list(
    obj: &mut Map<String, Value>,
    key: &'static str,
    limit: usize,
) -> Result<Vec<Value>, PeersError> {
    match obj.remove(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(entries)) if entries.len() > limit => Err(PeersError::TooMany {
            list: key,
            limit,
            received: entries.len(),
        }),
        Some(Value::Array(entries)) => Ok(entries),
        Some(_) => Err(PeersError::NotAList(key)),
    }
}

/// One entry of a list, read field by field.
struct Entry {
    list: &'static str,
    index: usize,
    obj: Map<String, Value>,
}

impl Entry {
    fn new(list: &'static str, index: usize, value: Value) -> Result<Self, PeersError> {
        match value {
            Value::Object(obj) => Ok(Self { list, index, obj }),
            _ => Err(PeersError::InvalidEntry { list, index }),
        }
    }

    fn invalid(&self) -> PeersError {
        PeersError::InvalidEntry {
            list: self.list,
            index: self.index,
        }
    }

    fn bounded(&self, field: &'static str, text: String) -> Result<String, PeersError> {
        if text.len() > MAX_TEXT {
            return Err(PeersError::TooLong {
                list: self.list,
                index: self.index,
                field,
            });
        }
        Ok(text)
    }

    fn diagnostic(&self, field: &str, diags: &mut Vec<Diagnostic>) {
        diags.push(Diagnostic {
            path: "/".to_owned(),
            kind: DiagnosticKind::InvalidAttribute(format!(
                "{}[{}].{field}",
                self.list, self.index
            )),
        });
    }

    /// `IP`, required: without it the entry names no one.
    fn ip(&mut self) -> Result<String, PeersError> {
        match self.obj.remove("IP") {
            Some(Value::String(ip)) if !ip.trim().is_empty() => self.bounded("IP", ip),
            _ => Err(self.invalid()),
        }
    }

    /// `PORT`, required.
    fn required_port(&mut self) -> Result<u16, PeersError> {
        self.obj
            .remove("PORT")
            .as_ref()
            .and_then(port)
            .ok_or_else(|| self.invalid())
    }

    fn text(
        &mut self,
        field: &'static str,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<Option<String>, PeersError> {
        match self.obj.remove(field) {
            None => Ok(None),
            Some(Value::String(s)) => self.bounded(field, s).map(Some),
            Some(_) => {
                self.diagnostic(field, diags);
                Ok(None)
            }
        }
    }

    fn port(&mut self, field: &'static str, diags: &mut Vec<Diagnostic>) -> Option<u16> {
        let v = self.obj.remove(field)?;
        let p = port(&v);
        if p.is_none() {
            self.diagnostic(field, diags);
        }
        p
    }

    fn count(&mut self, field: &'static str, diags: &mut Vec<Diagnostic>) -> Option<u64> {
        let v = self.obj.remove(field)?;
        let n = v.as_u64();
        if n.is_none() {
            self.diagnostic(field, diags);
        }
        n
    }

    /// `LISTEN`: the strings of the list, in order. An entry that is not a string is left out
    /// and reported once.
    fn listen(&mut self, diags: &mut Vec<Diagnostic>) -> Result<Option<Vec<String>>, PeersError> {
        let entries = match self.obj.remove("LISTEN") {
            None => return Ok(None),
            Some(Value::Array(entries)) => entries,
            Some(_) => {
                self.diagnostic("LISTEN", diags);
                return Ok(None);
            }
        };
        if entries.len() > MAX_LISTEN {
            return Err(PeersError::TooMany {
                list: "LISTEN",
                limit: MAX_LISTEN,
                received: entries.len(),
            });
        }
        let mut paths = Vec::with_capacity(entries.len());
        let mut skipped = false;
        for entry in entries {
            match entry {
                Value::String(path) => paths.push(self.bounded("LISTEN", path)?),
                _ => skipped = true,
            }
        }
        if skipped {
            self.diagnostic("LISTEN", diags);
        }
        Ok(Some(paths))
    }
}

/// A port given as a JSON number or as a numeric string, as `HOST_INFO` ports are read.
fn port(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
        Value::String(s) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
    .filter(|p| *p != 0)
}

/// An address in one form: an IPv4 address mapped into IPv6, as a server listening on both
/// reports its IPv4 clients, is that IPv4 address.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// The `IP` of an entry as an address, when it is one: brackets and an IPv6 zone dropped, and
/// the same form for an IPv4 address however it was written. `None` when the text is not an
/// address; it is then compared with nothing.
pub fn canonical_ip(text: &str) -> Option<IpAddr> {
    let text = text.trim().trim_start_matches('[').trim_end_matches(']');
    let text = text.split('%').next().unwrap_or(text);
    text.parse::<IpAddr>().ok().map(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Result<(Peers, Vec<Diagnostic>), PeersError> {
        Peers::parse(v.to_string().as_bytes())
    }

    /// Every field of both lists, read as published; a field the server left out stays absent.
    #[test]
    fn both_lists_are_read_and_an_absent_field_stays_absent() {
        let (peers, diags) = parse(json!({
            "CLIENTS": [
                {"IP": "10.0.0.5", "PORT": 51234, "NAME": "Max", "OSC_PORT": 9000,
                 "SENDER_PORT": "9001", "LISTEN": ["/a", "/b/c"], "PACKETS": 12},
                {"IP": "::1", "PORT": 40000}
            ],
            "SENDERS": [{"IP": "10.0.0.9", "PORT": 8000, "PACKETS": 3, "LAST_MS": 250}]
        }))
        .unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            peers.clients,
            [
                PeerClient {
                    ip: "10.0.0.5".into(),
                    port: 51234,
                    name: Some("Max".into()),
                    osc_port: Some(9000),
                    sender_port: Some(9001),
                    listen: Some(vec!["/a".into(), "/b/c".into()]),
                    packets: Some(12),
                },
                PeerClient {
                    ip: "::1".into(),
                    port: 40000,
                    name: None,
                    osc_port: None,
                    sender_port: None,
                    listen: None,
                    packets: None,
                },
            ]
        );
        assert_eq!(
            peers.senders,
            [PeerSender {
                ip: "10.0.0.9".into(),
                port: 8000,
                packets: Some(3),
                last_ms: Some(250),
            }]
        );
    }

    /// The two lists under a `PEERS` key, as an attribute query answers, read the same; one list
    /// alone is a reply; an object holding neither is not a reply, and is not read as no peers.
    #[test]
    fn the_lists_are_read_at_the_top_level_or_under_peers_and_never_assumed() {
        let lists = json!({"CLIENTS": [{"IP": "10.0.0.5", "PORT": 1}], "SENDERS": []});
        let top = parse(lists.clone()).unwrap().0;
        let wrapped = parse(json!({ "PEERS": lists })).unwrap().0;
        assert_eq!(top, wrapped);
        assert_eq!(top.clients.len(), 1);
        let only_senders = parse(json!({"SENDERS": []})).unwrap().0;
        assert_eq!(only_senders, Peers::default());
        for not_peers in [
            json!({}),
            json!({"PEERS": true}),
            json!({"PEERS": {}}),
            json!({"FULL_PATH": "/", "CONTENTS": {}}),
        ] {
            assert!(
                matches!(parse(not_peers.clone()), Err(PeersError::NotPeers)),
                "{not_peers}"
            );
        }
        assert!(matches!(
            parse(json!({"CLIENTS": {"IP": "1.2.3.4"}})),
            Err(PeersError::NotAList("CLIENTS"))
        ));
        assert!(matches!(
            parse(json!({"CLIENTS": [], "SENDERS": 3})),
            Err(PeersError::NotAList("SENDERS"))
        ));
    }

    /// Law 5: each list is bounded, and a reply over a bound is an error, not a cut list.
    #[test]
    fn a_list_over_its_bound_is_an_error() {
        let client = |i: usize| json!({"IP": "10.0.0.5", "PORT": 1 + i % 60000});
        let at_limit: Vec<Value> = (0..MAX_CLIENTS).map(client).collect();
        assert_eq!(
            parse(json!({ "CLIENTS": at_limit }))
                .unwrap()
                .0
                .clients
                .len(),
            MAX_CLIENTS
        );
        let over: Vec<Value> = (0..=MAX_CLIENTS).map(client).collect();
        assert!(matches!(
            parse(json!({ "CLIENTS": over })),
            Err(PeersError::TooMany { list: "CLIENTS", limit: MAX_CLIENTS, received })
                if received == MAX_CLIENTS + 1
        ));
        let senders: Vec<Value> = (0..=MAX_SENDERS).map(client).collect();
        assert!(matches!(
            parse(json!({ "SENDERS": senders })),
            Err(PeersError::TooMany {
                list: "SENDERS",
                ..
            })
        ));
        let listen = |n: usize| -> Vec<String> { (0..n).map(|i| format!("/p{i}")).collect() };
        let one = |n| json!({"CLIENTS": [{"IP": "10.0.0.5", "PORT": 1, "LISTEN": listen(n)}]});
        assert_eq!(
            parse(one(MAX_LISTEN)).unwrap().0.clients[0]
                .listen
                .as_ref()
                .map(Vec::len),
            Some(MAX_LISTEN)
        );
        assert!(matches!(
            parse(one(MAX_LISTEN + 1)),
            Err(PeersError::TooMany { list: "LISTEN", .. })
        ));
    }

    /// Law 5: a name, an address or a listened path of hostile length is an error.
    #[test]
    fn a_text_over_its_bound_is_an_error() {
        let long = "x".repeat(MAX_TEXT + 1);
        let fits = "x".repeat(MAX_TEXT);
        let with = |field: &str, text: &str| {
            let mut c = json!({"IP": "10.0.0.5", "PORT": 1});
            c[field] = if field == "LISTEN" {
                json!([text])
            } else {
                json!(text)
            };
            parse(json!({ "CLIENTS": [c] }))
        };
        for field in ["NAME", "IP", "LISTEN"] {
            assert!(with(field, &fits).is_ok(), "{field} at the bound");
            assert!(
                matches!(
                    with(field, &long),
                    Err(PeersError::TooLong { list: "CLIENTS", index: 0, field: f }) if f == field
                ),
                "{field} over the bound"
            );
        }
    }

    /// An entry that names no one makes the reply unreadable: dropping it would hide a
    /// connection the server reported.
    #[test]
    fn an_entry_without_an_ip_and_a_port_is_an_error() {
        for (entry, list) in [
            (json!("10.0.0.5"), "CLIENTS"),
            (json!({"PORT": 1}), "CLIENTS"),
            (json!({"IP": "10.0.0.5"}), "CLIENTS"),
            (json!({"IP": "", "PORT": 1}), "CLIENTS"),
            (json!({"IP": 5, "PORT": 1}), "CLIENTS"),
            (json!({"IP": "10.0.0.5", "PORT": 0}), "SENDERS"),
            (json!({"IP": "10.0.0.5", "PORT": 70000}), "SENDERS"),
            (json!(null), "SENDERS"),
        ] {
            let ok = json!({"IP": "10.0.0.1", "PORT": 1});
            let reply = json!({ list: [ok, entry] });
            assert!(
                matches!(
                    parse(reply.clone()),
                    Err(PeersError::InvalidEntry { list: l, index: 1 }) if l == list
                ),
                "{reply}"
            );
        }
    }

    /// An optional field of the wrong type is absent, with a diagnostic naming it; it is not
    /// read as a zero.
    #[test]
    fn an_optional_field_of_the_wrong_type_is_absent_and_reported() {
        let (peers, diags) = parse(json!({
            "CLIENTS": [{"IP": "10.0.0.5", "PORT": 1, "NAME": 7, "OSC_PORT": "x",
                "SENDER_PORT": -1, "LISTEN": "/a", "PACKETS": 1.5}],
            "SENDERS": [{"IP": "10.0.0.5", "PORT": 1, "PACKETS": "many", "LAST_MS": -3}]
        }))
        .unwrap();
        let c = &peers.clients[0];
        assert_eq!(
            (&c.name, c.osc_port, c.sender_port, &c.listen, c.packets),
            (&None, None, None, &None, None)
        );
        let s = &peers.senders[0];
        assert_eq!((s.packets, s.last_ms), (None, None));
        let named: Vec<String> = diags
            .iter()
            .map(|d| match &d.kind {
                DiagnosticKind::InvalidAttribute(k) => k.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            named,
            [
                "CLIENTS[0].NAME",
                "CLIENTS[0].OSC_PORT",
                "CLIENTS[0].SENDER_PORT",
                "CLIENTS[0].LISTEN",
                "CLIENTS[0].PACKETS",
                "SENDERS[0].PACKETS",
                "SENDERS[0].LAST_MS",
            ]
        );
        let (peers, diags) = parse(json!({
            "CLIENTS": [{"IP": "10.0.0.5", "PORT": 1, "LISTEN": ["/a", 3, null, "/b"]}]
        }))
        .unwrap();
        assert_eq!(
            peers.clients[0].listen,
            Some(vec!["/a".to_owned(), "/b".to_owned()])
        );
        assert_eq!(diags.len(), 1, "the entries left out are reported once");
    }

    /// Law 5: what is not JSON, not an object, or nested to exhaust a recursive parser, is an
    /// error. The deep case runs on a small stack.
    #[test]
    fn a_hostile_document_is_an_error_never_a_panic() {
        for body in [
            &b"not json"[..],
            b"",
            b"[]",
            b"3",
            b"\"CLIENTS\"",
            b"{\"CLIENTS\":",
        ] {
            assert!(
                matches!(Peers::parse(body), Err(PeersError::Document(_))),
                "{:?}",
                String::from_utf8_lossy(body)
            );
        }
        let deep = format!(
            "{{\"CLIENTS\":{}{}}}",
            "[".repeat(200_000),
            "]".repeat(200_000)
        );
        let r = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || Peers::parse(deep.as_bytes()).map(|_| ()))
            .unwrap()
            .join()
            .unwrap();
        assert!(matches!(
            r,
            Err(PeersError::Document(DocumentError::TooDeep))
        ));
    }

    /// This application's own connection is found by the address of its own socket, both parts:
    /// the port alone is someone else's at another address, and a mapped IPv4 address is that
    /// IPv4 address.
    #[test]
    fn the_own_connection_is_found_by_its_socket_address() {
        let (peers, _) = parse(json!({"CLIENTS": [
            {"IP": "10.0.0.7", "PORT": 50000},
            {"IP": "::ffff:10.0.0.5", "PORT": 50000},
            {"IP": "10.0.0.5", "PORT": 50001},
            {"IP": "[fe80::1%en0]", "PORT": 50002},
            {"IP": "studio.local", "PORT": 50003}
        ]}))
        .unwrap();
        let at = |s: &str| peers.client_at(s.parse().unwrap());
        assert_eq!(at("10.0.0.5:50000"), Some(1));
        assert_eq!(at("10.0.0.5:50001"), Some(2));
        assert_eq!(at("[fe80::1]:50002"), Some(3));
        assert_eq!(at("10.0.0.6:50000"), None, "the port alone names no one");
        assert_eq!(at("10.0.0.5:50003"), None);
        assert_eq!(canonical_ip("studio.local"), None);
        assert_eq!(
            canonical_ip(" ::ffff:127.0.0.1 "),
            Some("127.0.0.1".parse().unwrap())
        );
    }
}
