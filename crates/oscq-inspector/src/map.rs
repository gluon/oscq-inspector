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
    AppState, ConnState, PeersSnapshot, PeersView, ServerEntry, client_label, ip_port,
};
use crate::style::{self, GRID};

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
    pub title: String,
    /// What is known of it, one statement a line, and nothing more.
    pub lines: Vec<(String, Tone)>,
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
                lines.push((format!("peers not read: {reason}"), Tone::Warn))
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
                    lines.push((format!("not read since: {reason}"), Tone::Warn));
                }
            }
        }
    }
    Node {
        kind: NodeKind::Server(e.server.id),
        title: e.server.name.clone(),
        lines,
    }
}

/// Where the edge of a `CLIENTS` entry starts (PRD 7.6): on `NAME` first, then, failing that,
/// on `IP` alone, never on `PORT`. A name or an address that several known servers share
/// matches none of them: the application does not guess.
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
    if let Some(name) = &c.name {
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
                title: app_name.to_owned(),
                lines: vec![("this application".to_owned(), Tone::Plain)],
            },
            Origin::Named { name, ip } => Node {
                kind: NodeKind::Client,
                title: name.clone(),
                lines: std::iter::once((ip.clone(), Tone::Plain))
                    .chain(tone(notes))
                    .collect(),
            },
            Origin::Unnamed { ip, port } => Node {
                kind: NodeKind::Client,
                title: "unnamed client".to_owned(),
                lines: std::iter::once((ip_port(ip, *port), Tone::Plain))
                    .chain(tone(notes))
                    .collect(),
            },
            Origin::Sender { ip, port } => Node {
                kind: NodeKind::Sender,
                title: "UDP source".to_owned(),
                lines: vec![(ip_port(ip, *port), Tone::Plain)],
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

const NODE_W: f32 = 52.0 * GRID;
const TITLE_H: f32 = 5.0 * GRID;
const LINE_H: f32 = 4.0 * GRID;
const PAD: f32 = 2.0 * GRID;
/// Room a node takes along its ring, and the least distance between the two rings.
const SLOT: f32 = NODE_W + 16.0 * GRID;
const RING_GAP: f32 = 56.0 * GRID;
/// The rings are ellipses, wider than tall, as the nodes are.
const STRETCH: Vec2 = Vec2::new(1.25, 0.75);
const MARGIN: f32 = 12.0 * GRID;

fn node_size(node: &Node) -> Vec2 {
    Vec2::new(
        NODE_W,
        2.0 * PAD + TITLE_H + node.lines.len() as f32 * LINE_H,
    )
}

/// The places of the nodes: computed, never authored (PRD 9.10). The servers on a ring, in
/// their order; every other node on a ring around it, towards the servers it is connected to.
/// The same model gives the same places.
pub fn layout(model: &Model) -> Vec<Rect> {
    let servers: Vec<usize> = (0..model.nodes.len())
        .filter(|i| matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let others: Vec<usize> = (0..model.nodes.len())
        .filter(|i| !matches!(model.nodes[*i].kind, NodeKind::Server(_)))
        .collect();
    let on_ring = |radius: f32, angle: f32| {
        Vec2::new(
            radius * STRETCH.x * angle.cos(),
            radius * STRETCH.y * angle.sin(),
        )
    };
    let tau = std::f32::consts::TAU;
    let mut centres = vec![Vec2::ZERO; model.nodes.len()];
    let inner = if servers.len() <= 1 {
        0.0
    } else {
        (servers.len() as f32 * SLOT / tau).max(40.0 * GRID)
    };
    for (k, i) in servers.iter().enumerate() {
        // The first on the left, then clockwise.
        let angle = std::f32::consts::PI + k as f32 * tau / servers.len() as f32;
        centres[*i] = on_ring(inner, angle);
    }
    if !others.is_empty() {
        let outer = (inner + RING_GAP).max(others.len() as f32 * SLOT / tau);
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
    let rects: Vec<Rect> = centres
        .iter()
        .zip(&model.nodes)
        .map(|(c, n)| Rect::from_center_size(c.to_pos2(), node_size(n)))
        .collect();
    // From the origin, a margin around: loops and labels are drawn outside the nodes.
    let bounds = rects
        .iter()
        .fold(Rect::NOTHING, |a, r| a.union(*r))
        .expand(MARGIN);
    let shift = if rects.is_empty() {
        Vec2::ZERO
    } else {
        -bounds.min.to_vec2()
    };
    rects
        .into_iter()
        .map(|r| {
            let r = r.translate(shift);
            Rect::from_min_size(
                Pos2::new(r.min.x.round(), r.min.y.round()),
                Vec2::new(r.width(), r.height()),
            )
        })
        .collect()
}

/// The rectangle holding every node and what is drawn around them.
pub fn bounds(rects: &[Rect]) -> Rect {
    if rects.is_empty() {
        return Rect::from_min_size(Pos2::ZERO, Vec2::splat(2.0 * MARGIN));
    }
    rects
        .iter()
        .fold(Rect::NOTHING, |a, r| a.union(*r))
        .expand(MARGIN)
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
fn edge_line(edge: &Edge, a: Pos2, b: Pos2) -> Vec<Shape> {
    let stroke = edge_stroke(edge);
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

/// Draws the map in `ui`, whose coordinates are those of `rects`. Edges under the nodes, their
/// labels over everything. What a node or a label cuts short is whole on hover.
pub fn draw(ui: &mut egui::Ui, model: &Model, rects: &[Rect], selected: Option<ServerId>) {
    let visuals = ui.visuals().clone();
    let small = FontId::proportional(style::SMALL);
    let body = FontId::proportional(style::BODY);
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
        let stroke = edge_stroke(edge);
        if edge.from == edge.to {
            let (centre, radius) = loop_circle(*to, nth);
            painter.add(Shape::circle_stroke(centre, radius, stroke));
            labels.push((centre - Vec2::new(0.0, radius + 2.0 * GRID), i));
            continue;
        }
        let aside = 1.5 * GRID + nth as f32 * 3.0 * GRID;
        let Some((a, b)) = segment(*from, *to, aside) else {
            continue;
        };
        painter.extend(edge_line(edge, a, b));
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
    for (i, (node, rect)) in model.nodes.iter().zip(rects).enumerate() {
        let (fill, mut edge) = (
            visuals.extreme_bg_color,
            visuals.widgets.noninteractive.bg_stroke,
        );
        match node.kind {
            NodeKind::ThisApp => edge = Stroke::new(1.5, visuals.selection.bg_fill),
            NodeKind::Server(id) if selected == Some(id) => {
                edge = Stroke::new(1.5, visuals.widgets.inactive.fg_stroke.color)
            }
            _ => {}
        }
        painter.rect(*rect, 1.5 * GRID, fill, edge, egui::StrokeKind::Inside);
        let width = rect.width() - 2.0 * PAD;
        let mut at = rect.min + Vec2::splat(PAD);
        let title = single_line(
            ui,
            &node.title,
            body.clone(),
            visuals.strong_text_color(),
            width,
        );
        painter.galley(
            at + Vec2::new(0.0, (TITLE_H - title.size().y) / 2.0),
            title,
            visuals.strong_text_color(),
        );
        at.y += TITLE_H;
        for (text, tone) in &node.lines {
            let colour = match tone {
                Tone::Plain => visuals.weak_text_color(),
                Tone::Warn => visuals.warn_fg_color,
            };
            let line = single_line(ui, text, small.clone(), colour, width);
            painter.galley(
                at + Vec2::new(0.0, (LINE_H - line.size().y) / 2.0),
                line,
                colour,
            );
            at.y += LINE_H;
        }
        ui.interact(*rect, ui.id().with(("map node", i)), Sense::hover())
            .on_hover_ui(|ui| {
                ui.label(egui::RichText::new(&node.title).strong());
                for (text, _) in &node.lines {
                    ui.label(text);
                }
            });
    }
    for (pos, i) in labels {
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
        let colour = if edge.stale {
            visuals.weak_text_color()
        } else {
            visuals.text_color()
        };
        let galley = painter.layout_no_wrap(text.to_owned(), small.clone(), colour);
        let rect = Align2::CENTER_CENTER
            .anchor_size(pos, galley.size())
            .expand2(Vec2::new(GRID, 0.5 * GRID));
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

    /// PRD 7.6: a client is matched to a known server on its name first; with no name to go
    /// by, on its address alone; never on its port. What matches nothing is a node of its own,
    /// a client with no name labelled as unnamed by its address and port.
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
                // No name: by address, C. Its port is B's server port, which says nothing.
                {"IP": "::ffff:10.0.0.3", "PORT": 5000},
                // A name no server has, at an address no server has: its own node.
                {"IP": "10.0.0.9", "PORT": 40003, "NAME": "TouchDesigner"},
                // No name, no known address: an unnamed client.
                {"IP": "10.0.0.8", "PORT": 40004}
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
                (
                    node_of(&m, "C"),
                    How::Address,
                    "unnamed client [::ffff:10.0.0.3]:5000, by address"
                ),
                (node_of(&m, "TouchDesigner"), How::Own, ""),
                (node_of(&m, "unnamed client"), How::Own, ""),
            ]
        );
        let unnamed = &m.nodes[node_of(&m, "unnamed client")];
        assert_eq!(unnamed.lines, [("10.0.0.8:40004".to_owned(), Tone::Plain)]);
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
            let rects = layout(&m);
            assert_eq!(rects, layout(&m), "the same model, the same places");
            for (i, a) in rects.iter().enumerate() {
                assert!(a.min.x >= 0.0 && a.min.y >= 0.0, "{servers}+{peers}: {a:?}");
                for b in &rects[i + 1..] {
                    assert!(
                        !a.intersects(*b),
                        "{servers} servers and {peers} peers: {a:?} covers {b:?}"
                    );
                }
            }
        }
    }
}
