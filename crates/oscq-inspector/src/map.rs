//! The map of PRD 9.10: the setup as each server publishes it. A model first, nodes and edges
//! built from the readings of `?PEERS` and from nothing else, then its places, then its drawing.
//! No edge is inferred: not from traffic, not from mDNS, not from a port number.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use oscq_core::peers::{canonical, canonical_ip};
use oscq_core::{PeerClient, ServerId, ServiceKind};

use crate::state::{
    AppState, ConnState, PeersSnapshot, PeersView, ServerEntry, client_label, ip_port, short_reason,
};
use crate::style::{self, GRID};

/// The least size, in points on screen, at which a text of the map is drawn: below it, a
/// node is drawn without its text, whole on hover. egui's own smallest text style, `Small`,
/// is 9 points (egui 0.36, `style.rs`, the default text styles).
pub const READABLE: f32 = 9.0;

/// A reading older than this many poll intervals is marked stale (PRD 9.10).
pub const STALE_AFTER_INTERVALS: u32 = 3;

/// The colours of the edges, each kind its own, so that an edge is told from everything else
/// the window draws. Each kind has its own line too: colour alone is not what tells them apart.
pub const EDGE_CLIENT: Color32 = Color32::from_rgb(70, 150, 230);
pub const EDGE_SENDER: Color32 = Color32::from_rgb(205, 140, 40);
pub const EDGE_AMBIGUOUS: Color32 = Color32::from_rgb(200, 90, 200);
pub const EDGE_STALE: Color32 = Color32::from_rgb(128, 128, 132);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A discovered or manually entered server, whatever its state.
    Server(ServerId),
    /// This application, as the servers it is connected to list it.
    ThisApp,
    /// A WebSocket client that is no known server.
    Client,
    /// A UDP source that is no known server.
    Sender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    /// A failure, or a reading that is no longer current.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    /// Who the node is, the same from one reading to the next: what a position set by hand is
    /// kept under. See [`Node::kept`].
    pub key: String,
    pub title: String,
    /// What is known of it, one statement a line, and nothing more.
    pub lines: Vec<(String, Tone)>,
    /// What a line says in short, whole, shown on hover.
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// One `CLIENTS` entry: an open WebSocket.
    Client,
    /// One `SENDERS` entry: a UDP source that holds no WebSocket, a weaker fact.
    Sender,
}

/// How the start of an edge was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The entry is this application's own socket.
    This,
    /// Its `NAME` is the name of exactly one known server.
    Name,
    /// No name to go by: its `IP` is an address of exactly one known server.
    Address,
    /// Neither: the entry is a node of its own.
    Own,
    /// Its `NAME` is the name of several known servers: not matched, and said so.
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Index of the node the edge starts from: the peer.
    pub from: usize,
    /// Index of the server the peer is connected to. Equal to `from` for a server connected to
    /// itself: a loop.
    pub to: usize,
    pub kind: EdgeKind,
    pub how: How,
    /// The reading it comes from is no longer current.
    pub stale: bool,
    /// What the edge carries: who, when the node does not say it, how it was matched when that
    /// is not by name, and the packets the server counted.
    pub label: String,
    /// The rest of the entry, on hover.
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Where an edge starts, before the nodes have their indices. The order is the order of the
/// nodes, so that a map keeps its places from one reading to the next.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Server(usize),
    This,
    /// A client under a name, at an address: one node for its connections to several servers.
    Named {
        name: String,
        ip: String,
    },
    /// A client that gave no name: known by its address and its port, one node a connection.
    Unnamed {
        ip: String,
        port: u16,
    },
    Sender {
        ip: String,
        port: u16,
    },
}

/// The names a server goes by: the one it was found or entered under, and the one its
/// `HOST_INFO` gives.
fn names(e: &ServerEntry) -> impl Iterator<Item = &str> {
    [
        Some(e.server.name.as_str()),
        e.host_info.as_ref().and_then(|h| h.name.as_deref()),
    ]
    .into_iter()
    .flatten()
}

fn has_address(e: &ServerEntry, ip: IpAddr) -> bool {
    e.server.addresses.iter().any(|a| canonical(a.ip) == ip)
        || canonical_ip(&e.server.host) == Some(ip)
}

/// An address as a node is keyed and labelled by it: in one form when it is an address.
fn address_key(ip: &str) -> String {
    canonical_ip(ip).map_or_else(|| ip.trim().to_owned(), |a| a.to_string())
}

fn count(n: u64, one: &str, many: &str) -> String {
    match n {
        0 => format!("no {one}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// Whether a reading is no longer current: older than three poll intervals, or of a server
/// that is not connected, which nothing can read again.
pub fn is_stale(
    e: &ServerEntry,
    snapshot: &PeersSnapshot,
    now: Instant,
    interval: Duration,
) -> bool {
    e.conn != ConnState::Connected
        || now.saturating_duration_since(snapshot.at) > interval * STALE_AFTER_INTERVALS
}

fn connection_line(e: &ServerEntry) -> (String, Tone) {
    if e.server.kind == ServiceKind::OscOnly {
        return ("OSC only, no query support".to_owned(), Tone::Plain);
    }
    match &e.conn {
        ConnState::Idle | ConnState::Disconnected => (
            "not connected: select it to read it".to_owned(),
            Tone::Plain,
        ),
        ConnState::Connecting => ("connecting".to_owned(), Tone::Plain),
        ConnState::Failed(reason) => (format!("not connected: {reason}"), Tone::Warn),
        ConnState::Connected => ("connected".to_owned(), Tone::Plain),
    }
}

/// A server's node: what is known of it and nothing more (PRD 9.10).
fn server_node(e: &ServerEntry, now: Instant, interval: Duration) -> Node {
    let mut lines = vec![(e.key(), Tone::Plain), connection_line(e)];
    let mut detail = Vec::new();
    if e.server.kind == ServiceKind::OscQuery {
        match e.peers_view() {
            PeersView::NotConnected => {}
            PeersView::HostInfoUnknown => lines.push((
                "HOST_INFO not read: its peers are not known".to_owned(),
                Tone::Plain,
            )),
            PeersView::NotPublished => {
                lines.push(("does not publish its peers".to_owned(), Tone::Plain))
            }
            PeersView::NotRead => lines.push(("peers not read yet".to_owned(), Tone::Plain)),
            PeersView::Failed(reason) => {
                lines.push((
                    format!("peers not read: {}", short_reason(reason)),
                    Tone::Warn,
                ));
                detail.push(format!("peers not read: {reason}"));
            }
            PeersView::Read { snapshot, failure } => {
                lines.push((
                    format!(
                        "{}, {}",
                        count(snapshot.peers.clients.len() as u64, "client", "clients"),
                        count(
                            snapshot.peers.senders.len() as u64,
                            "UDP source",
                            "UDP sources"
                        ),
                    ),
                    Tone::Plain,
                ));
                let age = now.saturating_duration_since(snapshot.at).as_secs();
                if is_stale(e, snapshot, now, interval) {
                    lines.push((format!("stale: read {age} s ago"), Tone::Warn));
                } else {
                    lines.push((format!("read {age} s ago"), Tone::Plain));
                }
                if let Some(reason) = failure {
                    lines.push((
                        format!("not read since: {}", short_reason(reason)),
                        Tone::Warn,
                    ));
                    detail.push(format!("not read since: {reason}"));
                }
            }
        }
    }
    Node {
        kind: NodeKind::Server(e.server.id),
        key: format!("server {}", e.key()),
        // What the server declares wins (CLAUDE.md law 2): its `HOST_INFO` name, however it came
        // into the list; the name it was found or entered under failing that. Its address is the
        // first line, and on hover.
        title: e
            .host_info
            .as_ref()
            .and_then(|h| h.name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| e.server.name.clone()),
        lines,
        detail,
    }
}

impl Node {
    /// Whether a position set by hand for this node is kept across launches: its identity
    /// outlives the connection. A server by its address, this application, a client by the
    /// name it gave and its address. An unnamed client and a UDP source are known by a port
    /// the system chose for one connection: their positions last the session.
    pub fn kept(&self) -> bool {
        match self.kind {
            NodeKind::Server(_) | NodeKind::ThisApp => true,
            NodeKind::Client => self.key.starts_with("client "),
            NodeKind::Sender => false,
        }
    }
}

/// Where the edge of a `CLIENTS` entry starts (PRD 7.6): on `NAME` first, then, a name that is
/// no known server's failing that, on `IP` alone, never on `PORT`. A client that gave no name is
/// a node of its own, labelled by its address and port, and is never drawn as a server that
/// happens to share its address (PRD 9.10). A name or an address that several known servers
/// share matches none of them: the application does not guess.
fn client_origin(c: &PeerClient, servers: &[ServerEntry]) -> (Origin, How, Option<String>) {
    let own = || match &c.name {
        Some(name) => Origin::Named {
            name: name.clone(),
            ip: address_key(&c.ip),
        },
        None => Origin::Unnamed {
            ip: address_key(&c.ip),
            port: c.port,
        },
    };
    let Some(name) = &c.name else {
        return (own(), How::Own, None);
    };
    let named: Vec<usize> = (0..servers.len())
        .filter(|i| names(&servers[*i]).any(|n| n == name))
        .collect();
    match named[..] {
        [one] => return (Origin::Server(one), How::Name, None),
        [] => {}
        _ => {
            let note = format!("name of {} known servers: not matched", named.len());
            return (own(), How::Ambiguous, Some(note));
        }
    }
    let at: Vec<usize> = canonical_ip(&c.ip).map_or_else(Vec::new, |ip| {
        (0..servers.len())
            .filter(|i| has_address(&servers[*i], ip))
            .collect()
    });
    match at[..] {
        [one] => (Origin::Server(one), How::Address, None),
        [] => (own(), How::Own, None),
        _ => {
            let note = format!("at the address of {} known servers", at.len());
            (own(), How::Own, Some(note))
        }
    }
}

fn client_detail(c: &PeerClient) -> Vec<String> {
    let mut detail = vec![ip_port(&c.ip, c.port)];
    if let Some(p) = c.osc_port {
        detail.push(format!("receives OSC on port {p}"));
    }
    if let Some(p) = c.sender_port {
        detail.push(format!("sends OSC from port {p}"));
    }
    if let Some(listen) = &c.listen {
        const SHOWN: usize = 12;
        detail.push(format!(
            "listens to {}",
            count(listen.len() as u64, "address", "addresses")
        ));
        detail.extend(listen.iter().take(SHOWN).map(|p| format!("  {p}")));
        if listen.len() > SHOWN {
            detail.push(format!("  and {} more", listen.len() - SHOWN));
        }
    }
    detail
}

/// The map of everything published: one node per known server, one per peer that is no known
/// server, one edge per `CLIENTS` entry and one per `SENDERS` entry (PRD 9.10).
pub fn build(state: &AppState, now: Instant, interval: Duration, app_name: &str) -> Model {
    let servers = &state.servers;
    struct Draft {
        origin: Origin,
        to: usize,
        edge: Edge,
    }
    let mut drafts: Vec<Draft> = Vec::new();
    let mut notes: BTreeMap<Origin, Vec<String>> = BTreeMap::new();
    for (si, e) in servers.iter().enumerate() {
        let PeersView::Read { snapshot, .. } = e.peers_view() else {
            continue;
        };
        let stale = is_stale(e, snapshot, now, interval);
        for (ci, c) in snapshot.peers.clients.iter().enumerate() {
            let (origin, how, note) = if snapshot.this_client == Some(ci) {
                (Origin::This, How::This, None)
            } else {
                client_origin(c, servers)
            };
            let mut label = Vec::new();
            match how {
                // The node the edge starts from is not the client's own: the edge says who.
                How::Address => label.push(format!("{}, by address", client_label(c))),
                How::Ambiguous => label.push(format!(
                    "ambiguous: {}",
                    note.as_deref().unwrap_or("not matched")
                )),
                How::This | How::Name | How::Own => {}
            }
            if let Some(n) = c.packets {
                label.push(count(n, "packet", "packets"));
            }
            notes
                .entry(origin.clone())
                .or_default()
                .extend(note.filter(|n| !n.is_empty()));
            drafts.push(Draft {
                origin,
                to: si,
                edge: Edge {
                    from: 0,
                    to: si,
                    kind: EdgeKind::Client,
                    how,
                    stale,
                    label: label.join(", "),
                    detail: client_detail(c),
                },
            });
        }
        for s in &snapshot.peers.senders {
            let at: Vec<usize> = canonical_ip(&s.ip).map_or_else(Vec::new, |ip| {
                (0..servers.len())
                    .filter(|i| has_address(&servers[*i], ip))
                    .collect()
            });
            let (origin, how) = match at[..] {
                [one] => (Origin::Server(one), How::Address),
                _ => (
                    Origin::Sender {
                        ip: address_key(&s.ip),
                        port: s.port,
                    },
                    How::Own,
                ),
            };
            let mut label = vec![match how {
                How::Address => format!("UDP from port {}, by address", s.port),
                _ => "UDP".to_owned(),
            }];
            if let Some(n) = s.packets {
                label.push(count(n, "packet", "packets"));
            }
            let mut detail = vec![ip_port(&s.ip, s.port)];
            if let Some(ms) = s.last_ms {
                detail.push(format!("last packet {ms} ms before the reading"));
            }
            notes.entry(origin.clone()).or_default();
            drafts.push(Draft {
                origin,
                to: si,
                edge: Edge {
                    from: 0,
                    to: si,
                    kind: EdgeKind::Sender,
                    how,
                    stale,
                    label: label.join(", "),
                    detail,
                },
            });
        }
    }

    let mut nodes: Vec<Node> = servers
        .iter()
        .map(|e| server_node(e, now, interval))
        .collect();
    // The other nodes, in the order of their keys: the same from one reading to the next.
    let mut index: BTreeMap<Origin, usize> = BTreeMap::new();
    for (origin, mut notes) in notes {
        notes.sort();
        notes.dedup();
        let tone = |n: Vec<String>| n.into_iter().map(|n| (n, Tone::Plain));
        let node = match &origin {
            Origin::Server(i) => {
                index.insert(origin.clone(), *i);
                continue;
            }
            Origin::This => Node {
                kind: NodeKind::ThisApp,
                key: "this application".to_owned(),
                title: app_name.to_owned(),
                lines: vec![("this application".to_owned(), Tone::Plain)],
                detail: Vec::new(),
            },
            Origin::Named { name, ip } => Node {
                kind: NodeKind::Client,
                key: format!("client {name}@{ip}"),
                title: name.clone(),
                lines: std::iter::once((ip.clone(), Tone::Plain))
                    .chain(tone(notes))
                    .collect(),
                detail: Vec::new(),
            },
            Origin::Unnamed { ip, port } => Node {
                kind: NodeKind::Client,
                key: format!("unnamed {}", ip_port(ip, *port)),
                title: "unnamed client".to_owned(),
                lines: std::iter::once((ip_port(ip, *port), Tone::Plain))
                    .chain(tone(notes))
                    .collect(),
                detail: Vec::new(),
            },
            Origin::Sender { ip, port } => Node {
                kind: NodeKind::Sender,
                key: format!("udp {}", ip_port(ip, *port)),
                title: "UDP source".to_owned(),
                lines: vec![(ip_port(ip, *port), Tone::Plain)],
                detail: Vec::new(),
            },
        };
        index.insert(origin, nodes.len());
        nodes.push(node);
    }
    let edges = drafts
        .into_iter()
        .filter_map(|d| {
            let from = *index.get(&d.origin)?;
            Some(Edge {
                from,
                to: d.to,
                ..d.edge
            })
        })
        .collect();
    Model { nodes, edges }
}

// ---------------------------------------------------------------------------- places

pub(crate) const NODE_W: f32 = 52.0 * GRID;
const TITLE_H: f32 = 5.0 * GRID;
const LINE_H: f32 = 4.0 * GRID;
const PAD: f32 = 2.0 * GRID;
/// Room a node takes along its ring, and the least distance between the two rings.
const SLOT: f32 = NODE_W + 16.0 * GRID;
const RING_GAP: f32 = 56.0 * GRID;
/// The least stretch of the rings across and down, the ellipse the map had before its shape
/// followed the canvas: what an edge between the two rings needs. Across, the rings at their
/// sides are `1.25 * RING_GAP` apart, 280 points: a node, 208, and 72 of edge. Down, at their
/// top, `0.75 * RING_GAP`, 168 points: half of two nodes and about 90 of edge. At stretch 1
/// across, an edge was 16 points long, four dashes, which
/// `a_name_two_servers_share_draws_an_ambiguous_edge_that_says_so` caught.
const STRETCH_MIN: Vec2 = Vec2::new(1.25, 0.75);
const MARGIN: f32 = 12.0 * GRID;

fn node_size(node: &Node) -> Vec2 {
    Vec2::new(
        NODE_W,
        2.0 * PAD + TITLE_H + node.lines.len() as f32 * LINE_H,
    )
}

/// The places of the nodes: computed, never authored (PRD 9.10). The servers on a ring, in
/// their order; every other node on a ring around it, towards the servers it is connected to.
/// The rings are ellipses shaped to `aspect`, the width over the height of the canvas, so that
/// the map fits it on both sides and not on one alone; each as small as the nodes allow, so that
/// it is drawn as large as the canvas allows. The same model and aspect give the same places.
fn ring_layout(model: &Model, aspect: f32) -> Vec<Rect> {
    let servers: Vec<usize> = (0..model.nodes.len())
        .filter(|i| matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let others: Vec<usize> = (0..model.nodes.len())
        .filter(|i| !matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let tau = std::f32::consts::TAU;
    let inner = if servers.len() <= 1 {
        0.0
    } else {
        (servers.len() as f32 * SLOT / tau).max(40.0 * GRID)
    };
    let outer = if others.is_empty() {
        inner
    } else {
        (inner + RING_GAP).max(others.len() as f32 * SLOT / tau)
    };
    let tallest = model
        .nodes
        .iter()
        .map(|n| node_size(n).y)
        .fold(0.0, f32::max);
    let stretch = stretch_for(aspect, outer, Vec2::new(NODE_W, tallest));
    let centres = ring_centres(model, inner, outer, stretch);
    let rects: Vec<Rect> = centres
        .iter()
        .zip(&model.nodes)
        .map(|(c, n)| Rect::from_center_size(c.to_pos2(), node_size(n)))
        .collect();
    // From the origin, a margin around: loops and labels are drawn outside the nodes.
    let shift = to_origin(rects.iter().copied());
    rects.into_iter().map(|r| placed(r, shift)).collect()
}

/// The centres of the nodes, around the origin: the servers on the inner ring in their order,
/// the first on the left, then clockwise; every other node on the outer ring, towards the
/// servers it is connected to.
fn ring_centres(model: &Model, inner: f32, outer: f32, stretch: Vec2) -> Vec<Vec2> {
    let tau = std::f32::consts::TAU;
    let servers: Vec<usize> = (0..model.nodes.len())
        .filter(|i| matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let others: Vec<usize> = (0..model.nodes.len())
        .filter(|i| !matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let on_ring = |radius: f32, angle: f32| {
        Vec2::new(
            radius * stretch.x * angle.cos(),
            radius * stretch.y * angle.sin(),
        )
    };
    let mut centres = vec![Vec2::ZERO; model.nodes.len()];
    for (k, i) in servers.iter().enumerate() {
        // The first on the left, then clockwise.
        let angle = std::f32::consts::PI + k as f32 * tau / servers.len() as f32;
        centres[*i] = on_ring(inner, angle);
    }
    if !others.is_empty() {
        let step = tau / others.len() as f32;
        // Where each would rather be: towards the servers it is connected to.
        let wish = |i: usize| -> Option<f32> {
            let sum: Vec2 = model
                .edges
                .iter()
                .filter(|e| e.from == i)
                .map(|e| centres[e.to].normalized())
                .fold(Vec2::ZERO, |a, b| a + b);
            (sum.length() > 0.01).then(|| sum.y.atan2(sum.x))
        };
        let mut order: Vec<(usize, Option<f32>)> = others.iter().map(|i| (*i, wish(*i))).collect();
        order.sort_by(|a, b| {
            let key = |w: Option<f32>| w.unwrap_or(f32::MAX);
            key(a.1).total_cmp(&key(b.1)).then(a.0.cmp(&b.0))
        });
        // Even slots, turned so that the first node with a wish gets it.
        let start = order
            .iter()
            .enumerate()
            .find_map(|(k, (_, w))| w.map(|w| w - k as f32 * step))
            .unwrap_or(-std::f32::consts::FRAC_PI_2);
        for (k, (i, _)) in order.iter().enumerate() {
            centres[*i] = on_ring(outer, start + k as f32 * step);
        }
    }
    centres
}

/// The translation that brings a box, a margin around it, to the origin.
fn to_origin(rects: impl Iterator<Item = Rect>) -> Vec2 {
    let bounds = rects.fold(Rect::NOTHING, |a, r| a.union(r));
    if bounds.is_negative() {
        Vec2::ZERO
    } else {
        -bounds.expand(MARGIN).min.to_vec2()
    }
}

/// A rectangle moved by `shift`, its corner on whole points.
fn placed(r: Rect, shift: Vec2) -> Rect {
    let r = r.translate(shift);
    Rect::from_min_size(Pos2::new(r.min.x.round(), r.min.y.round()), r.size())
}

/// How much of each node the map draws (PRD 9.10, decision of 2026-11-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Every node a box with its title and its lines, every edge with its label.
    Full,
    /// A server a chip with its name alone; a peer a dot, its name beside it when there is room.
    /// What else they say is on hover.
    Compact,
}

/// The scale under which an 11 point text, a node's line or an edge's label, is not readable.
pub const LINES_READABLE: f32 = READABLE / style::SMALL;
/// The scale under which a 13 point name is not readable: the least zoom of the map, so that
/// no server's name is ever drawn under the readable size.
pub const NAMES_READABLE: f32 = READABLE / style::BODY;

/// Where the nodes are drawn, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Places {
    pub detail: Detail,
    /// The frame the nodes were spread over: what a position set by hand is a fraction of.
    pub frame: Vec2,
    /// Each node's rectangle: its box, its chip, or its dot.
    pub rects: Vec<Rect>,
    /// Compact only: where each peer's name is drawn beside its dot. `None` for a server, whose
    /// name is in its chip, and for a name left out because it would cover something drawn.
    pub names: Vec<Option<Rect>>,
}

impl Places {
    /// The rectangle holding every node, every name and what is drawn around them.
    pub fn bounds(&self) -> Rect {
        if self.rects.is_empty() {
            return Rect::from_min_size(Pos2::ZERO, Vec2::splat(2.0 * MARGIN));
        }
        self.rects
            .iter()
            .chain(self.names.iter().flatten())
            .fold(Rect::NOTHING, |a, r| a.union(*r))
            .expand(MARGIN)
    }
}

/// The scale at which all of `bounds` holds in a canvas of `canvas` size; not finite for a
/// canvas or a map of no size.
pub fn fit_scale(bounds: Rect, canvas: Vec2) -> f32 {
    let scale = (canvas / bounds.size()).min_elem();
    if scale > 0.0 { scale } else { f32::NAN }
}

/// A position set by hand, as a fraction of the frame it was set in: the same place in the
/// map whatever the size of the canvas and whichever the level of detail.
pub type Pinned<'a> = &'a dyn Fn(&str) -> Option<Vec2>;

/// What the force directed placement computed for one map in one canvas: the centres of the
/// nodes and the frame they were spread over, in full and in compact. Computed when the map
/// opens and when its structure changes, never on a frame of its own (decision of 2026-11-14).
#[derive(Debug, Clone, PartialEq)]
pub struct Arrangement {
    pub full: (Vec2, Vec<Vec2>),
    pub compact: (Vec2, Vec<Vec2>),
}

/// Room around a node in the placement: what keeps two nodes visibly apart.
const SPACING: f32 = 4.0 * GRID;
/// How much of the frame the nodes' footprints take at least: a force directed placement
/// needs room around each node to show who is connected to whom.
const DENSITY: f32 = 2.5;

/// The least frame for nodes of `footprints` at `aspect`.
fn least_frame(footprints: &[Vec2], aspect: f32) -> Vec2 {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let area: f32 = footprints.iter().map(|f| f.x * f.y).sum::<f32>() * DENSITY;
    let largest = footprints.iter().fold(Vec2::ZERO, |a, f| a.max(*f));
    let w = (area * aspect).sqrt();
    Vec2::new(w, area / w.max(1.0)).max(largest)
}

/// The frame for nodes of `footprints` in a canvas of `canvas` size: the least frame, or the
/// canvas less its margin when the canvas is larger, so that a small map spreads over it.
fn frame_for(footprints: &[Vec2], canvas: Vec2) -> Vec2 {
    least_frame(footprints, canvas.x / canvas.y).max(canvas - Vec2::splat(2.0 * MARGIN))
}

/// Where the placement starts: the servers on an inner ellipse, the others on an outer one,
/// inside the frame.
fn start_in(model: &Model, frame: Vec2) -> Vec<Vec2> {
    let short = frame.min_elem().max(1.0);
    let stretch = frame / short;
    ring_centres(model, 0.2 * short, 0.4 * short, stretch)
        .into_iter()
        .map(|c| c + frame / 2.0)
        .collect()
}

/// The pairs of nodes the edges of `model` join, for the placement.
fn joined(model: &Model) -> Vec<(usize, usize)> {
    model.edges.iter().map(|e| (e.from, e.to)).collect()
}

/// The centres of the nodes of `footprints` in `frame`, those set by hand held there.
fn spread(model: &Model, footprints: &[Vec2], frame: Vec2, pinned: Pinned<'_>) -> Vec<Vec2> {
    let held: Vec<Option<Vec2>> = model
        .nodes
        .iter()
        .map(|n| pinned(&n.key).map(|f| f * frame))
        .collect();
    crate::force::place(&crate::force::Input {
        sizes: footprints,
        edges: &joined(model),
        start: &start_in(model, frame),
        pinned: &held,
        frame,
    })
}

fn footprints_full(model: &Model) -> Vec<Vec2> {
    model
        .nodes
        .iter()
        .map(|n| node_size(n) + Vec2::splat(SPACING))
        .collect()
}

/// In compact, a peer's footprint is its dot; its name, beside it, counts in the frame's area
/// but not in the placement, which would otherwise hold it apart from everything.
fn footprints_compact(model: &Model, widths: &[f32]) -> (Vec<Vec2>, Vec<Vec2>) {
    let mut placed = Vec::new();
    let mut area = Vec::new();
    for (n, w) in model.nodes.iter().zip(widths) {
        if matches!(n.kind, NodeKind::Server(_)) {
            let f = Vec2::new(chip_w(*w), CHIP_H) + Vec2::splat(SPACING);
            placed.push(f);
            area.push(f);
        } else {
            placed.push(
                Vec2::new(DOT + 2.0 * (w.min(NODE_W) + GRID), TITLE_H) + Vec2::splat(SPACING),
            );
            area.push(Vec2::new(DOT + w.min(NODE_W) + GRID, TITLE_H) + Vec2::splat(SPACING));
        }
    }
    (placed, area)
}

/// The placement of `model` in a canvas of `canvas` size, in full and in compact. The costly
/// part: the caller keeps it while the structure of the map and the canvas stay the same.
pub fn arrange(
    model: &Model,
    canvas: Vec2,
    name_w: &dyn Fn(&str) -> f32,
    pinned: Pinned<'_>,
) -> Arrangement {
    let full = footprints_full(model);
    let full_frame = frame_for(&full, canvas);
    let widths: Vec<f32> = model.nodes.iter().map(|n| name_w(&n.title)).collect();
    let (dots, area) = footprints_compact(model, &widths);
    let compact_frame = frame_for(&area, canvas);
    let aspect = canvas.x / canvas.y;
    let mut full_centres = spread(model, &full, full_frame, pinned);
    let mut compact_centres = spread(model, &dots, compact_frame, pinned);
    // With no position set by hand, a placement that left two nodes overlapping gives way to the
    // ellipses of M7, which never do. Positions set by hand are the user's, overlapping or not.
    let held = model.nodes.iter().any(|n| pinned(&n.key).is_some());
    if !held && crate::force::any_overlap(&full_centres, &full) {
        full_centres = ring_layout(model, aspect)
            .iter()
            .map(|r| r.center().to_vec2())
            .collect();
    }
    if !held && crate::force::any_overlap(&compact_centres, &dots) {
        compact_centres = ring_compact(model, aspect, name_w)
            .rects
            .iter()
            .map(|r| r.center().to_vec2())
            .collect();
    }
    Arrangement {
        full: (full_frame, full_centres),
        compact: (compact_frame, compact_centres),
    }
}

/// The places of the map from its arrangement: every node in full when the map holds in the
/// canvas with its lines readable, else compact. A position set by hand since the arrangement
/// was computed is applied here, on the node alone: nothing else moves for it.
pub fn places(
    model: &Model,
    arrangement: &Arrangement,
    canvas: Vec2,
    name_w: &dyn Fn(&str) -> f32,
    pinned: Pinned<'_>,
) -> Places {
    let centre = |frame: Vec2, computed: &[Vec2], i: usize| -> Pos2 {
        let n = &model.nodes[i];
        pinned(&n.key)
            .map(|f| f * frame)
            .or_else(|| computed.get(i).copied())
            .unwrap_or(frame / 2.0)
            .to_pos2()
    };
    let (frame, computed) = &arrangement.full;
    let full = Places {
        detail: Detail::Full,
        frame: *frame,
        rects: (0..model.nodes.len())
            .map(|i| {
                Rect::from_center_size(centre(*frame, computed, i), node_size(&model.nodes[i]))
            })
            .collect(),
        names: vec![None; model.nodes.len()],
    };
    if fit_scale(full.bounds(), canvas) >= LINES_READABLE {
        return full;
    }
    let (frame, computed) = &arrangement.compact;
    let widths: Vec<f32> = model.nodes.iter().map(|n| name_w(&n.title)).collect();
    let rects: Vec<Rect> = (0..model.nodes.len())
        .map(|i| {
            let size = if matches!(model.nodes[i].kind, NodeKind::Server(_)) {
                Vec2::new(chip_w(widths[i]), CHIP_H)
            } else {
                Vec2::splat(DOT)
            };
            Rect::from_center_size(centre(*frame, computed, i), size)
        })
        .collect();
    let names = names_beside(model, &rects, &widths);
    Places {
        detail: Detail::Compact,
        frame: *frame,
        rects,
        names,
    }
}

/// The names of the peers beside their dots, each on the side away from the nodes it is joined
/// to, or from the middle of the map when it is joined to none, in the order of the nodes; a
/// name that would cover a chip, a dot or a name already placed is left out.
fn names_beside(model: &Model, rects: &[Rect], widths: &[f32]) -> Vec<Option<Rect>> {
    let n = rects.len();
    let middle = rects
        .iter()
        .fold(Rect::NOTHING, |a, r| a.union(*r))
        .center();
    let mut names: Vec<Option<Rect>> = vec![None; n];
    let mut taken: Vec<Rect> = rects.iter().map(|r| r.expand(GRID)).collect();
    for i in 0..n {
        if matches!(model.nodes[i].kind, NodeKind::Server(_)) {
            continue;
        }
        let neighbours: Vec<Pos2> = model
            .edges
            .iter()
            .filter_map(|e| match (e.from == i, e.to == i) {
                (true, false) => Some(rects[e.to].center()),
                (false, true) => Some(rects[e.from].center()),
                _ => None,
            })
            .collect();
        let from = if neighbours.is_empty() {
            middle
        } else {
            let sum = neighbours.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2());
            (sum / neighbours.len() as f32).to_pos2()
        };
        let size = Vec2::new(widths[i].min(NODE_W), TITLE_H);
        // The side away from its neighbours first, then the three others, the first free.
        let dot = rects[i];
        let away = name_beside(dot, from, size);
        let sides = [
            away,
            Align2::LEFT_CENTER.anchor_size(dot.right_center() + Vec2::new(GRID, 0.0), size),
            Align2::RIGHT_CENTER.anchor_size(dot.left_center() - Vec2::new(GRID, 0.0), size),
            Align2::CENTER_TOP.anchor_size(dot.center_bottom() + Vec2::new(0.0, GRID), size),
            Align2::CENTER_BOTTOM.anchor_size(dot.center_top() - Vec2::new(0.0, GRID), size),
        ];
        let free = sides.into_iter().find(|at| {
            !taken
                .iter()
                .enumerate()
                .any(|(k, t)| k != i && t.intersects(*at))
        });
        if let Some(at) = free {
            names[i] = Some(at);
            taken.push(at.expand(GRID));
        }
    }
    names
}

#[cfg(test)]
/// The places of the nodes in full, computed, never set by hand: the force directed placement
/// in the least frame for `aspect`, then brought to the origin. Should the placement leave two
/// nodes overlapping, the ellipses of M7 are used, which never do. The same model and aspect
/// give the same places.
pub fn layout(model: &Model, aspect: f32) -> Vec<Rect> {
    let footprints = footprints_full(model);
    let frame = least_frame(&footprints, aspect);
    let centres = spread(model, &footprints, frame, &|_| None);
    if crate::force::any_overlap(&centres, &footprints) {
        return ring_layout(model, aspect);
    }
    let rects: Vec<Rect> = centres
        .iter()
        .zip(&model.nodes)
        .map(|(c, n)| Rect::from_center_size(c.to_pos2(), node_size(n)))
        .collect();
    let shift = to_origin(rects.iter().copied());
    rects.into_iter().map(|r| placed(r, shift)).collect()
}

#[cfg(test)]
/// The compact places, computed: the force directed placement of chips and dots in the least
/// frame for `aspect`, the names beside, brought to the origin. The ellipses of M7 should the
/// placement leave two nodes overlapping.
pub fn layout_compact(model: &Model, aspect: f32, name_w: &dyn Fn(&str) -> f32) -> Places {
    let widths: Vec<f32> = model.nodes.iter().map(|n| name_w(&n.title)).collect();
    let (dots, area) = footprints_compact(model, &widths);
    let frame = least_frame(&area, aspect);
    let centres = spread(model, &dots, frame, &|_| None);
    if crate::force::any_overlap(&centres, &dots) {
        return ring_compact(model, aspect, name_w);
    }
    let rects: Vec<Rect> = (0..model.nodes.len())
        .map(|i| {
            let size = if matches!(model.nodes[i].kind, NodeKind::Server(_)) {
                Vec2::new(chip_w(widths[i]), CHIP_H)
            } else {
                Vec2::splat(DOT)
            };
            Rect::from_center_size(centres[i].to_pos2(), size)
        })
        .collect();
    let names = names_beside(model, &rects, &widths);
    let shift = to_origin(rects.iter().chain(names.iter().flatten()).copied());
    Places {
        detail: Detail::Compact,
        frame,
        rects: rects.into_iter().map(|r| placed(r, shift)).collect(),
        names: names
            .into_iter()
            .map(|r| r.map(|r| placed(r, shift)))
            .collect(),
    }
}

/// The side of a peer's dot.
const DOT: f32 = 3.0 * GRID;
/// A chip's height: its name and its padding.
const CHIP_H: f32 = TITLE_H + 2.0 * PAD;
/// The least length of an edge between a chip and a dot.
const EDGE_COMPACT: f32 = 10.0 * GRID;

/// A server's chip, as wide as its name and at most as wide as a box.
fn chip_w(name_w: f32) -> f32 {
    (name_w + 2.0 * PAD).clamp(16.0 * GRID, NODE_W)
}

/// Where the name of a dot at `dot` goes, of `size`: beside it, away from the centre of the
/// map at `centre`, so that it stays outside the rings.
fn name_beside(dot: Rect, centre: Pos2, size: Vec2) -> Rect {
    let away = (dot.center() - centre).normalized();
    let gap = GRID;
    let (anchor, at) = if away.x > 0.38 {
        (
            Align2::LEFT_CENTER,
            dot.right_center() + Vec2::new(gap, 0.0),
        )
    } else if away.x < -0.38 {
        (
            Align2::RIGHT_CENTER,
            dot.left_center() - Vec2::new(gap, 0.0),
        )
    } else if away.y > 0.0 {
        (
            Align2::CENTER_TOP,
            dot.center_bottom() + Vec2::new(0.0, gap),
        )
    } else {
        (
            Align2::CENTER_BOTTOM,
            dot.center_top() - Vec2::new(0.0, gap),
        )
    };
    anchor.anchor_size(at, size)
}

/// The compact places: chips for the servers on the inner ring, dots for the peers on the
/// outer one, each ring as small as its nodes and an edge between them allow. A peer's name
/// is placed beside its dot, in the order of the nodes, and left out when it would cover a
/// chip, a dot or a name already placed: when room is short a client's name goes, a server's
/// never.
fn ring_compact(model: &Model, aspect: f32, name_w: &dyn Fn(&str) -> f32) -> Places {
    let tau = std::f32::consts::TAU;
    let is_server = |i: usize| matches!(model.nodes[i].kind, NodeKind::Server(_));
    let n = model.nodes.len();
    let servers = (0..n).filter(|i| is_server(*i)).count();
    let others = n - servers;
    let widths: Vec<f32> = model.nodes.iter().map(|m| name_w(&m.title)).collect();
    let widest_chip = (0..n)
        .filter(|i| is_server(*i))
        .map(|i| chip_w(widths[i]))
        .fold(0.0, f32::max);
    let widest_name = (0..n)
        .filter(|i| !is_server(*i))
        .map(|i| widths[i].min(NODE_W))
        .fold(0.0, f32::max);
    // Along the ring at the least stretch: a chip and a gap across, a chip and a gap down.
    let inner = if servers <= 1 {
        0.0
    } else {
        let across = servers as f32 * (widest_chip + 4.0 * GRID) / (tau * STRETCH_MIN.x);
        let down = servers as f32 * (CHIP_H + 4.0 * GRID) / (tau * STRETCH_MIN.y);
        across.max(down).max(12.0 * GRID)
    };
    let outer = if others == 0 {
        inner
    } else {
        // An edge between a chip and a dot, across at the rings' sides, down at their top.
        let across = ((widest_chip + DOT) / 2.0 + EDGE_COMPACT) / STRETCH_MIN.x;
        let down = ((CHIP_H + DOT) / 2.0 + EDGE_COMPACT) / STRETCH_MIN.y;
        let gap = across.max(down);
        (inner + gap).max(others as f32 * 6.0 * GRID / tau)
    };
    // The outer extent of a peer: its dot, and its name on either side.
    let extent = Vec2::new(
        widest_chip.max(DOT + 2.0 * (widest_name + GRID)),
        CHIP_H.max(DOT + 2.0 * (TITLE_H + GRID)),
    );
    let stretch = stretch_for(aspect, outer, extent);
    let centres = ring_centres(model, inner, outer, stretch);
    let rects: Vec<Rect> = (0..n)
        .map(|i| {
            let size = if is_server(i) {
                Vec2::new(chip_w(widths[i]), CHIP_H)
            } else {
                Vec2::splat(DOT)
            };
            Rect::from_center_size(centres[i].to_pos2(), size)
        })
        .collect();
    let mut names: Vec<Option<Rect>> = vec![None; n];
    let mut taken: Vec<Rect> = rects.iter().map(|r| r.expand(GRID)).collect();
    for i in (0..n).filter(|i| !is_server(*i)) {
        let size = Vec2::new(widths[i].min(NODE_W), TITLE_H);
        let at = name_beside(rects[i], Pos2::ZERO, size);
        // Its own dot is beside it, not under it.
        let covers = taken
            .iter()
            .enumerate()
            .any(|(k, t)| k != i && t.intersects(at));
        if !covers {
            names[i] = Some(at);
            taken.push(at.expand(GRID));
        }
    }
    let shift = to_origin(rects.iter().chain(names.iter().flatten()).copied());
    Places {
        detail: Detail::Compact,
        frame: Vec2::ZERO,
        rects: rects.into_iter().map(|r| placed(r, shift)).collect(),
        names: names
            .into_iter()
            .map(|r| r.map(|r| placed(r, shift)))
            .collect(),
    }
}

/// The stretch of the rings, across and down, for a canvas of `aspect`: one at its least, the
/// other as large as the map's box, `2 * radius * stretch + node + 2 * MARGIN` on each side,
/// needs to take the aspect of the canvas. A degenerate aspect keeps the least stretch.
fn stretch_for(aspect: f32, radius: f32, node: Vec2) -> Vec2 {
    if radius <= 0.0 || !aspect.is_finite() || aspect <= 0.0 {
        return STRETCH_MIN;
    }
    let extra = node + Vec2::splat(2.0 * MARGIN);
    // The least stretch down, and across what the aspect then asks.
    let across = (aspect * (2.0 * radius * STRETCH_MIN.y + extra.y) - extra.x) / (2.0 * radius);
    if across >= STRETCH_MIN.x {
        return Vec2::new(across, STRETCH_MIN.y);
    }
    // A canvas too narrow for that: the least across, and down what the aspect asks.
    let down = ((2.0 * radius * STRETCH_MIN.x + extra.x) / aspect - extra.y) / (2.0 * radius);
    Vec2::new(STRETCH_MIN.x, down.max(STRETCH_MIN.y))
}

/// Where the ray from `origin`, inside `rect`, towards `dir` leaves the rectangle.
fn exit(rect: Rect, origin: Pos2, dir: Vec2) -> Pos2 {
    let along = |d: f32, lo: f32, hi: f32, o: f32| {
        if d > 0.0 {
            (hi - o) / d
        } else if d < 0.0 {
            (lo - o) / d
        } else {
            f32::INFINITY
        }
    };
    let t = along(dir.x, rect.left(), rect.right(), origin.x)
        .min(along(dir.y, rect.top(), rect.bottom(), origin.y))
        .max(0.0);
    if t.is_finite() {
        origin + dir * t
    } else {
        origin
    }
}

/// The line of an edge between two nodes, from the border of one to the border of the other,
/// moved `aside` to its left: two nodes connected both ways show two edges, not one.
fn segment(from: Rect, to: Rect, aside: f32) -> Option<(Pos2, Pos2)> {
    let dir = (to.center() - from.center()).normalized();
    if dir == Vec2::ZERO || !dir.is_finite() {
        return None;
    }
    let left = Vec2::new(dir.y, -dir.x) * aside;
    let a = exit(from, from.center() + left, dir);
    let b = exit(to, to.center() + left, -dir);
    ((b - a).dot(dir) > 4.0).then_some((a, b))
}

/// The circle of the `k`th loop of a node: on its top edge, half outside.
fn loop_circle(rect: Rect, k: usize) -> (Pos2, f32) {
    let radius = 3.0 * GRID;
    (
        Pos2::new(
            rect.right() - 6.0 * GRID - k as f32 * 10.0 * GRID,
            rect.top(),
        ),
        radius,
    )
}

fn edge_stroke(edge: &Edge) -> Stroke {
    let colour = match (edge.stale, edge.how, edge.kind) {
        (true, _, _) => EDGE_STALE,
        (false, How::Ambiguous, _) => EDGE_AMBIGUOUS,
        (false, _, EdgeKind::Sender) => EDGE_SENDER,
        (false, _, EdgeKind::Client) => EDGE_CLIENT,
    };
    Stroke::new(if edge.stale { 1.0 } else { 1.5 }, colour)
}

/// The line of an edge: whole for a client, long dashes for an ambiguous one, short dashes for
/// a UDP source. A stale edge keeps its line and loses its colour.
fn edge_line(edge: &Edge, stroke: Stroke, a: Pos2, b: Pos2) -> Vec<Shape> {
    match (edge.how, edge.kind) {
        (How::Ambiguous, _) => Shape::dashed_line(&[a, b], stroke, 8.0, 5.0),
        (_, EdgeKind::Sender) => Shape::dashed_line(&[a, b], stroke, 2.0, 4.0),
        (_, EdgeKind::Client) => vec![Shape::line_segment([a, b], stroke)],
    }
}

fn single_line(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    ui.painter().layout_job(job)
}

/// Draws the map in `ui`, whose coordinates are those of `places`. Edges under the nodes, their
/// labels over everything. No text is drawn under the readable size, and what is drawn in
/// compact follows an order: a server's name always, a client's name when it has its room, an
/// edge's label only when every client's name is drawn and the label covers nothing. What a
/// node or a label leaves out or cuts short is whole on hover.
/// How much of its colour a node or an edge keeps when another is lit.
pub const DIMMED: f32 = 0.3;

/// What the pointer did to the map's nodes on this frame, by node index.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gesture {
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
    /// The node dragged, and by how much, in the map's coordinates.
    pub dragged: Option<(usize, Vec2)>,
    /// The node whose drag ended.
    pub released: Option<usize>,
}

/// The nodes and the edges lit by the node `focus`, by key: it, the edges that touch it, and
/// the nodes at their other ends. `None` when nothing is lit, or when that node is not drawn.
pub fn lit(model: &Model, focus: Option<&str>) -> Option<(Vec<bool>, Vec<bool>)> {
    let f = model
        .nodes
        .iter()
        .position(|n| Some(n.key.as_str()) == focus)?;
    let mut nodes = vec![false; model.nodes.len()];
    nodes[f] = true;
    let edges: Vec<bool> = model
        .edges
        .iter()
        .map(|e| {
            let touches = e.from == f || e.to == f;
            if touches {
                nodes[e.from] = true;
                nodes[e.to] = true;
            }
            touches
        })
        .collect();
    Some((nodes, edges))
}

pub fn draw(
    ui: &mut egui::Ui,
    model: &Model,
    places: &Places,
    selected: Option<ServerId>,
    focus: Option<&str>,
) -> Gesture {
    let mut gesture = Gesture::default();
    let lit = lit(model, focus);
    let node_lit = |i: usize| lit.as_ref().is_none_or(|(n, _)| n[i]);
    let edge_lit = |i: usize| lit.as_ref().is_none_or(|(_, e)| e[i]);
    let tint = |c: Color32, on: bool| if on { c } else { c.gamma_multiply(DIMMED) };
    let rects = &places.rects;
    let compact = places.detail == Detail::Compact;
    let visuals = ui.visuals().clone();
    let small = FontId::proportional(style::SMALL);
    let body = FontId::proportional(style::BODY);
    // A text under the readable size on screen is not drawn: what it says is on hover.
    let scale = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .map_or(1.0, |t| t.scaling);
    let readable = |font: &FontId| font.size * scale >= READABLE;
    let (titles, lines) = (readable(&body), readable(&small));
    let every_name = model
        .nodes
        .iter()
        .zip(&places.names)
        .all(|(n, name)| matches!(n.kind, NodeKind::Server(_)) || name.is_some());
    let labels_shown = lines && (!compact || (titles && every_name));
    let painter = ui.painter().clone();
    // The labels, drawn last: where, what, and the edge they belong to.
    let mut labels: Vec<(Pos2, usize)> = Vec::new();
    let mut between: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (i, edge) in model.edges.iter().enumerate() {
        let (Some(from), Some(to)) = (rects.get(edge.from), rects.get(edge.to)) else {
            continue;
        };
        let k = between.entry((edge.from, edge.to)).or_default();
        let nth = *k;
        *k += 1;
        let mut stroke = edge_stroke(edge);
        stroke.color = tint(stroke.color, edge_lit(i));
        if edge.from == edge.to {
            let (centre, radius) = loop_circle(*to, nth);
            painter.add(Shape::circle_stroke(centre, radius, stroke));
            labels.push((centre - Vec2::new(0.0, radius + 2.0 * GRID), i));
            continue;
        }
        let aside = if compact {
            nth as f32 * 3.0 * GRID
        } else {
            1.5 * GRID + nth as f32 * 3.0 * GRID
        };
        let Some((a, b)) = segment(*from, *to, aside) else {
            continue;
        };
        painter.extend(edge_line(edge, stroke, a, b));
        // The head, at the server: the edge goes from the peer to the server it is connected to.
        let dir = (b - a).normalized();
        for turn in [0.45f32, -0.45] {
            let (sin, cos) = turn.sin_cos();
            let back =
                Vec2::new(dir.x * cos - dir.y * sin, dir.x * sin + dir.y * cos) * -2.5 * GRID;
            painter.line_segment([b, b + back], stroke);
        }
        let left = Vec2::new(dir.y, -dir.x);
        labels.push((a + (b - a) * 0.5 + left * 2.5 * GRID, i));
    }
    let hover = |ui: &mut egui::Ui, node: &Node| {
        ui.label(egui::RichText::new(&node.title).strong());
        for (text, _) in &node.lines {
            ui.label(text);
        }
        for text in &node.detail {
            ui.label(text);
        }
    };
    for (i, (node, rect)) in model.nodes.iter().zip(rects).enumerate() {
        let warns = node.lines.iter().any(|(_, t)| *t == Tone::Warn);
        let (fill, mut edge) = (
            visuals.extreme_bg_color,
            visuals.widgets.noninteractive.bg_stroke,
        );
        match node.kind {
            NodeKind::ThisApp => edge = Stroke::new(1.5, visuals.selection.bg_fill),
            NodeKind::Server(id) if selected == Some(id) => {
                edge = Stroke::new(1.5, visuals.widgets.inactive.fg_stroke.color)
            }
            // A chip says no line: that one of them is a warning shows on its border.
            NodeKind::Server(_) if compact && warns => {
                edge = Stroke::new(1.5, visuals.warn_fg_color)
            }
            _ => {}
        }
        let on = node_lit(i);
        edge.color = tint(edge.color, on);
        let strong = tint(visuals.strong_text_color(), on);
        let text = tint(visuals.text_color(), on);
        let weak = tint(visuals.weak_text_color(), on);
        let warn = tint(visuals.warn_fg_color, on);
        let sense = Sense::click_and_drag();
        let mut react = |response: egui::Response| {
            if response.double_clicked() {
                gesture.double_clicked = Some(i);
            } else if response.clicked() {
                gesture.clicked = Some(i);
            }
            if response.dragged() && response.drag_delta() != Vec2::ZERO {
                gesture.dragged = Some((i, response.drag_delta()));
            }
            if response.drag_stopped() {
                gesture.released = Some(i);
            }
        };
        if compact {
            let mut zone = *rect;
            if matches!(node.kind, NodeKind::Server(_)) {
                painter.rect(*rect, 1.5 * GRID, fill, edge, egui::StrokeKind::Inside);
                // Always: the least zoom keeps a name readable.
                let title = single_line(
                    ui,
                    &node.title,
                    body.clone(),
                    strong,
                    rect.width() - 2.0 * PAD,
                );
                painter.galley(rect.center() - title.size() / 2.0, title, strong);
            } else {
                let colour = tint(
                    match node.kind {
                        NodeKind::ThisApp => visuals.selection.bg_fill,
                        NodeKind::Sender => EDGE_SENDER,
                        _ => visuals.strong_text_color(),
                    },
                    on,
                );
                painter.circle_filled(rect.center(), DOT / 2.0 - 1.0, colour);
                if let (true, Some(at)) = (titles, places.names[i]) {
                    let name = single_line(ui, &node.title, body.clone(), text, at.width());
                    painter.galley(at.min, name, text);
                    zone = zone.union(at);
                }
            }
            react(
                ui.interact(zone.expand(GRID), ui.id().with(("map node", i)), sense)
                    .on_hover_ui(|ui| hover(ui, node)),
            );
            continue;
        }
        painter.rect(*rect, 1.5 * GRID, fill, edge, egui::StrokeKind::Inside);
        let width = rect.width() - 2.0 * PAD;
        let mut at = rect.min + Vec2::splat(PAD);
        if titles {
            let title = single_line(ui, &node.title, body.clone(), strong, width);
            painter.galley(
                at + Vec2::new(0.0, (TITLE_H - title.size().y) / 2.0),
                title,
                strong,
            );
        }
        at.y += TITLE_H;
        for (text, tone) in node.lines.iter().filter(|_| lines) {
            let colour = match tone {
                Tone::Plain => weak,
                Tone::Warn => warn,
            };
            let line = single_line(ui, text, small.clone(), colour, width);
            painter.galley(
                at + Vec2::new(0.0, (LINE_H - line.size().y) / 2.0),
                line,
                colour,
            );
            at.y += LINE_H;
        }
        react(
            ui.interact(*rect, ui.id().with(("map node", i)), sense)
                .on_hover_ui(|ui| hover(ui, node)),
        );
    }
    // In compact, a label covers nothing drawn: not a chip, a dot, a name or another label.
    let mut taken: Vec<Rect> = if compact {
        rects
            .iter()
            .chain(places.names.iter().flatten())
            .copied()
            .collect()
    } else {
        Vec::new()
    };
    for (pos, i) in labels.into_iter().filter(|_| labels_shown) {
        let edge = &model.edges[i];
        if edge.label.is_empty() && edge.detail.is_empty() {
            continue;
        }
        // An edge that carries nothing to say still has its entry to show, behind a mark.
        let text = if edge.label.is_empty() {
            "\u{b7}"
        } else {
            edge.label.as_str()
        };
        let colour = tint(
            if edge.stale {
                visuals.weak_text_color()
            } else {
                visuals.text_color()
            },
            edge_lit(i),
        );
        let galley = painter.layout_no_wrap(text.to_owned(), small.clone(), colour);
        let rect = Align2::CENTER_CENTER
            .anchor_size(pos, galley.size())
            .expand2(Vec2::new(GRID, 0.5 * GRID));
        if compact {
            if taken.iter().any(|t| t.intersects(rect)) {
                continue;
            }
            taken.push(rect);
        }
        painter.rect_filled(rect, GRID, visuals.panel_fill);
        painter.galley(rect.min + Vec2::new(GRID, 0.5 * GRID), galley, colour);
        ui.interact(rect, ui.id().with(("map edge", i)), Sense::hover())
            .on_hover_ui(|ui| {
                if !edge.label.is_empty() {
                    ui.label(&edge.label);
                }
                for line in &edge.detail {
                    ui.label(style::data(line));
                }
                if edge.stale {
                    ui.label("From a reading that is no longer current.");
                }
            });
    }
    gesture
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use oscq_core::{
        DiscoveredServer, Event, HostInfo, Origin as ServerOrigin, PeersReading, TransportState,
    };
    use serde_json::{Value, json};

    const POLL: Duration = Duration::from_millis(2000);

    /// A connected server named `name` at `ip`, declaring the extension or not.
    fn add(state: &mut AppState, name: &str, ip: &str, port: u16, declares: bool) -> ServerId {
        let server = DiscoveredServer {
            id: ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: name.to_owned(),
            host: ip.to_owned(),
            port,
            addresses: Vec::new(),
            origin: ServerOrigin::Manual,
            stale: false,
        };
        let id = server.id;
        state.apply(Event::ServerFound(server));
        let extensions = if declares {
            json!({"PEERS": true, "CLIENT_NAME": true})
        } else {
            json!({})
        };
        let host_info = json!({"NAME": name, "EXTENSIONS": extensions}).to_string();
        let (info, _) = HostInfo::parse(host_info.as_bytes()).unwrap();
        let destination = info.osc_destination(ip, port);
        state.apply(Event::HostInfo {
            server_id: id,
            info,
            destination,
        });
        state.apply(Event::TransportState {
            server_id: id,
            state: TransportState::HttpOnly,
        });
        id
    }

    fn read(
        state: &mut AppState,
        id: ServerId,
        at: Instant,
        reply: Value,
        this_client: Option<usize>,
    ) {
        let (peers, _) = oscq_core::Peers::parse(reply.to_string().as_bytes()).unwrap();
        state.apply(Event::Peers {
            server_id: id,
            at,
            result: Ok(PeersReading { peers, this_client }),
        });
    }

    fn node_of(model: &Model, title: &str) -> usize {
        let found: Vec<usize> = (0..model.nodes.len())
            .filter(|i| model.nodes[*i].title == title)
            .collect();
        assert_eq!(
            found.len(),
            1,
            "one node titled {title:?}: {:?}",
            model.nodes
        );
        found[0]
    }

    /// PRD 7.6 and 9.10: a client is matched to a known server on its name first; a name that
    /// is no known server's, on its address alone; never on its port. A client that gave no
    /// name is a node of its own, labelled by its address and port, even at the address of a
    /// known server: it is never given that server's name. What matches nothing is a node of
    /// its own.
    #[test]
    fn a_client_is_matched_on_its_name_then_on_its_address_never_on_its_port() {
        let mut state = AppState::default();
        let now = Instant::now();
        let a = add(&mut state, "A", "10.0.0.1", 5000, true);
        add(&mut state, "B", "10.0.0.2", 5000, true);
        add(&mut state, "C", "10.0.0.3", 5000, true);
        read(
            &mut state,
            a,
            now,
            json!({"CLIENTS": [
                // By name: B, though it is seen at C's address.
                {"IP": "10.0.0.3", "PORT": 40001, "NAME": "B", "PACKETS": 7},
                // A name no server has, at C's address: C, by address, named on its edge.
                {"IP": "::ffff:10.0.0.3", "PORT": 40002, "NAME": "TouchDesigner"},
                // No name, at C's address, its port B's server port: neither C nor B.
                {"IP": "10.0.0.3", "PORT": 5000},
                // A name no server has, at an address no server has: its own node.
                {"IP": "10.0.0.9", "PORT": 40003, "NAME": "Ghost"}
            ]}),
            None,
        );
        let m = build(&state, now, POLL, "Inspector");
        let to = node_of(&m, "A");
        let got: Vec<(usize, How, &str)> = m
            .edges
            .iter()
            .map(|e| {
                assert_eq!((e.to, e.kind), (to, EdgeKind::Client));
                (e.from, e.how, e.label.as_str())
            })
            .collect();
        assert_eq!(
            got,
            [
                (node_of(&m, "B"), How::Name, "7 packets"),
                (node_of(&m, "C"), How::Address, "TouchDesigner, by address"),
                (node_of(&m, "unnamed client"), How::Own, ""),
                (node_of(&m, "Ghost"), How::Own, ""),
            ]
        );
        let unnamed = &m.nodes[node_of(&m, "unnamed client")];
        assert_eq!(unnamed.lines, [("10.0.0.3:5000".to_owned(), Tone::Plain)]);
        assert_eq!(
            m.nodes.len(),
            5,
            "three servers and two peers: {:?}",
            m.nodes
        );
    }

    /// PRD 7.6: a server reporting a client under its own name is connected to itself. The
    /// edge is kept, from the node to itself.
    #[test]
    fn a_server_connected_to_itself_keeps_its_edge_as_a_loop() {
        let mut state = AppState::default();
        let now = Instant::now();
        let a = add(&mut state, "A", "10.0.0.1", 5000, true);
        read(
            &mut state,
            a,
            now,
            json!({"CLIENTS": [{"IP": "10.0.0.1", "PORT": 40001, "NAME": "A"}]}),
            None,
        );
        let m = build(&state, now, POLL, "Inspector");
        let node = node_of(&m, "A");
        assert_eq!(m.nodes.len(), 1);
        let [edge] = &m.edges[..] else {
            panic!("one edge: {:?}", m.edges)
        };
        assert_eq!((edge.from, edge.to, edge.how), (node, node, How::Name));
    }

    /// PRD 7.6: two known servers sharing one name make the match ambiguous. The edge starts
    /// from neither: from a node of the client's own, and it says so.
    #[test]
    fn a_name_two_servers_share_matches_neither() {
        let mut state = AppState::default();
        let now = Instant::now();
        let a = add(&mut state, "A", "10.0.0.1", 5000, true);
        let twin1 = add(&mut state, "Twin", "10.0.0.2", 5000, true);
        let twin2 = add(&mut state, "Twin", "10.0.0.3", 5000, true);
        read(
            &mut state,
            a,
            now,
            // Seen at the address of one of the two: the name was ambiguous, the address is
            // not asked to settle it.
            json!({"CLIENTS": [{"IP": "10.0.0.2", "PORT": 40001, "NAME": "Twin"}]}),
            None,
        );
        let m = build(&state, now, POLL, "Inspector");
        let [edge] = &m.edges[..] else {
            panic!("one edge: {:?}", m.edges)
        };
        assert_eq!(edge.how, How::Ambiguous);
        assert_eq!(
            edge.label,
            "ambiguous: name of 2 known servers: not matched"
        );
        let from = &m.nodes[edge.from];
        assert_eq!(from.kind, NodeKind::Client, "from neither server");
        for twin in [twin1, twin2] {
            assert_ne!(from.kind, NodeKind::Server(twin));
        }
        assert_eq!(
            from.lines,
            [
                ("10.0.0.2".to_owned(), Tone::Plain),
                (
                    "name of 2 known servers: not matched".to_owned(),
                    Tone::Plain
                )
            ]
        );
    }

    /// PRD 9.10: this application is one node, whatever the number of servers that list it,
    /// and it is told from another client that gave the same name.
    #[test]
    fn this_application_is_one_node_told_from_a_namesake() {
        let mut state = AppState::default();
        let now = Instant::now();
        let a = add(&mut state, "A", "10.0.0.1", 5000, true);
        let b = add(&mut state, "B", "10.0.0.2", 5000, true);
        for id in [a, b] {
            read(
                &mut state,
                id,
                now,
                json!({"CLIENTS": [
                    {"IP": "10.0.0.50", "PORT": 40001, "NAME": "Inspector"},
                    {"IP": "10.0.0.60", "PORT": 40002, "NAME": "Inspector"}
                ]}),
                Some(0),
            );
        }
        let m = build(&state, now, POLL, "Inspector");
        let this: Vec<usize> = (0..m.nodes.len())
            .filter(|i| m.nodes[*i].kind == NodeKind::ThisApp)
            .collect();
        let [this] = this[..] else {
            panic!("one node for this application: {:?}", m.nodes)
        };
        assert_eq!(
            m.nodes[this].lines,
            [("this application".to_owned(), Tone::Plain)]
        );
        let from_this: Vec<usize> = m
            .edges
            .iter()
            .filter(|e| e.from == this)
            .map(|e| e.to)
            .collect();
        assert_eq!(from_this, [node_of(&m, "A"), node_of(&m, "B")]);
        // The namesake at another address: a client like any other, one node for its two edges.
        let namesake: Vec<&Edge> = m.edges.iter().filter(|e| e.how == How::Own).collect();
        assert_eq!(namesake.len(), 2);
        assert_eq!(namesake[0].from, namesake[1].from);
        assert_eq!(m.nodes[namesake[0].from].kind, NodeKind::Client);
    }

    /// PRD 9.10: a UDP source is an edge of its own kind, from a node of its own unless its
    /// address is that of exactly one known server.
    #[test]
    fn a_udp_source_is_an_edge_of_its_own_kind() {
        let mut state = AppState::default();
        let now = Instant::now();
        let a = add(&mut state, "A", "10.0.0.1", 5000, true);
        add(&mut state, "B", "10.0.0.2", 5000, false);
        read(
            &mut state,
            a,
            now,
            json!({"SENDERS": [
                {"IP": "10.0.0.2", "PORT": 9001, "PACKETS": 3, "LAST_MS": 40},
                {"IP": "10.0.0.77", "PORT": 9002}
            ]}),
            None,
        );
        let m = build(&state, now, POLL, "Inspector");
        let got: Vec<(usize, EdgeKind, How, &str)> = m
            .edges
            .iter()
            .map(|e| (e.from, e.kind, e.how, e.label.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    node_of(&m, "B"),
                    EdgeKind::Sender,
                    How::Address,
                    "UDP from port 9001, by address, 3 packets"
                ),
                (node_of(&m, "UDP source"), EdgeKind::Sender, How::Own, "UDP"),
            ]
        );
        assert_eq!(
            m.edges[0].detail,
            ["10.0.0.2:9001", "last packet 40 ms before the reading"]
        );
        assert_eq!(
            m.edges[1].detail,
            ["10.0.0.77:9002"],
            "no LAST_MS, none said"
        );
    }

    /// The places are computed from the model alone: the same model, the same places, no node
    /// on another, whatever the number of nodes the budget names.
    /// The compact places: the same model, aspect and widths give the same places; no chip, dot
    /// or placed name covers another; a name left out is a client's, a server having none to
    /// lose; and clients set by hand on one spot lose their names rather than draw over each
    /// other.
    #[test]
    fn compact_places_cover_nothing_and_leave_out_only_clients_names() {
        let now = Instant::now();
        for (servers, peers, width) in [(2, 1, 7.0), (3, 6, 7.0), (8, 24, 7.0), (8, 24, 24.0)] {
            let mut state = AppState::default();
            let ids: Vec<ServerId> = (0..servers)
                .map(|i| {
                    add(
                        &mut state,
                        &format!("S{i}"),
                        &format!("10.0.0.{}", i + 1),
                        5000,
                        true,
                    )
                })
                .collect();
            for (s, id) in ids.iter().enumerate() {
                let clients: Vec<Value> = (0..peers)
                    .filter(|q| q % servers == s)
                    .map(|q| json!({"IP": "10.0.1.1", "PORT": 40000 + q, "NAME": format!("client {q}")}))
                    .collect();
                read(&mut state, *id, now, json!({ "CLIENTS": clients }), None);
            }
            let m = build(&state, now, POLL, "Inspector");
            let name_w = |t: &str| t.chars().count() as f32 * width;
            for aspect in [0.5, 1.0, 1.6, 4.0] {
                let p = layout_compact(&m, aspect, &name_w);
                assert_eq!(p, layout_compact(&m, aspect, &name_w), "the same places");
                assert_eq!(p.detail, Detail::Compact);
                let mut drawn: Vec<Rect> = p.rects.clone();
                drawn.extend(p.names.iter().flatten());
                for (i, a) in drawn.iter().enumerate() {
                    assert!(a.min.x >= 0.0 && a.min.y >= 0.0, "{a:?}");
                    for b in &drawn[i + 1..] {
                        assert!(
                            !a.intersects(*b),
                            "{servers}+{peers}, width {width}, aspect {aspect}: {a:?} covers {b:?}"
                        );
                    }
                }
                for (node, name) in m.nodes.iter().zip(&p.names) {
                    if matches!(node.kind, NodeKind::Server(_)) {
                        assert_eq!(*name, None, "a server's name is in its chip");
                    }
                }
                if servers == 8 {
                    // Every client set by hand on one spot: their names can only cover each
                    // other, and all but one are left out; the servers lose nothing.
                    let canvas = Vec2::new(1000.0, 1000.0 / aspect);
                    let pin = |key: &str| key.starts_with("client ").then_some(Vec2::splat(0.5));
                    let arranged = arrange(&m, canvas, &name_w, &pin);
                    let crowded = places(&m, &arranged, canvas, &name_w, &pin);
                    if crowded.detail == Detail::Compact {
                        let placed = crowded.names.iter().flatten().count();
                        assert!(
                            placed <= 1,
                            "aspect {aspect}: at most one name on the spot, {placed}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_places_are_computed_and_no_node_covers_another() {
        for (servers, peers) in [(1, 0), (1, 1), (2, 1), (2, 3), (5, 6), (8, 24)] {
            let mut state = AppState::default();
            let now = Instant::now();
            let ids: Vec<ServerId> = (0..servers)
                .map(|i| {
                    add(
                        &mut state,
                        &format!("S{i}"),
                        &format!("10.0.0.{}", i + 1),
                        5000,
                        true,
                    )
                })
                .collect();
            for (s, id) in ids.iter().enumerate() {
                let clients: Vec<Value> = (0..peers)
                    .filter(|q| q % servers == s)
                    .map(|q| json!({"IP": "10.0.1.1", "PORT": 40000 + q}))
                    .collect();
                read(&mut state, *id, now, json!({ "CLIENTS": clients }), None);
            }
            let m = build(&state, now, POLL, "Inspector");
            assert_eq!(m.nodes.len(), servers + peers);
            // Canvases from twice as tall as wide to four times as wide as tall.
            for aspect in [0.5, 0.8, 1.0, 1.6, 2.5, 4.0] {
                let rects = layout(&m, aspect);
                assert_eq!(rects, layout(&m, aspect), "the same model, the same places");
                for (i, a) in rects.iter().enumerate() {
                    assert!(a.min.x >= 0.0 && a.min.y >= 0.0, "{servers}+{peers}: {a:?}");
                    for b in &rects[i + 1..] {
                        assert!(
                            !a.intersects(*b),
                            "{servers} servers and {peers} peers, aspect {aspect}: {a:?} \
                             covers {b:?}"
                        );
                    }
                }
            }
        }
    }
}
