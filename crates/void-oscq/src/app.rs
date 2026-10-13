//! The window. Runs on the UI thread: never blocks, never does I/O. Network work goes out as
//! `Command`s, results come back as `Event`s drained at the top of every frame; file writes are
//! handed to the runtime.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::text::LayoutJob;
use egui::{Color32, FontId, Key, Modifiers, RichText, TextFormat};
use oscq_core::model::parse_rgba_hex;
use oscq_core::osc::arg_from_text;
use oscq_core::{
    ArgType, Command, Event, Origin, OscPath, PortSource, ServerId, ServiceKind, TypeSignature,
    TypeTag,
};
use oscq_discovery::{ManualEntry, ManualHistory};
use rosc::{OscColor, OscType};

use crate::config::{self, Config, Theme, WindowGeometry};
use crate::json_view::{self, Line, Token};
use crate::presets;
use crate::rate::{INTERVAL, RateLimiter};
use crate::state::{AppState, ConnState, ServerEntry, WsState};
use crate::style::{self, GRID, GROUP_GAP, ROW_H};
use crate::traffic;
use crate::tree::{self, Hit, NodeIndex, Row};
use crate::widget::{self, WidgetKind};

/// Events applied per frame at most, so a flood cannot starve rendering.
const EVENTS_PER_FRAME: usize = 512;
/// Window geometry is saved once it has been stable this long.
const GEOMETRY_SETTLE: Duration = Duration::from_millis(500);
/// An empty server list shows a hint after this long, not a spinner forever.
const DISCOVERY_HINT_AFTER: Duration = Duration::from_secs(2);

pub struct Backend {
    pub commands: flume::Sender<Command>,
    pub events: flume::Receiver<Event>,
    /// Runtime for file writes, so the UI thread never touches the disk.
    pub io: tokio::runtime::Handle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendReq {
    pub server_id: ServerId,
    pub path: OscPath,
    pub signature: TypeSignature,
    pub args: Vec<OscType>,
    /// Whether the node can be read back: a refresh after the send is pointless otherwise.
    pub readable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendMode {
    /// From a continuous widget being dragged: rate limited, no refresh.
    Drag,
    /// Commit, click, release: always sent, value refreshed after when the node is readable.
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Tree,
    Json,
    Traffic,
}

enum Action {
    SelectServer(ServerId),
    Connect(ServerId),
    Disconnect(ServerId),
    /// The OSC port the user sets for a server whose port is inferred; `None` infers it again.
    SetOscPort(ServerId, Option<u16>),
    /// Drops a manual entry, its history line and its saved OSC port (PRD 9.1).
    RemoveManual(ServerId),
    /// Pins or unpins a path of the selected server (PRD 9.8).
    TogglePin(String),
    /// Captures the selected server into a preset slot, 1 to 16 (PRD 9.9).
    Capture(usize),
    /// Recalls a preset slot against the selected server.
    Recall(usize),
    Toggle(String),
    SelectPath(String),
    Send(SendReq, SendMode),
    RefreshValue(String),
    RefreshTree(String),
    Copy(String),
    Save {
        stem: &'static str,
        ext: &'static str,
        contents: String,
    },
}

#[derive(Default)]
struct RowsCache {
    key: Option<(ServerId, u64, u64, String, bool)>,
    rows: Vec<Row>,
}

#[derive(Default)]
struct FilterCache {
    key: Option<(ServerId, u64, String, bool)>,
    hits: Vec<Hit>,
    /// Names that two hits or more carry in different containers: only those show their path.
    ambiguous: HashSet<String>,
}

/// The paths the tree hides with writable nodes only, for one tree.
#[derive(Default)]
struct HiddenCache {
    key: Option<(ServerId, u64, bool)>,
    paths: HashSet<String>,
}

#[derive(Default)]
struct JsonCache {
    key: Option<(ServerId, u64, Option<String>)>,
    exact: String,
    lines: Vec<Line>,
    note: Option<String>,
}

#[derive(Default)]
struct View {
    tab: Tab,
    filter: String,
    prefix: String,
    selected_path: Option<String>,
    focus_filter: bool,
    focus_widget: bool,
    scroll_to_selected: bool,
    expanded_gen: u64,
    rows: RowsCache,
    hits: FilterCache,
    hidden: HiddenCache,
    json: JsonCache,
    text_buffers: std::collections::HashMap<String, String>,
    /// Paths with a drag send not yet followed by its final send.
    drag_open: std::collections::HashSet<String>,
    /// Methods on the rows drawn this frame, the visible part of the listen set (PRD 7.4).
    visible_methods: Vec<String>,
    traffic_filter: String,
    traffic_dir: traffic::DirFilter,
    /// Blocks of the traffic log shown open, by the time they were logged.
    traffic_open: std::collections::HashSet<Instant>,
    manual_open: bool,
    manual_text: String,
    manual_error: Option<String>,
    /// The OSC port field of the status bar, and the server and port it was last filled from.
    osc_port_text: String,
    osc_port_shown: Option<(ServerId, u16)>,
    about_open: bool,
    /// The macOS application menu's About item opens the about box.
    #[cfg(target_os = "macos")]
    about_menu_installed: bool,
    note: Option<String>,
    geometry_dirty_since: Option<Instant>,
}

struct Owned {
    runtime: tokio::runtime::Runtime,
    discovery: oscq_discovery::MdnsSdDiscovery,
}

pub struct VoidApp {
    state: AppState,
    backend: Backend,
    config: Config,
    config_dir: Option<PathBuf>,
    history: ManualHistory,
    view: View,
    limiter: RateLimiter<SendReq>,
    started: Instant,
    /// The wall clock time discovery began, as the server pane says it: "14:05".
    searching_since: String,
    io_tx: flume::Sender<String>,
    io_rx: flume::Receiver<String>,
    owned: Option<Owned>,
    /// When `main` started, until the first frame reports the startup time.
    launched: Option<Instant>,
}

impl VoidApp {
    /// The application as it runs: network service and discovery on `runtime`.
    pub fn start(
        launched: Instant,
        ctx: egui::Context,
        runtime: tokio::runtime::Runtime,
        config: Config,
        config_warning: Option<String>,
        config_dir: Option<PathBuf>,
    ) -> Self {
        use oscq_discovery::{Discovery, DiscoveryEvent, MdnsSdDiscovery};
        let (events_tx, events_rx) = flume::unbounded();
        let repaint = ctx.clone();
        let notify: oscq_core::Notify = Arc::new(move || repaint.request_repaint());
        let commands =
            oscq_core::service::spawn(runtime.handle(), events_tx.clone(), notify.clone());
        let mut discovery = MdnsSdDiscovery::default();
        let mut startup_error = config_warning;
        match discovery.start() {
            Ok(rx) => {
                runtime.spawn(async move {
                    while let Ok(e) = rx.recv_async().await {
                        let event = match e {
                            DiscoveryEvent::Found(s) => Event::ServerFound(s),
                            DiscoveryEvent::Changed(s) => Event::ServerChanged(s),
                            DiscoveryEvent::Stale(s) => Event::ServerStale(s),
                        };
                        if events_tx.send(event).is_err() {
                            break;
                        }
                        notify();
                    }
                });
            }
            Err(e) => startup_error = Some(format!("mDNS discovery unavailable: {e}")),
        }
        let backend = Backend {
            commands,
            events: events_rx,
            io: runtime.handle().clone(),
        };
        let mut app = Self::with_backend(&ctx, backend, config, config_dir);
        app.state.global_error = startup_error;
        app.owned = Some(Owned { runtime, discovery });
        app.launched = Some(launched);
        app
    }

    /// The application around a given backend, without discovery: used by `start` and tests.
    pub fn with_backend(
        ctx: &egui::Context,
        backend: Backend,
        config: Config,
        config_dir: Option<PathBuf>,
    ) -> Self {
        apply_theme(ctx, config.theme);
        style::apply(ctx);
        let history = ManualHistory::from_entries(
            config
                .manual_entries
                .iter()
                .filter_map(|s| ManualEntry::parse(s).ok()),
        );
        let (io_tx, io_rx) = flume::unbounded();
        let mut app = Self {
            state: AppState::default(),
            backend,
            config,
            config_dir,
            history,
            view: View::default(),
            limiter: RateLimiter::new(INTERVAL),
            started: Instant::now(),
            searching_since: clock_time(std::time::SystemTime::now()),
            io_tx,
            io_rx,
            owned: None,
            launched: None,
        };
        // Manual entries are known servers from the start.
        let entries: Vec<ManualEntry> = app.history.entries().cloned().collect();
        for e in entries.iter().rev() {
            app.state.add_manual(e.to_server());
        }
        // Restore the last selected server when it is a manual entry; a discovered one is
        // restored when discovery finds it.
        let last = app.config.last_selected.clone();
        if let Some(id) = app
            .state
            .servers
            .iter()
            .find(|e| Some(e.key()) == last)
            .map(|e| e.server.id)
        {
            app.act(ctx, Action::SelectServer(id));
        }
        app
    }

    /// One frame. Everything the window shows comes from here.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Why this frame runs: where the previous pass asked for it. Costs nothing when off.
        tracing::trace!(
            target: "void_oscq::frame",
            causes = ?ctx.repaint_causes(),
            inputs = ?input_kinds(&ctx),
            "frame"
        );
        if let Some(t) = self.launched.take() {
            tracing::info!(
                target: "void_oscq::startup",
                "first frame {:.1} ms after launch",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        #[cfg(target_os = "macos")]
        if !self.view.about_menu_installed {
            self.view.about_menu_installed = crate::macos_menu::install(&ctx);
        }
        if ctx.data_mut(|d| d.remove_temp::<bool>(about_request_id())) == Some(true) {
            self.view.about_open = true;
        }
        self.drain(&ctx);
        self.flush_limiter(&ctx);
        self.track_geometry(&ctx);
        while let Ok(note) = self.io_rx.try_recv() {
            self.view.note = Some(note);
        }

        let mut actions = Vec::new();
        self.view.visible_methods.clear();
        // The side panes take what the centre leaves, so the centre keeps CENTRE_MIN whatever
        // the window width and however the panes were dragged: the server pane first, leaving
        // the work pane its default width, so that it is the one to narrow first; then the work
        // pane, from what the server pane took.
        let width = ui.available_width();
        let servers_max = (width - style::CENTRE_MIN - style::WORK_DEFAULT).max(style::SERVERS_MIN);
        let servers = egui::Panel::left("servers")
            .resizable(true)
            .default_size(56.0 * style::GRID)
            .size_range(style::SERVERS_MIN..=servers_max)
            .show(ui, |ui| self.server_pane(ui, &mut actions))
            .response
            .rect
            .width();
        let work_max = (width - servers - style::CENTRE_MIN).max(style::WORK_MIN);
        egui::Panel::bottom("status")
            .exact_size(style::ROW_H + 2.0 * style::GRID)
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("work")
            .resizable(true)
            .default_size(style::WORK_DEFAULT)
            .size_range(style::WORK_MIN..=work_max)
            .show(ui, |ui| self.work_pane(ui, &mut actions));
        egui::CentralPanel::default().show(ui, |ui| self.main_pane(ui, &mut actions));
        self.manual_modal(&ctx, &mut actions);
        self.about_modal(&ctx);
        for a in actions {
            self.act(&ctx, a);
        }
        self.sync_listens();
    }

    /// Keeps each connection's listens equal to its visible and pinned methods: `LISTEN` when a
    /// method appears, `IGNORE` when it leaves, none for containers (PRD 7.4). Commands go out
    /// only on a difference, so an idle frame sends nothing.
    fn sync_listens(&mut self) {
        let selected = self.state.selected;
        for e in &mut self.state.servers {
            let Some(t) = e.tree.as_ref().filter(|_| e.conn == ConnState::Connected) else {
                continue;
            };
            let mut wanted: std::collections::HashSet<&str> = self
                .config
                .pinned_paths
                .get(&e.key())
                .into_iter()
                .flatten()
                .filter(|p| t.node(p).is_some_and(|n| n.is_method()))
                .map(String::as_str)
                .collect();
            if selected == Some(e.server.id) {
                wanted.extend(self.view.visible_methods.iter().map(String::as_str));
            }
            let id = e.server.id;
            let gone: Vec<String> = e
                .listening
                .iter()
                .filter(|p| !wanted.contains(p.as_str()))
                .cloned()
                .collect();
            for path in gone {
                e.listening.remove(&path);
                let _ = self.backend.commands.send(Command::Ignore {
                    server_id: id,
                    path,
                });
            }
            for path in wanted {
                if !e.listening.contains(path) {
                    e.listening.insert(path.to_owned());
                    let _ = self.backend.commands.send(Command::Listen {
                        server_id: id,
                        path: path.to_owned(),
                    });
                }
            }
        }
    }

    fn drain(&mut self, ctx: &egui::Context) {
        for _ in 0..EVENTS_PER_FRAME {
            let Ok(event) = self.backend.events.try_recv() else {
                return;
            };
            let restore = match &event {
                Event::ServerFound(s)
                    if self.state.selected.is_none()
                        && self.config.last_selected.as_deref()
                            == Some(crate::state::host_port(s).as_str()) =>
                {
                    Some(s.id)
                }
                _ => None,
            };
            if let Event::PathRenamed { old, new, .. } = &event {
                let moved = |p: &mut String| {
                    if let Some(n) = oscq_core::ws::renamed(p, old, new) {
                        *p = n;
                    }
                };
                if let Some(p) = self.view.selected_path.as_mut() {
                    moved(p);
                }
            }
            tracing::trace!(target: "void_oscq::event", kind = event_kind(&event));
            self.state.apply(event);
            if let Some(id) = restore {
                self.act(ctx, Action::SelectServer(id));
            }
        }
        // More waiting: come back next frame rather than block this one.
        ctx.request_repaint();
    }

    fn flush_limiter(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        for req in self.limiter.due(now) {
            self.dispatch(req, false);
        }
        if let Some(wait) = self.limiter.next_due(now) {
            ctx.request_repaint_after(wait);
        }
    }

    fn track_geometry(&mut self, ctx: &egui::Context) {
        let geometry = ctx.input(|i| {
            let v = i.viewport();
            match (v.outer_rect, v.inner_rect) {
                (Some(o), Some(inner)) => Some(WindowGeometry {
                    x: o.min.x,
                    y: o.min.y,
                    width: inner.width(),
                    height: inner.height(),
                }),
                _ => None,
            }
        });
        let Some(g) = geometry else {
            return;
        };
        if self.config.window == Some(g) {
            self.view.geometry_dirty_since = None;
            return;
        }
        let now = Instant::now();
        let since = *self.view.geometry_dirty_since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        if waited >= GEOMETRY_SETTLE {
            self.config.window = Some(g);
            self.view.geometry_dirty_since = None;
            self.save_config(ctx);
        } else {
            ctx.request_repaint_after(GEOMETRY_SETTLE - waited);
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::SelectServer(id) => {
                if self.state.selected != Some(id) {
                    self.state.selected = Some(id);
                    self.view.selected_path = None;
                }
                if let Some(e) = self.state.entry(id) {
                    self.config.last_selected = Some(e.key());
                    self.save_config(ctx);
                    let reconnect = matches!(
                        e.conn,
                        ConnState::Idle | ConnState::Disconnected | ConnState::Failed(_)
                    ) || (e.server.stale && e.conn != ConnState::Connecting);
                    if e.server.kind.connectable() && reconnect {
                        self.act(ctx, Action::Connect(id));
                    }
                }
            }
            Action::Connect(id) => {
                let Some(e) = self.state.entry_mut(id) else {
                    return;
                };
                match e.server.http_base() {
                    Some(url) => {
                        e.conn = ConnState::Connecting;
                        e.ws = WsState::Absent;
                        e.listening.clear();
                        // The port the user set, before the connection derives its destination.
                        if let Some(&port) = self.config.osc_ports.get(&e.key()) {
                            let _ = self.backend.commands.send(Command::SetOscPort {
                                server_id: id,
                                port: Some(port),
                            });
                        }
                        let _ = self.backend.commands.send(Command::Connect {
                            server_id: id,
                            http_base: url,
                        });
                    }
                    None => e.conn = ConnState::Failed("no usable address".to_owned()),
                }
            }
            Action::SetOscPort(id, port) => {
                let Some(key) = self.state.entry(id).map(|e| e.key()) else {
                    return;
                };
                match port {
                    Some(p) => self.config.osc_ports.insert(key, p),
                    None => self.config.osc_ports.remove(&key),
                };
                self.save_config(ctx);
                let _ = self.backend.commands.send(Command::SetOscPort {
                    server_id: id,
                    port,
                });
            }
            Action::RemoveManual(id) => {
                let Some(e) = self
                    .state
                    .entry(id)
                    .filter(|e| e.server.origin == Origin::Manual)
                else {
                    return;
                };
                let key = e.key();
                if let Ok(entry) = ManualEntry::parse(&key) {
                    self.history.forget(&entry);
                }
                self.config.manual_entries =
                    self.history.entries().map(ToString::to_string).collect();
                self.config.osc_ports.remove(&key);
                if self.config.last_selected.as_deref() == Some(key.as_str()) {
                    self.config.last_selected = None;
                }
                let _ = self
                    .backend
                    .commands
                    .send(Command::Disconnect { server_id: id });
                self.state.remove(id);
                if self.state.selected == Some(id) {
                    self.state.selected = None;
                    self.view.selected_path = None;
                }
                self.save_config(ctx);
            }
            Action::TogglePin(path) => {
                let Some(key) = self.state.selected_entry().map(|e| e.key()) else {
                    return;
                };
                let pins = self.config.pinned_paths.entry(key.clone()).or_default();
                match pins.iter().position(|p| *p == path) {
                    Some(i) => {
                        pins.remove(i);
                    }
                    None => pins.push(path),
                }
                if pins.is_empty() {
                    self.config.pinned_paths.remove(&key);
                }
                self.save_config(ctx);
            }
            Action::Capture(slot) => {
                let Some(e) = self.state.selected_entry() else {
                    return;
                };
                let Some(t) = e.tree.as_ref() else {
                    self.view.note = Some(format!("Slot {slot:02}: no tree to capture"));
                    return;
                };
                let key = e.key();
                let pinned = self
                    .config
                    .pinned_paths
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();
                let c = presets::capture(&key, &e.server.name, t, e.value_supported(), &pinned);
                let scope = if pinned.is_empty() { "" } else { " pinned" };
                let mut note = format!(
                    "Slot {slot:02}: {} values of{scope} methods captured",
                    c.preset.values.len()
                );
                if c.skipped > 0 {
                    note.push_str(&format!(", {} without a value skipped", c.skipped));
                }
                self.view.note = Some(note);
                self.config.presets.insert(slot, c.preset);
                self.save_config(ctx);
            }
            Action::Recall(slot) => self.recall(slot),
            Action::Disconnect(id) => {
                let _ = self
                    .backend
                    .commands
                    .send(Command::Disconnect { server_id: id });
            }
            Action::Toggle(path) => {
                let Some(id) = self.state.selected else {
                    return;
                };
                let Some(t) = self.state.entry_mut(id).and_then(|e| e.tree.as_mut()) else {
                    return;
                };
                self.view.expanded_gen += 1;
                if !t.expanded.remove(&path) {
                    t.expanded.insert(path.clone());
                    // Values are fetched on expand: one request for the whole subtree, which
                    // also loads a subtree the server did not include.
                    if t.fetched.insert(path.clone()) {
                        let _ = self.backend.commands.send(Command::RefreshTree {
                            server_id: id,
                            from_path: path,
                        });
                    }
                }
            }
            Action::SelectPath(p) => self.view.selected_path = Some(p),
            Action::Send(req, mode) => {
                let now = Instant::now();
                let key = req.path.as_str().to_owned();
                match mode {
                    SendMode::Drag => {
                        if let Some(r) = self.limiter.offer(now, &key, req.clone()) {
                            self.dispatch(r, false);
                        } else {
                            self.mark_pending(&req);
                        }
                    }
                    SendMode::Final => {
                        let r = self.limiter.finish(now, &key, req);
                        let refresh = r.readable && !self.value_pushed(&r);
                        self.dispatch(r, refresh);
                    }
                }
                if let Some(wait) = self.limiter.next_due(now) {
                    ctx.request_repaint_after(wait);
                }
            }
            Action::RefreshValue(path) => {
                if let Some(id) = self.state.selected {
                    let _ = self.backend.commands.send(Command::RefreshValue {
                        server_id: id,
                        path,
                    });
                }
            }
            Action::RefreshTree(path) => {
                if let Some(id) = self.state.selected {
                    let _ = self.backend.commands.send(Command::RefreshTree {
                        server_id: id,
                        from_path: path,
                    });
                }
            }
            Action::Copy(text) => ctx.copy_text(text),
            Action::Save {
                stem,
                ext,
                contents,
            } => self.write_export(ctx, stem, ext, contents),
        }
    }

    /// Whether the server pushes this method's value: listened to on a WebSocket that has
    /// notified. Then a refresh after a send is redundant (PRD 7.4 fetches after a send only
    /// without a WebSocket); on a socket that never sent anything (Max 9) it is still needed.
    fn value_pushed(&self, req: &SendReq) -> bool {
        self.state
            .entry(req.server_id)
            .is_some_and(|e| e.ws == WsState::Live && e.listening.contains(req.path.as_str()))
    }

    fn mark_pending(&mut self, req: &SendReq) {
        if let Some(t) = self
            .state
            .entry_mut(req.server_id)
            .and_then(|e| e.tree.as_mut())
        {
            t.pending
                .insert(req.path.as_str().to_owned(), req.args.clone());
        }
    }

    fn dispatch(&mut self, req: SendReq, refresh_after: bool) {
        self.mark_pending(&req);
        let _ = self.backend.commands.send(Command::SendOsc {
            server_id: req.server_id,
            path: req.path,
            signature: req.signature,
            args: req.args,
            refresh_after,
        });
    }

    /// Sends a preset slot to the selected server at once, as one block (PRD 9.9).
    fn recall(&mut self, slot: usize) {
        let Some(preset) = self.config.presets.get(&slot).cloned() else {
            return;
        };
        let Some(e) = self.state.selected_entry() else {
            return;
        };
        let id = e.server.id;
        if let Some(reason) = e.send_blocked() {
            self.view.note = Some(format!(
                "Slot {slot:02} not recalled: cannot send: {reason}"
            ));
            return;
        }
        let Some(t) = e.tree.as_ref() else {
            return;
        };
        let other = (preset.server != e.key()).then(|| preset.server_name.clone());
        let r = presets::recall(&preset, t, e.value_supported());
        let mut messages = Vec::with_capacity(r.messages.len());
        for m in r.messages {
            let req = SendReq {
                server_id: id,
                path: m.path,
                signature: m.signature,
                args: m.args,
                readable: m.readable,
            };
            let refresh_after = req.readable && !self.value_pushed(&req);
            self.mark_pending(&req);
            messages.push(oscq_core::BlockMessage {
                path: req.path,
                signature: req.signature,
                args: req.args,
                refresh_after,
            });
        }
        let mut note = format!("Slot {slot:02} recalled: {} sent", messages.len());
        if r.unmatched > 0 {
            note.push_str(&format!(", {} not found on this server", r.unmatched));
        }
        if let Some(name) = other {
            note.push_str(&format!(" (captured on {name})"));
        }
        self.view.note = Some(note);
        if !messages.is_empty() {
            let _ = self.backend.commands.send(Command::SendBlock {
                server_id: id,
                label: format!("preset {slot:02}"),
                messages,
            });
        }
    }

    fn save_config(&self, ctx: &egui::Context) {
        let Some(dir) = self.config_dir.clone() else {
            return;
        };
        let bytes = self.config.to_bytes();
        let tx = self.io_tx.clone();
        let ctx = ctx.clone();
        self.backend.io.spawn_blocking(move || {
            let path = config::config_path(&dir);
            if let Err(e) = config::write_atomic(&path, &bytes) {
                let _ = tx.send(format!("config not saved to {}: {e}", path.display()));
                ctx.request_repaint();
            }
        });
    }

    fn write_export(&self, ctx: &egui::Context, stem: &str, ext: &str, contents: String) {
        let Some(dir) = self.config_dir.clone() else {
            let _ = self
                .io_tx
                .send("no config directory to export to".to_owned());
            return;
        };
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join("exports").join(format!("{stem}-{secs}.{ext}"));
        let tx = self.io_tx.clone();
        let ctx = ctx.clone();
        self.backend.io.spawn_blocking(move || {
            let msg = match config::write_atomic(&path, contents.as_bytes()) {
                Ok(()) => format!("saved {}", path.display()),
                Err(e) => format!("not saved to {}: {e}", path.display()),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    // ---------------------------------------------------------------- server pane

    fn server_pane(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        // As wide as the pane, whatever it holds: see `work_pane`.
        ui.set_min_width(ui.available_width());
        // The footer first, so the list above takes what remains.
        egui::Panel::bottom("server_footer")
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.add_space(GRID);
                // Wrapped, so About stays on screen in the narrowest pane.
                ui.horizontal_wrapped(|ui| {
                    let mut theme = self.config.theme;
                    egui::ComboBox::from_id_salt("theme")
                        .width(24.0 * GRID)
                        .selected_text(match theme {
                            Theme::System => "Theme: system",
                            Theme::Dark => "Theme: dark",
                            Theme::Light => "Theme: light",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut theme, Theme::System, "System");
                            ui.selectable_value(&mut theme, Theme::Dark, "Dark");
                            ui.selectable_value(&mut theme, Theme::Light, "Light");
                        });
                    if theme != self.config.theme {
                        self.config.theme = theme;
                        apply_theme(ui.ctx(), theme);
                        self.save_config(ui.ctx());
                    }
                    if ui.button("About").clicked() {
                        request_about(ui.ctx());
                    }
                });
                ui.add_space(GRID);
            });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(GRID);
                let (manual, discovered): (Vec<&ServerEntry>, Vec<&ServerEntry>) = self
                    .state
                    .servers
                    .iter()
                    .partition(|e| e.server.origin == Origin::Manual);
                style::section(ui, "Discovered");
                if discovered.is_empty() {
                    let elapsed = self.started.elapsed();
                    if elapsed < DISCOVERY_HINT_AFTER {
                        ui.weak("Searching the network");
                        ui.ctx()
                            .request_repaint_after(DISCOVERY_HINT_AFTER - elapsed);
                    } else {
                        // A time, not a running count: a count would repaint the window every
                        // second while it is at rest.
                        ui.weak(format!(
                            "Searching since {}, none found. Use + manual for a host the \
                             network does not show.",
                            self.searching_since
                        ));
                    }
                }
                for e in discovered {
                    server_row(ui, e, self.state.selected, actions);
                }
                ui.add_space(GROUP_GAP);
                style::section(ui, "Manual");
                for e in manual {
                    server_row(ui, e, self.state.selected, actions);
                }
                if ui.button("+ manual").clicked() {
                    self.view.manual_open = true;
                    self.view.manual_error = None;
                }
                if let Some(e) = self.state.selected_entry() {
                    if e.conn == ConnState::Connected && ui.button("Disconnect").clicked() {
                        actions.push(Action::Disconnect(e.server.id));
                    }
                    ui.add_space(GROUP_GAP);
                    style::section(ui, "OSC destination");
                    match &e.destination {
                        Some(Ok(d)) => {
                            let transport = match d.transport {
                                oscq_core::OscTransport::Udp => "UDP",
                                oscq_core::OscTransport::Tcp => "TCP",
                            };
                            ui.label(style::data(format!("{}:{} {transport}", d.host, d.port)));
                            ui.weak(port_source_text(d.port_source));
                            if d.port_source != PortSource::Advertised {
                                osc_port_editor(ui, e.server.id, d, &mut self.view, actions);
                            }
                        }
                        Some(Err(blocked)) => {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                format!("{CANNOT_SEND}: {blocked}"),
                            );
                        }
                        None => {
                            ui.weak("not known yet");
                        }
                    }
                }
            });
    }

    /// The working surface, whatever the tab: the pinned nodes, then the presets.
    fn work_pane(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        // A side pane is drawn as wide as its content: without this, a pane holding little
        // would be painted narrower than it was dragged, the rest of the window left blank, then
        // spring back on release.
        ui.set_min_width(ui.available_width());
        ui.add_space(GRID);
        style::section(ui, "Pinned");
        let entry = self.state.selected_entry();
        let pins: Vec<String> = entry
            .and_then(|e| self.config.pinned_paths.get(&e.key()))
            .cloned()
            .unwrap_or_default();
        match entry {
            None => {
                ui.weak("Select a server.");
            }
            Some(_) if pins.is_empty() => {
                ui.weak("Right click a node in the tree to pin it here.");
            }
            Some(e) => {
                // What the preset section takes below, open or folded to its header.
                let presets = if self.config.presets_open {
                    64.0 * GRID
                } else {
                    8.0 * GRID
                };
                let height = (ui.available_height() - presets).max(4.0 * ROW_H);
                egui::ScrollArea::vertical()
                    .id_salt("pinned")
                    .max_height(height)
                    .auto_shrink([false, true])
                    .show_rows(ui, ROW_H, pins.len(), |ui, range| {
                        for path in &pins[range] {
                            pinned_row(ui, e, path, &mut self.view, actions);
                        }
                    });
            }
        }
        ui.add_space(GROUP_GAP);
        // Folded until opened: open, the sixteen slots take a third of the pane.
        let open = self.config.presets_open;
        if ui
            .add(
                egui::Button::new(
                    RichText::new(if open { "- PRESETS" } else { "+ PRESETS" })
                        .small()
                        .weak(),
                )
                .frame(false),
            )
            .on_hover_text(if open {
                "Fold the preset slots"
            } else {
                "Show the sixteen preset slots"
            })
            .clicked()
        {
            self.config.presets_open = !open;
            self.save_config(ui.ctx());
        }
        if !open {
            return;
        }
        let scope = if pins.is_empty() {
            "every writable node"
        } else {
            "the pinned nodes"
        };
        // Each slot names its two actions, Recall and Capture, and shows whether it holds
        // anything: a held slot on a tinted ground, its number plain, Recall on; an empty slot,
        // its number faint, Recall off. Rows of as many slots as the pane holds, two at most.
        const NUMBERS: [&str; presets::SLOTS] = [
            "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14",
            "15", "16",
        ];
        let (number_w, recall_w, capture_w) = (6.0 * GRID, 12.0 * GRID, 14.0 * GRID);
        let cell = number_w + recall_w + capture_w;
        let cols = (((ui.available_width() + GRID) / (cell + GRID)).floor() as usize).clamp(1, 2);
        let held_fill = ui.visuals().selection.bg_fill.gamma_multiply(0.35);
        for (r, numbers) in NUMBERS.chunks(cols).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GRID;
                for (c, number) in numbers.iter().enumerate() {
                    let slot = r * cols + c + 1;
                    let held = self.config.presets.get(&slot);
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(cell, style::CONTROL_H),
                        egui::Sense::hover(),
                    );
                    if held.is_some() {
                        ui.painter().rect_filled(rect, 2.0, held_fill);
                    }
                    let ui = &mut ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let n = style::data(*number);
                    // Hover texts are built only when shown: a frame builds none.
                    ui.add_sized(
                        egui::vec2(number_w, style::CONTROL_H),
                        egui::Label::new(if held.is_some() { n } else { n.weak() }),
                    )
                    .on_hover_ui(|ui| {
                        ui.label(match held {
                            Some(p) => format!(
                                "Slot {number}: {} values, captured on {}",
                                p.values.len(),
                                p.server_name
                            ),
                            None => format!("Slot {number}: empty"),
                        });
                    });
                    let recall = ui
                        .add_enabled(
                            held.is_some(),
                            egui::Button::new(RichText::new("Recall").small())
                                .min_size(egui::vec2(recall_w, style::CONTROL_H)),
                        )
                        .on_hover_ui(|ui| {
                            if let Some(p) = held {
                                ui.label(format!(
                                    "Send its {} values to this server",
                                    p.values.len()
                                ));
                            }
                        })
                        .on_disabled_hover_text("Empty: Capture first");
                    if recall.clicked() {
                        actions.push(Action::Recall(slot));
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Capture").small())
                                .min_size(egui::vec2(capture_w, style::CONTROL_H)),
                        )
                        .on_hover_ui(|ui| {
                            ui.label(format!(
                                "Store {scope} of this server in this slot{}",
                                if held.is_some() {
                                    ", replacing what it holds"
                                } else {
                                    ""
                                }
                            ));
                        })
                        .clicked()
                    {
                        actions.push(Action::Capture(slot));
                    }
                }
            });
        }
    }

    fn manual_modal(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.view.manual_open {
            return;
        }
        let mut connect = None;
        let response = egui::Modal::new(egui::Id::new("manual")).show(ctx, |ui| {
            ui.heading("Connect to host:port");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.view.manual_text)
                    .hint_text("192.168.1.20:2345 or [fe80::1]:2345")
                    .desired_width(260.0),
            );
            edit.request_focus();
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if let Some(err) = &self.view.manual_error {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            if !self.history.is_empty() {
                ui.label("Recent");
                for e in self.history.entries() {
                    if ui.button(e.to_string()).clicked() {
                        self.view.manual_text = e.to_string();
                    }
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Connect").clicked() || enter {
                    connect = Some(self.view.manual_text.clone());
                }
                if ui.button("Cancel").clicked() {
                    self.view.manual_open = false;
                }
            });
        });
        if response.should_close() {
            self.view.manual_open = false;
        }
        if let Some(text) = connect {
            match ManualEntry::parse(&text) {
                Ok(entry) => {
                    self.history.remember(entry.clone());
                    self.config.manual_entries =
                        self.history.entries().map(ToString::to_string).collect();
                    let id = self.state.add_manual(entry.to_server());
                    self.view.manual_open = false;
                    self.view.manual_text.clear();
                    actions.push(Action::SelectServer(id));
                    actions.push(Action::Connect(id));
                }
                Err(e) => self.view.manual_error = Some(e.to_string()),
            }
        }
    }

    fn about_modal(&mut self, ctx: &egui::Context) {
        if !self.view.about_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
            for line in about_lines() {
                match line {
                    AboutLine::Title(t) => {
                        ui.heading(t);
                    }
                    AboutLine::Text(t) => {
                        ui.label(t);
                    }
                    AboutLine::Link(text, url) => {
                        ui.hyperlink_to(style::data(text), url);
                    }
                }
            }
            ui.add_space(style::ITEM_GAP);
            if ui.button("Close").clicked() {
                self.view.about_open = false;
            }
        });
        if response.should_close() {
            self.view.about_open = false;
        }
    }

    // ---------------------------------------------------------------- status bar

    /// One line in fixed zones: state, server, transport, node count, then the message (PRD 9.1).
    /// Each zone is short; the details are on hover.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let zone = |ui: &mut egui::Ui, width: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    add(ui);
                },
            );
        };
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = GROUP_GAP;
            let Some(e) = self.state.selected_entry() else {
                ui.weak("No server selected");
                if let Some(n) = &self.view.note {
                    ui.add(egui::Label::new(RichText::new(n).weak()).truncate());
                }
                return;
            };
            // The message: the last error, or why nothing can be sent, or a note.
            let message = error_text(e)
                .map(|t| (t, ui.visuals().error_fg_color))
                .or_else(|| match &e.conn {
                    ConnState::Failed(r) => Some((r.clone(), ui.visuals().error_fg_color)),
                    _ => None,
                })
                .or_else(|| {
                    e.send_blocked()
                        .filter(|_| e.conn == ConnState::Connected)
                        .map(|r| (format!("not controllable: {r}"), ui.visuals().warn_fg_color))
                })
                .or_else(|| match values_text(e) {
                    Some((t, true)) => Some((t, ui.visuals().warn_fg_color)),
                    _ => None,
                })
                .or_else(|| {
                    self.state
                        .global_error
                        .clone()
                        .map(|g| (g, ui.visuals().error_fg_color))
                })
                .or_else(|| {
                    self.view
                        .note
                        .clone()
                        .map(|n| (n, ui.visuals().weak_text_color()))
                });
            // The zones narrow together when the window does, the message keeping some room:
            // nothing runs past the edge of the window, the details stay on hover.
            let gaps = 4.0 * GROUP_GAP;
            let message_floor = if message.is_some() { 32.0 * GRID } else { 0.0 };
            let scale =
                ((ui.available_width() - gaps - message_floor) / (168.0 * GRID)).clamp(0.0, 1.0);
            let hidden = if self.config.hide_read_only {
                Some(hidden_paths(&mut self.view.hidden, e).len())
            } else {
                None
            };
            zone(ui, 36.0 * GRID * scale, &mut |ui| {
                ui.spacing_mut().item_spacing.x = GRID;
                status_dot(ui, dot_colour(e));
                ui.add(egui::Label::new(state_text(e)).truncate())
                    .on_hover_ui(|ui| {
                        ui.label(conn_text(e));
                        if let Some((v, _)) = values_text(e) {
                            ui.label(v);
                        }
                    });
            });
            zone(ui, 40.0 * GRID * scale, &mut |ui| {
                ui.add(egui::Label::new(RichText::new(&e.server.name).strong()).truncate())
                    .on_hover_ui(|ui| {
                        ui.label(server_tooltip(e));
                    });
            });
            zone(ui, 52.0 * GRID * scale, &mut |ui| {
                let text = match &e.destination {
                    Some(Ok(d)) => {
                        let transport = match d.transport {
                            oscq_core::OscTransport::Udp => "UDP",
                            oscq_core::OscTransport::Tcp => "TCP",
                        };
                        format!("OSC {}:{} {transport}", d.host, d.port)
                    }
                    Some(Err(_)) => "OSC: cannot send".to_owned(),
                    None => "OSC: not known yet".to_owned(),
                };
                ui.add(egui::Label::new(style::data(text)).truncate())
                    .on_hover_ui(|ui| match &e.destination {
                        Some(Ok(d)) => {
                            ui.label(port_source_text(d.port_source));
                            if e.host_info.as_ref().is_some_and(|h| h.query_ignored) {
                                ui.label(
                                    "HOST_INFO ignored by the server, defaults used: OSC and \
                                     WebSocket on the HTTP address and port",
                                );
                            }
                        }
                        Some(Err(blocked)) => {
                            ui.label(blocked.to_string());
                        }
                        None => {}
                    });
            });
            zone(ui, 40.0 * GRID * scale, &mut |ui| {
                if let Some(t) = &e.tree {
                    let mut text = format!("{} nodes", t.nodes.saturating_sub(1));
                    if let Some(n) = hidden {
                        text.push_str(&format!(", {n} hidden"));
                    }
                    if !t.diagnostics.is_empty() {
                        text.push_str(&format!(", {} warnings", t.diagnostics.len()));
                    }
                    let label = ui.add(egui::Label::new(text).truncate());
                    if let Some(n) = hidden {
                        label.on_hover_ui(|ui| {
                            ui.label(format!(
                                "{n} nodes hidden: read only, or containers holding only read \
                                 only nodes. Unchecking Writable, in the toolbar, shows them again."
                            ));
                        });
                    }
                }
            });
            if let Some((text, colour)) = message {
                ui.add(egui::Label::new(RichText::new(&text).color(colour)).truncate())
                    .on_hover_text(text);
            }
        });
    }

    // ---------------------------------------------------------------- main pane

    fn main_pane(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(id) = self.state.selected else {
            ui.centered_and_justified(|ui| ui.weak("Select a server, or use + manual"));
            return;
        };
        ui.add_space(GRID);
        // The tabs and Refresh first, then the filter and the root fields sharing what the line
        // has left. The fields go to a line of their own only when that is less than their
        // least widths: on a wide window everything holds on one line; on a narrow one nothing
        // runs under the right pane.
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(ROW_H);
            let width = ui.available_width();
            ui.selectable_value(&mut self.view.tab, Tab::Tree, "Tree");
            ui.selectable_value(&mut self.view.tab, Tab::Json, "JSON");
            ui.selectable_value(&mut self.view.tab, Tab::Traffic, "Traffic");
            // Groups apart by GROUP_GAP, the item spacing included.
            let gap = ui.spacing().item_spacing.x;
            ui.add_space(GROUP_GAP - gap);
            if ui
                .button("Refresh")
                .on_hover_text("Refresh the selected node, or the whole tree (F5)")
                .clicked()
            {
                actions.push(self.refresh_action());
            }
            ui.add_space(GROUP_GAP - gap);
            let mut hide = self.config.hide_read_only;
            if ui
                .checkbox(&mut hide, "Writable")
                .on_hover_text(
                    "Hide the nodes that cannot be written, and the containers left with none",
                )
                .changed()
            {
                self.config.hide_read_only = hide;
                self.save_config(ui.ctx());
            }
            ui.add_space(GROUP_GAP - gap);
            let (filter_min, root_min) = (22.0 * GRID, 18.0 * GRID);
            // In a wrapping layout `available_width` is the whole line's: what is left of it is
            // from the cursor to the right edge.
            let mut room = ui.max_rect().right() - ui.cursor().min.x;
            if room < filter_min + gap + root_min + GRID {
                ui.end_row();
                room = width;
            }
            // Three parts to the filter, two to the root, each within its least and most width.
            // A grid step of slack: two fields filling the line exactly would wrap on rounding.
            let share = (room - gap - GRID).max(0.0).floor();
            let filter_w = (share * 0.6).clamp(filter_min.min(share), 64.0 * GRID);
            let root_w = (share - filter_w).clamp(root_min.min(share), 48.0 * GRID);
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.view.filter)
                    .id(egui::Id::new("filter"))
                    .font(egui::TextStyle::Monospace)
                    .hint_text("filter  /")
                    .desired_width(filter_w),
            );
            if std::mem::take(&mut self.view.focus_filter) {
                filter.request_focus();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.view.prefix)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("root at path")
                    .desired_width(root_w),
            );
        });
        ui.add_space(style::ITEM_GAP);
        let Some(entry) = self.state.entry(id) else {
            return;
        };
        if entry.tree.is_none() {
            ui.label(conn_text(entry));
            if matches!(
                entry.conn,
                ConnState::Failed(_) | ConnState::Disconnected | ConnState::Idle
            ) && entry.server.kind.connectable()
                && ui.button("Retry").clicked()
            {
                actions.push(Action::Connect(id));
            }
            return;
        }
        match self.view.tab {
            Tab::Tree => {
                let pins = self
                    .config
                    .pinned_paths
                    .get(&entry.key())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                tree_tab(
                    ui,
                    entry,
                    pins,
                    self.config.hide_read_only,
                    &mut self.view,
                    actions,
                )
            }
            Tab::Json => json_tab(ui, entry, &mut self.view, actions),
            Tab::Traffic => {
                let started = self.started;
                if let Some(e) = self.state.entry_mut(id) {
                    traffic_tab(ui, e, &mut self.view, started, actions);
                }
            }
        }
    }

    fn refresh_action(&self) -> Action {
        let entry = self.state.selected_entry();
        let node = entry
            .and_then(|e| e.tree.as_ref())
            .zip(self.view.selected_path.as_ref())
            .and_then(|(t, p)| t.root.find(p));
        match node {
            Some(n) if n.is_container() => Action::RefreshTree(n.full_path.clone()),
            Some(n) => Action::RefreshValue(n.full_path.clone()),
            None => Action::RefreshTree("/".to_owned()),
        }
    }
}

impl eframe::App for VoidApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

impl Drop for VoidApp {
    fn drop(&mut self) {
        let _ = self.backend.commands.send(Command::Shutdown);
        if let Some(mut owned) = self.owned.take() {
            use oscq_discovery::Discovery;
            owned.discovery.stop();
            owned.runtime.shutdown_background();
        }
    }
}

/// The kinds of the raw input events of this frame, `PointerMoved` and so on, for the frame trace.
fn input_kinds(ctx: &egui::Context) -> Vec<String> {
    ctx.input(|i| {
        i.raw
            .events
            .iter()
            .map(|e| {
                let d = format!("{e:?}");
                d.split(['(', ' ', '{'])
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    })
}

/// The variant of an event, for the trace that tells what keeps waking the window.
fn event_kind(e: &Event) -> &'static str {
    match e {
        Event::ServerFound(_) => "ServerFound",
        Event::ServerChanged(_) => "ServerChanged",
        Event::ServerStale(_) => "ServerStale",
        Event::HostInfo { .. } => "HostInfo",
        Event::HostInfoUnavailable { .. } => "HostInfoUnavailable",
        Event::TreeLoaded { .. } => "TreeLoaded",
        Event::SubtreeLoaded { .. } => "SubtreeLoaded",
        Event::ValueChanged { .. } => "ValueChanged",
        Event::PathAdded { .. } => "PathAdded",
        Event::PathRemoved { .. } => "PathRemoved",
        Event::PathRenamed { .. } => "PathRenamed",
        Event::TransportState { .. } => "TransportState",
        Event::TrafficLogged(_) => "TrafficLogged",
        Event::Error { .. } => "Error",
        Event::Succeeded { .. } => "Succeeded",
    }
}

/// Where the OSC port comes from, as the status bar says it.
fn port_source_text(source: PortSource) -> &'static str {
    match source {
        PortSource::Advertised => "port advertised",
        PortSource::InferredFromHttp => "port inferred from the HTTP port",
        PortSource::User => "port set by the user",
    }
}

/// The field to set the OSC port of a server that does not advertise one, and the button that
/// infers it again once set.
fn osc_port_editor(
    ui: &mut egui::Ui,
    id: ServerId,
    d: &oscq_core::OscDestination,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    if view.osc_port_shown != Some((id, d.port)) {
        view.osc_port_shown = Some((id, d.port));
        view.osc_port_text = d.port.to_string();
    }
    let field = ui
        .add(egui::TextEdit::singleline(&mut view.osc_port_text).desired_width(48.0))
        .on_hover_text(
            "The server does not advertise its OSC port. Set the port it receives OSC on; \
             it is kept for this server.",
        );
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if ui.small_button("Set OSC port").clicked() || enter {
        match view.osc_port_text.trim().parse::<u16>() {
            Ok(p) if p > 0 => actions.push(Action::SetOscPort(id, Some(p))),
            _ => view.note = Some(format!("not a port: {}", view.osc_port_text.trim())),
        }
    }
    if d.port_source == PortSource::User && ui.small_button("Infer OSC port").clicked() {
        actions.push(Action::SetOscPort(id, None));
    }
}

/// What a row or the status bar says of a writable node nothing can be sent to.
const CANNOT_SEND: &str = "cannot send";

/// The error the status bar shows for a server, with the path it concerns. A failed connection
/// is already told by the connection state, and is not said twice.
fn error_text(e: &ServerEntry) -> Option<String> {
    let err = e.last_error.as_ref()?;
    if err.operation == oscq_core::Operation::Connect && matches!(e.conn, ConnState::Failed(_)) {
        return None;
    }
    Some(match &err.path {
        Some(p) => format!("{p}: {}", err.message),
        None => err.message.clone(),
    })
}

fn about_request_id() -> egui::Id {
    egui::Id::new("about_requested")
}

/// Opens the about box on the next frame of `ctx`. The macOS application menu's About item calls
/// it, outside any frame.
pub fn request_about(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(about_request_id(), true));
    ctx.request_repaint();
}

/// What the about box says, from the manifest so it cannot drift from the release: the name, the
/// version, the author and his studio, the license when one is declared, the repository.
enum AboutLine {
    Title(&'static str),
    Text(String),
    Link(String, &'static str),
}

fn about_lines() -> Vec<AboutLine> {
    let mut lines = vec![
        AboutLine::Title("Void OSCQ"),
        AboutLine::Text(format!("version {}", env!("CARGO_PKG_VERSION"))),
        AboutLine::Text(format!("{}, Structure Void", env!("CARGO_PKG_AUTHORS"))),
    ];
    let license = env!("CARGO_PKG_LICENSE");
    if !license.is_empty() {
        lines.push(AboutLine::Text(format!("License {license}")));
    }
    let repository = env!("CARGO_PKG_REPOSITORY");
    lines.push(AboutLine::Link(
        repository.trim_start_matches("https://").to_owned(),
        repository,
    ));
    lines
}

/// The offset of local time from UTC, read once by `capture_local_offset`.
static LOCAL_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

/// Reads the local time offset. On Linux and macOS the system gives it only to a process with a
/// single thread: call this first thing in `main`, before any thread starts.
pub fn capture_local_offset() {
    if let Ok(offset) = time::UtcOffset::current_local_offset() {
        let _ = LOCAL_OFFSET.set(offset);
    }
}

/// Hours and minutes of `at`, in local time, or in UTC, said so, when the local offset is unknown.
fn clock_time(at: std::time::SystemTime) -> String {
    let utc = time::OffsetDateTime::from(at);
    match LOCAL_OFFSET.get() {
        Some(offset) => {
            let t = utc.to_offset(*offset);
            format!("{:02}:{:02}", t.hour(), t.minute())
        }
        None => format!("{:02}:{:02} UTC", utc.hour(), utc.minute()),
    }
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::Light => egui::ThemePreference::Light,
    });
}

fn dot_colour(e: &ServerEntry) -> Color32 {
    if e.server.kind == ServiceKind::OscOnly {
        return Color32::from_gray(110);
    }
    if e.server.stale {
        return Color32::from_rgb(200, 140, 40);
    }
    match e.conn {
        ConnState::Idle | ConnState::Disconnected => Color32::from_gray(150),
        ConnState::Connecting => Color32::from_rgb(220, 200, 60),
        ConnState::Connected if e.ws == WsState::Live => Color32::from_rgb(60, 150, 230),
        ConnState::Connected if e.ws == WsState::Open => Color32::from_rgb(120, 170, 160),
        ConnState::Connected => Color32::from_rgb(60, 180, 90),
        ConnState::Failed(_) => Color32::from_rgb(210, 60, 60),
    }
}

/// Where the values shown come from, as observed (PRD 9.6), and whether it is a warning. An open
/// WebSocket is not claimed live until the server has sent something on it.
fn values_text(e: &ServerEntry) -> Option<(String, bool)> {
    if e.conn != ConnState::Connected {
        return None;
    }
    Some(match &e.ws {
        WsState::Live => ("values live over the WebSocket".to_owned(), false),
        WsState::Open => (
            "WebSocket connected, no update received yet: values on refresh only".to_owned(),
            false,
        ),
        WsState::Absent => ("values on refresh only".to_owned(), false),
        WsState::Lost { retry_in, reason } => (
            format!(
                "values on refresh only: WebSocket lost ({reason}), retry in {:.1} s",
                retry_in.as_secs_f64()
            ),
            true,
        ),
    })
}

/// The connection state in a word or two, for the status bar; `conn_text` on hover.
fn state_text(e: &ServerEntry) -> &'static str {
    if e.server.stale {
        return "stale";
    }
    match &e.conn {
        ConnState::Idle => "discovered",
        ConnState::Connecting => "connecting",
        ConnState::Connected => match e.ws {
            WsState::Live => "live",
            WsState::Open => "connected, silent",
            WsState::Absent => "connected",
            WsState::Lost { .. } => "connected, socket lost",
        },
        ConnState::Failed(_) => "error",
        ConnState::Disconnected => "disconnected",
    }
}

/// A server in the list: its dot and name; a manual entry has its remove button. A discovered
/// server has none: it would come back with the next announcement.
fn server_row(
    ui: &mut egui::Ui,
    e: &ServerEntry,
    selected: Option<ServerId>,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_H);
        ui.spacing_mut().item_spacing.x = GRID;
        status_dot(ui, dot_colour(e));
        let mut text = RichText::new(&e.server.name);
        if e.server.kind == ServiceKind::OscOnly || e.server.stale {
            text = text.weak();
        }
        let manual = e.server.origin == Origin::Manual;
        // The remove button and the gap before it: reserved, or the panel grows every frame.
        let remove_w = if manual { 8.0 * GRID } else { 0.0 };
        let name_w = (ui.available_width() - remove_w).max(10.0 * GRID);
        let label = if e.server.kind == ServiceKind::OscOnly {
            ui.add_enabled(
                false,
                egui::Button::selectable(false, text)
                    .right_text("")
                    .truncate()
                    .min_size(egui::vec2(name_w, style::CONTROL_H)),
            )
            .on_disabled_hover_text("OSC only, no query support")
        } else {
            ui.add(
                egui::Button::selectable(selected == Some(e.server.id), text)
                    .right_text("")
                    .truncate()
                    .min_size(egui::vec2(name_w, style::CONTROL_H)),
            )
        };
        if label
            .on_hover_ui(|ui| {
                ui.label(server_tooltip(e));
            })
            .clicked()
        {
            actions.push(Action::SelectServer(e.server.id));
        }
        if manual
            && ui
                .small_button("x")
                .on_hover_text("Remove this manual entry and its saved OSC port")
                .clicked()
        {
            actions.push(Action::RemoveManual(e.server.id));
        }
    });
}

fn status_dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, colour);
}

fn conn_text(e: &ServerEntry) -> String {
    let mut s = match &e.conn {
        ConnState::Idle => "discovered".to_owned(),
        ConnState::Connecting => "connecting".to_owned(),
        ConnState::Connected => match e.ws {
            WsState::Live => "connected, WebSocket live".to_owned(),
            WsState::Open => "connected, WebSocket open, nothing received yet".to_owned(),
            WsState::Absent => "connected over HTTP".to_owned(),
            WsState::Lost { .. } => "connected over HTTP, WebSocket lost".to_owned(),
        },
        ConnState::Failed(r) => format!("error: {r}"),
        ConnState::Disconnected => "disconnected".to_owned(),
    };
    if e.server.stale {
        s.push_str(", stale: no longer advertised, select to retry");
    }
    s
}

fn server_tooltip(e: &ServerEntry) -> String {
    let mut t = format!("{}\n{}", e.key(), conn_text(e));
    for a in &e.server.addresses {
        let ifaces: Vec<String> = a
            .interfaces
            .iter()
            .map(|i| format!("{}#{}", i.name, i.index))
            .collect();
        t.push_str(&format!("\n{} {:?} {}", a.ip, a.class, ifaces.join(", ")));
    }
    t
}

// -------------------------------------------------------------------- tree tab

/// Full paths the tree hides when it shows writable nodes only: the methods that cannot be
/// written, and the containers whose children are all hidden. A container with no child known,
/// not loaded yet, stays: what it holds is not known to be read only.
fn read_only_paths(root: &oscq_core::model::Node, value_supported: bool) -> HashSet<String> {
    let mut hidden = HashSet::new();
    // Children before their parent, without recursion: the depth of a tree costs no stack.
    let mut stack = vec![(root, false)];
    while let Some((node, children_done)) = stack.pop() {
        if !children_done {
            stack.push((node, true));
            stack.extend(node.children().map(|c| (c, false)));
            continue;
        }
        let writable = node.is_method() && node.effective_access(value_supported).writable();
        let mut children = node.children().peekable();
        let hide = if children.peek().is_some() {
            !writable && children.all(|c| hidden.contains(&c.full_path))
        } else {
            node.is_method() && !writable
        };
        if hide && node.full_path != "/" {
            hidden.insert(node.full_path.clone());
        }
    }
    hidden
}

/// The names that hits carry in two containers or more: the hits that need their container shown
/// to be told apart. Hits of names found once, or in one container, show their name alone.
fn ambiguous_names(root: &oscq_core::model::Node, hits: &[Hit]) -> HashSet<String> {
    let mut parents: HashMap<&str, HashSet<&str>> = HashMap::new();
    for h in hits {
        if let Some(n) = tree::node_at(root, &h.index) {
            let (parent, name) = n.full_path.rsplit_once('/').unwrap_or(("", &n.full_path));
            parents.entry(name).or_default().insert(parent);
        }
    }
    parents
        .into_iter()
        .filter(|(_, p)| p.len() > 1)
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// The paths hidden in the tree of `entry`, computed once per tree.
fn hidden_paths<'a>(cache: &'a mut HiddenCache, entry: &ServerEntry) -> &'a HashSet<String> {
    let value_supported = entry.value_supported();
    match entry.tree.as_ref() {
        Some(t) => {
            let key = (entry.server.id, t.version, value_supported);
            if cache.key != Some(key) {
                cache.paths = read_only_paths(&t.root, value_supported);
                cache.key = Some(key);
            }
        }
        None => {
            cache.paths.clear();
            cache.key = None;
        }
    }
    &cache.paths
}

fn tree_tab(
    ui: &mut egui::Ui,
    entry: &ServerEntry,
    pinned: &[String],
    hide_read_only: bool,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    let Some(t) = entry.tree.as_ref() else {
        return;
    };
    let id = entry.server.id;
    let filtering = !view.filter.trim().is_empty();
    let empty = HashSet::new();
    let hidden = if hide_read_only {
        hidden_paths(&mut view.hidden, entry)
    } else {
        &empty
    };
    let shown = |index: &tree::NodeIndex| {
        tree::node_at(&t.root, index).is_some_and(|n| !hidden.contains(&n.full_path))
    };

    let rows: Vec<Row> = if filtering {
        let key = (id, t.version, view.filter.clone(), hide_read_only);
        if view.hits.key.as_ref() != Some(&key) {
            let mut hits = tree::fuzzy(&t.root, &view.filter);
            hits.retain(|h| shown(&h.index));
            view.hits.ambiguous = ambiguous_names(&t.root, &hits);
            view.hits.hits = hits;
            view.hits.key = Some(key);
        }
        view.hits
            .hits
            .iter()
            .map(|h| Row {
                index: h.index.clone(),
                depth: 0,
            })
            .collect()
    } else {
        let prefix = view.prefix.trim().to_owned();
        let key = (
            id,
            t.version,
            view.expanded_gen,
            prefix.clone(),
            hide_read_only,
        );
        if view.rows.key.as_ref() != Some(&key) {
            let base = if prefix.is_empty() {
                Vec::new()
            } else {
                tree::find(&t.root, &prefix)
                    .filter(|i| tree::node_at(&t.root, i).is_some_and(|n| n.is_container()))
                    .unwrap_or_default()
            };
            let mut rows = tree::visible_rows(&t.root, &base, &t.expanded);
            rows.retain(|r| shown(&r.index));
            view.rows.rows = rows;
            view.rows.key = Some(key);
        }
        if !prefix.is_empty()
            && tree::find(&t.root, &prefix)
                .and_then(|i| tree::node_at(&t.root, &i))
                .is_none_or(|n| !n.is_container())
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("no container at {prefix}, showing the whole tree"),
            );
        }
        view.rows.rows.clone()
    };

    keyboard(ui, t, &rows, view, actions);

    let row_h = ROW_H;
    let selected_pos = view.selected_path.as_ref().and_then(|p| {
        rows.iter()
            .position(|r| tree::node_at(&t.root, &r.index).is_some_and(|n| &n.full_path == p))
    });
    // Rows touch: the 24 px row holds its own margins.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut area = egui::ScrollArea::vertical().auto_shrink(false);
    if std::mem::take(&mut view.scroll_to_selected)
        && let Some(pos) = selected_pos
    {
        // Rows touch: the 24 px row holds its own margins.
        let target = (pos as f32 * row_h - ui.available_height() / 2.0).max(0.0);
        area = area.vertical_scroll_offset(target);
    }
    let send_blocked = entry.send_blocked();
    let value_supported = entry.value_supported();
    area.show_rows(ui, row_h, rows.len(), |ui, range| {
        // Measured inside the scroll area, whose bar takes part of the width.
        // The visible width: a ui can be wider than what shows of it.
        let visible = (ui.clip_rect().right() - ui.max_rect().left()).min(ui.available_width());
        let cols = Columns::tree(visible);
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let row = &rows[i];
            let Some(node) = tree::node_at(&t.root, &row.index) else {
                continue;
            };
            let hit = if filtering {
                view.hits.hits.get(i).cloned()
            } else {
                None
            };
            if node.is_method() {
                view.visible_methods.push(node.full_path.clone());
            }
            let label: egui::WidgetText = match &hit {
                // The expander's place and the button's padding come off the name column.
                Some(h) => hit_label(
                    ui,
                    &node.full_path,
                    &h.positions,
                    cols.name - style::INDENT - 2.0 * ui.spacing().button_padding.x,
                    view.hits
                        .ambiguous
                        .contains(node.full_path.rsplit('/').next().unwrap_or_default()),
                )
                .into(),
                None => style::data(leaf_name(node)).into(),
            };
            let cell = NameCell {
                label,
                depth: row.depth,
                open: node
                    .is_container()
                    .then(|| t.expanded.contains(&node.full_path)),
                pinned: pinned.contains(&node.full_path),
                focusable: true,
            };
            node_row(
                ui,
                cols,
                entry,
                t,
                node,
                cell,
                send_blocked.as_deref(),
                value_supported,
                view,
                actions,
            );
        }
    });
}

/// The three columns of a row: name, control, metadata (PRD 9.2), each a multiple of the grid.
#[derive(Debug, Clone, Copy)]
struct Columns {
    name: f32,
    control: f32,
    meta: f32,
}

impl Columns {
    fn snap(x: f32) -> f32 {
        (x / GRID).floor() * GRID
    }

    /// The tree: the name close to its control, the metadata at the right edge. On a narrow
    /// pane the name gives way first, down to NAME_FLOOR, then the metadata, down to the access
    /// flags alone, then the name again: the widget is the tool and keeps its width, the name
    /// gives its full path on hover.
    fn tree(width: f32) -> Self {
        let full_name = 40.0 * GRID;
        let control = style::CONTROL_W;
        if width >= full_name + control + style::META_W {
            let name = Self::snap((width * 0.3).clamp(full_name, 72.0 * GRID));
            let meta = style::META_W;
            return Columns {
                name,
                control: Self::snap(width - name - meta),
                meta,
            };
        }
        let meta = if width >= style::NAME_FLOOR + control + style::META_W {
            style::META_W
        } else {
            style::FLAGS_W
        };
        Columns {
            name: Self::snap((width - control - meta).max(0.0)),
            control,
            meta,
        }
    }

    /// The pinned panel, narrower: the name is the full path, the metadata only the flags.
    fn pinned(width: f32) -> Self {
        let meta = 10.0 * GRID;
        let name = Self::snap((width * 0.4).max(24.0 * GRID));
        let control = Self::snap((width - name - meta).max(24.0 * GRID));
        Columns {
            name,
            control,
            meta,
        }
    }
}

/// What the name column of a row holds.
struct NameCell {
    label: egui::WidgetText,
    depth: usize,
    /// `Some(open)` for a container, which gets its expander.
    open: Option<bool>,
    pinned: bool,
    /// The row may take the keyboard focus of the selection (the tree, not the pinned panel).
    focusable: bool,
}

/// A row of the tree or of the pinned panel: name, control, metadata, in three strict columns.
/// The control column starts on the same vertical line on every row; the widget carries the
/// value, a node without a widget shows its value there as text.
#[allow(clippy::too_many_arguments)]
fn node_row(
    ui: &mut egui::Ui,
    cols: Columns,
    entry: &ServerEntry,
    t: &crate::state::TreeState,
    node: &oscq_core::Node,
    cell: NameCell,
    send_blocked: Option<&str>,
    value_supported: bool,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    let (row, _) = ui.allocate_exact_size(
        egui::vec2(cols.name + cols.control + cols.meta, ROW_H),
        egui::Sense::hover(),
    );
    let name_rect = egui::Rect::from_min_size(row.min, egui::vec2(cols.name, ROW_H));
    let control_rect = egui::Rect::from_min_size(
        egui::pos2(name_rect.right(), row.top()),
        egui::vec2(cols.control, ROW_H),
    );
    let meta_rect = egui::Rect::from_min_size(
        egui::pos2(control_rect.right(), row.top()),
        egui::vec2(cols.meta, ROW_H),
    );
    let cell_ui =
        |rect: egui::Rect, align: egui::Layout| egui::UiBuilder::new().max_rect(rect).layout(align);
    let left = egui::Layout::left_to_right(egui::Align::Center);
    // Guides: one thin line per ancestor level, so the eye follows the hierarchy.
    let guide = ui.visuals().widgets.noninteractive.bg_stroke;
    for level in 0..cell.depth {
        let x = name_rect.left() + level as f32 * style::INDENT + style::INDENT / 2.0;
        ui.painter().vline(
            x.round() + 0.5,
            name_rect.y_range(),
            egui::Stroke::new(1.0, guide.color),
        );
    }
    let selected = view.selected_path.as_deref() == Some(node.full_path.as_str());
    // Each column is a child ui inside the row already allocated: none moves the row's cursor.
    {
        let ui = &mut ui.new_child(cell_ui(name_rect, left));
        ui.set_clip_rect(name_rect.intersect(ui.clip_rect()));
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(cell.depth as f32 * style::INDENT);
        match cell.open {
            Some(open) => {
                if ui
                    .add_sized(
                        egui::vec2(style::INDENT, style::CONTROL_H),
                        egui::Button::new(style::data(if open { "-" } else { "+" })).frame(false),
                    )
                    .on_hover_text(if open { "Collapse" } else { "Expand" })
                    .clicked()
                {
                    actions.push(Action::Toggle(node.full_path.clone()));
                }
            }
            // The expander's place, so names of one level align; none in the pinned panel.
            None if cell.focusable => ui.add_space(style::INDENT),
            None => {}
        }
        let label = ui
            .add(
                egui::Button::selectable(selected, cell.label)
                    .right_text("")
                    .truncate()
                    .min_size(egui::vec2(ui.available_width(), style::CONTROL_H)),
            )
            .on_hover_text(&node.full_path);
        if label.clicked() {
            actions.push(Action::SelectPath(node.full_path.clone()));
        }
        label.context_menu(|ui| {
            if ui.button("Copy path").clicked() {
                actions.push(Action::Copy(node.full_path.clone()));
                ui.close();
            }
            if ui
                .button(if cell.pinned { "Unpin" } else { "Pin" })
                .clicked()
            {
                actions.push(Action::TogglePin(node.full_path.clone()));
                ui.close();
            }
        });
    }

    let access = node.effective_access(value_supported);
    let p = widget::present(node, access, send_blocked);
    let pending = t.pending.get(&node.full_path);
    let shown = shown_value(t, node, p.value);
    {
        let ui = &mut ui.new_child(cell_ui(control_rect, left));
        ui.set_clip_rect(control_rect.intersect(ui.clip_rect()));
        // A slider and its numeric field fill the column, whatever its width; numeric fields
        // have one width, so their right edges align from row to row.
        ui.spacing_mut().slider_width =
            (cols.control - 20.0 * GRID).clamp(12.0 * GRID, 64.0 * GRID);
        ui.spacing_mut().interact_size.x = 16.0 * GRID;
        if p.widget {
            if widget::choose(node) == WidgetKind::Raw
                && let Shown::Read(text) = &shown
            {
                ui.label(style::data(text.clone()));
            }
            let focus = cell.focusable && selected && std::mem::take(&mut view.focus_widget);
            let edit = edit_widget(ui, node, pending, view, focus);
            match &edit {
                Some((_, SendMode::Drag)) => {
                    view.drag_open.insert(node.full_path.clone());
                }
                Some((_, SendMode::Final)) => {
                    view.drag_open.remove(&node.full_path);
                }
                None => {}
            }
            if let Some((args, mode)) = edit
                && let (Some(path), Some(sig)) = (node.path.clone(), signature_of(node))
            {
                actions.push(Action::Send(
                    SendReq {
                        server_id: entry.server.id,
                        path,
                        signature: sig,
                        args,
                        readable: access.readable(),
                    },
                    mode,
                ));
            }
        } else {
            // No widget: the value is the control column's content.
            match &shown {
                Shown::Sent(text) => {
                    ui.label(style::data(text.clone()).italics().weak());
                }
                Shown::Unavailable => {
                    ui.weak("unavailable");
                }
                Shown::Read(text) => {
                    ui.label(style::data(text.clone()));
                }
                Shown::Nothing => {}
            }
            // The node keeps the access its server declares; what is said is that nothing can
            // be sent.
            if let Some(reason) = &p.blocked {
                ui.weak(CANNOT_SEND).on_hover_text(reason);
            }
        }
    }

    {
        let ui = &mut ui.new_child(cell_ui(
            meta_rect,
            egui::Layout::right_to_left(egui::Align::Center),
        ));
        ui.set_clip_rect(meta_rect.intersect(ui.clip_rect()));
        {
            ui.spacing_mut().item_spacing.x = style::ITEM_GAP;
            if node.is_method() {
                let flags = format!(
                    "{}{}",
                    if access.readable() { "r" } else { "" },
                    if access.writable() { "w" } else { "" }
                );
                ui.label(style::data(flags).weak());
                // Narrowed to the flags alone, the column holds nothing else.
                if cols.meta >= style::META_W
                    && let Some(tags) = &node.type_raw
                {
                    ui.add(egui::Label::new(style::data(tags).weak()).truncate());
                }
                if cols.meta >= style::META_W && matches!(shown, Shown::Sent(_)) {
                    ui.label(style::data("sent").italics().weak())
                        .on_hover_text(if p.value {
                            "Sent, not yet confirmed by the server"
                        } else {
                            "Sent, not confirmed: this node cannot be read"
                        });
                }
            }
        }
    }
}

/// A row of the pinned panel: its cells, then a button that unpins it.
fn pinned_row(
    ui: &mut egui::Ui,
    e: &ServerEntry,
    path: &str,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    // The row, then its unpin button at the right: unpinning needs no right click.
    let unpin_w = 6.0 * GRID;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        pinned_cells(ui, e, path, ui.available_width() - unpin_w, view, actions);
        if ui
            .add(egui::Button::new("\u{d7}").min_size(egui::vec2(unpin_w, style::CONTROL_H)))
            .on_hover_text("Unpin")
            .clicked()
        {
            actions.push(Action::TogglePin(path.to_owned()));
        }
    });
}

/// The cells of a pinned row: the node as the tree draws it, its full path for a name, or the
/// path marked absent when the server does not have it.
fn pinned_cells(
    ui: &mut egui::Ui,
    e: &ServerEntry,
    path: &str,
    width: f32,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    let cols = Columns::pinned(width);
    let node = e.tree.as_ref().and_then(|t| t.node(path).map(|n| (t, n)));
    match node {
        Some((t, node)) => {
            let cell = NameCell {
                label: style::data(path).into(),
                depth: 0,
                open: None,
                pinned: true,
                focusable: false,
            };
            node_row(
                ui,
                cols,
                e,
                t,
                node,
                cell,
                e.send_blocked().as_deref(),
                e.value_supported(),
                view,
                actions,
            );
        }
        None => {
            // The same columns as a present node: the path, then "absent" where a control goes.
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(cols.name + cols.control + cols.meta, ROW_H),
                egui::Sense::hover(),
            );
            let left = egui::Layout::left_to_right(egui::Align::Center);
            let name_rect = egui::Rect::from_min_size(row.min, egui::vec2(cols.name, ROW_H));
            let control_rect = egui::Rect::from_min_size(
                egui::pos2(name_rect.right(), row.top()),
                egui::vec2(cols.control, ROW_H),
            );
            let ui_at = |ui: &mut egui::Ui, rect| {
                ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(left))
            };
            let label = ui_at(ui, name_rect)
                .add(
                    egui::Button::selectable(false, style::data(path).weak())
                        .right_text("")
                        .truncate()
                        .min_size(egui::vec2(cols.name, style::CONTROL_H)),
                )
                .on_hover_text(path);
            label.context_menu(|ui| {
                if ui.button("Unpin").clicked() {
                    actions.push(Action::TogglePin(path.to_owned()));
                    ui.close();
                }
            });
            ui_at(ui, control_rect)
                .weak("absent")
                .on_hover_text("Not in the tree of this server: not published, or not loaded yet");
        }
    }
}

/// What the value column of a row shows.
#[derive(Debug, PartialEq)]
enum Shown {
    /// The last value sent, unconfirmed until a value is read from the server (PRD 9.6). Shown on
    /// a node that cannot be read too, where nothing else ever replaces it.
    Sent(String),
    Unavailable,
    Read(String),
    Nothing,
}

fn shown_value(t: &crate::state::TreeState, node: &oscq_core::Node, readable: bool) -> Shown {
    if !node.is_method() {
        return Shown::Nothing;
    }
    if let Some(args) = t.pending.get(&node.full_path) {
        return Shown::Sent(traffic::args_text(args));
    }
    if !readable {
        return Shown::Nothing;
    }
    if t.unavailable.contains(&node.full_path) {
        return Shown::Unavailable;
    }
    node.value
        .as_ref()
        .map_or(Shown::Nothing, |v| Shown::Read(widget::value_text(v)))
}

fn leaf_name(node: &oscq_core::Node) -> String {
    node.full_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("/")
        .to_owned()
}

fn signature_of(node: &oscq_core::Node) -> Option<TypeSignature> {
    match &node.signature {
        Some(s) => Some(s.clone()),
        None if node.type_raw.is_none() => TypeSignature::parse("").ok(),
        None => None,
    }
}

/// A filter hit as its row shows it: its name, and when another hit carries the same name in
/// another container, the path of its own container after it, cut from the left when the column
/// is short, so that the nearest containers, which tell those hits apart, stay in sight
/// ("normalized  .../param/fader"). The matched characters are marked wherever they show; the
/// full path is on hover.
fn hit_label(
    ui: &egui::Ui,
    full_path: &str,
    positions: &[u32],
    width: f32,
    with_container: bool,
) -> LayoutJob {
    let font = egui::FontId::new(style::DATA, egui::FontFamily::Monospace);
    // Monospace: every character, the ellipsis included, is as wide as the others.
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm')).max(1.0);
    let columns = (width / char_w).floor().max(0.0) as usize;
    let chars: Vec<char> = full_path.chars().collect();
    let cut = chars.iter().rposition(|c| *c == '/').unwrap_or(0);
    let name: String = chars[cut + 1..].iter().collect();
    let parent: String = chars[..cut].iter().collect();
    let gap = "  ";
    let room = columns.saturating_sub(name.chars().count() + gap.len());
    // The parent's characters shown, as an index into `chars`, and whether its head is cut.
    let (from, cut_head) = if parent.chars().count() <= room {
        (0, false)
    } else if room >= 2 {
        // Room for the ellipsis and at least one character; whole segments where possible.
        let keep = room - 1;
        let start = cut - keep;
        let start = chars[start..cut]
            .iter()
            .position(|c| *c == '/')
            .map_or(start, |i| start + i);
        (start, true)
    } else {
        (cut, true)
    };
    let color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let format = |c: egui::Color32, marked: bool| TextFormat {
        font_id: font.clone(),
        color: if marked { strong } else { c },
        underline: if marked {
            egui::Stroke::new(1.0, strong)
        } else {
            egui::Stroke::NONE
        },
        ..TextFormat::default()
    };
    let marked = |i: usize| positions.binary_search(&(i as u32)).is_ok();
    let mut job = LayoutJob::default();
    for (i, c) in chars.iter().enumerate().skip(cut + 1) {
        job.append(&c.to_string(), 0.0, format(color, marked(i)));
    }
    if !with_container || parent.is_empty() || (cut_head && from >= cut) {
        return job;
    }
    job.append(gap, 0.0, format(weak, false));
    if cut_head {
        job.append("\u{2026}", 0.0, format(weak, false));
    }
    for (i, c) in chars.iter().enumerate().take(cut).skip(from) {
        job.append(&c.to_string(), 0.0, format(weak, marked(i)));
    }
    job
}

fn keyboard(
    ui: &mut egui::Ui,
    t: &crate::state::TreeState,
    rows: &[Row],
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    let ctx = ui.ctx().clone();
    let typing = ctx.memory(|m| m.focused()).is_some();
    if typing {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        return;
    }
    let pressed = |k| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
    if pressed(Key::Slash) {
        view.focus_filter = true;
        return;
    }
    if pressed(Key::F5) {
        actions.push(match view.selected_path.clone() {
            Some(p) if t.root.find(&p).is_some_and(|n| n.is_container()) => Action::RefreshTree(p),
            Some(p) => Action::RefreshValue(p),
            None => Action::RefreshTree("/".to_owned()),
        });
    }
    let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
    if copy && let Some(p) = &view.selected_path {
        actions.push(Action::Copy(p.clone()));
    }
    if rows.is_empty() {
        return;
    }
    let path_of = |r: &Row| tree::node_at(&t.root, &r.index).map(|n| n.full_path.clone());
    let pos = view
        .selected_path
        .as_ref()
        .and_then(|p| rows.iter().position(|r| path_of(r).as_ref() == Some(p)));
    let select = |i: usize, view: &mut View| {
        if let Some(p) = rows.get(i).and_then(path_of) {
            view.selected_path = Some(p);
            view.scroll_to_selected = true;
        }
    };
    if pressed(Key::ArrowDown) {
        select(pos.map_or(0, |p| (p + 1).min(rows.len() - 1)), view);
    }
    if pressed(Key::ArrowUp) {
        select(pos.map_or(0, |p| p.saturating_sub(1)), view);
    }
    let Some(pos) = pos else {
        return;
    };
    let row = &rows[pos];
    let Some(node) = tree::node_at(&t.root, &row.index) else {
        return;
    };
    if pressed(Key::ArrowRight) && node.is_container() {
        if t.expanded.contains(&node.full_path) {
            select(pos + 1, view);
        } else {
            actions.push(Action::Toggle(node.full_path.clone()));
        }
    }
    if pressed(Key::ArrowLeft) {
        if node.is_container() && t.expanded.contains(&node.full_path) {
            actions.push(Action::Toggle(node.full_path.clone()));
        } else if row.index.len() > 1 {
            let parent: NodeIndex = row.index[..row.index.len() - 1].to_vec();
            if let Some(i) = rows.iter().position(|r| r.index == parent) {
                select(i, view);
            }
        }
    }
    if pressed(Key::Enter) {
        view.focus_widget = true;
    }
}

// -------------------------------------------------------------------- editing widgets

fn osc_f64(a: &OscType) -> Option<f64> {
    match a {
        OscType::Int(v) => Some(f64::from(*v)),
        OscType::Long(v) => Some(*v as f64),
        OscType::Float(v) => Some(f64::from(*v)),
        OscType::Double(v) => Some(*v),
        _ => None,
    }
}

fn current_f64(node: &oscq_core::Node, pending: Option<&Vec<OscType>>, i: usize) -> Option<f64> {
    match pending {
        Some(p) => p.get(i).and_then(osc_f64),
        None => widget::value_f64(node.value.as_ref(), i),
    }
}

fn current_colour(node: &oscq_core::Node, pending: Option<&Vec<OscType>>) -> Color32 {
    let from_bytes = |[r, g, b, a]: [u8; 4]| Color32::from_rgba_unmultiplied(r, g, b, a);
    if let Some(p) = pending {
        return match p.as_slice() {
            [OscType::Color(c)] => from_bytes([c.red, c.green, c.blue, c.alpha]),
            [a, b, c, d] => {
                let f =
                    |x: &OscType| (osc_f64(x).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                from_bytes([f(a), f(b), f(c), f(d)])
            }
            _ => Color32::WHITE,
        };
    }
    if let Some(hex) = widget::value_str(node.value.as_ref(), 0)
        && let Some(bytes) = parse_rgba_hex(&hex)
    {
        return from_bytes(bytes);
    }
    let f = |i| {
        (widget::value_f64(node.value.as_ref(), i)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0)
            * 255.0)
            .round() as u8
    };
    from_bytes([f(0), f(1), f(2), f(3)])
}

/// Whether a widget interaction sends, and how. `drag_open`: this drag already sent.
fn send_mode(dragged: bool, drag_stopped: bool, moved: bool, drag_open: bool) -> Option<SendMode> {
    if dragged {
        moved.then_some(SendMode::Drag)
    } else if drag_stopped {
        (moved || drag_open).then_some(SendMode::Final)
    } else {
        moved.then_some(SendMode::Final)
    }
}

/// Draws the widget for a node and returns the arguments to send, if the user changed anything.
fn edit_widget(
    ui: &mut egui::Ui,
    node: &oscq_core::Node,
    pending: Option<&Vec<OscType>>,
    view: &mut View,
    focus: bool,
) -> Option<(Vec<OscType>, SendMode)> {
    let path = node.full_path.clone();
    let kind = widget::choose(node);
    let fine = ui.input(|i| i.modifiers.shift);
    // Decided on the value itself, not on `changed()`, which a text edit in progress also
    // reports. A click alone ends a drag too: on release, send only if the value moved, now or
    // through drag sends of this drag. The sent value is no evidence of a drag: on a node that
    // cannot be read it stays, and every click would send it again.
    let drag_open = view.drag_open.contains(&path);
    let decide = |r: &egui::Response, moved: bool| {
        send_mode(r.dragged(), r.drag_stopped(), moved, drag_open)
    };
    let focus_on = |r: &egui::Response| {
        if focus {
            r.request_focus();
        }
    };
    match kind {
        WidgetKind::FloatSlider { min, max } => {
            let mut v = current_f64(node, pending, 0).unwrap_or(min).clamp(min, max);
            let before = v;
            let s = ui.add(egui::Slider::new(&mut v, min..=max).show_value(false));
            let after_slider = v;
            let d = ui.add(
                egui::DragValue::new(&mut v)
                    .update_while_editing(false)
                    .min_decimals(2)
                    .range(min..=max)
                    .speed((max - min) / if fine { 2000.0 } else { 200.0 }),
            );
            focus_on(&s);
            decide(&s, after_slider != before)
                .or_else(|| decide(&d, v != after_slider))
                .map(|m| (vec![OscType::Double(v)], m))
        }
        WidgetKind::FloatDrag => {
            let mut v = current_f64(node, pending, 0).unwrap_or(0.0);
            let before = v;
            let d = ui.add(
                egui::DragValue::new(&mut v)
                    .update_while_editing(false)
                    .min_decimals(2)
                    .speed(if fine { 0.001 } else { 0.01 }),
            );
            focus_on(&d);
            decide(&d, v != before).map(|m| (vec![OscType::Double(v)], m))
        }
        WidgetKind::IntSlider { min, max } => {
            let mut v = current_f64(node, pending, 0)
                .map(|f| f.round() as i64)
                .unwrap_or(min)
                .clamp(min, max);
            let before = v;
            let s = ui.add(
                egui::Slider::new(&mut v, min..=max)
                    .integer()
                    .update_while_editing(false),
            );
            focus_on(&s);
            decide(&s, v != before).map(|m| (vec![OscType::Long(v)], m))
        }
        WidgetKind::IntDrag { min, max } => {
            let mut v = current_f64(node, pending, 0)
                .map_or(0, |f| f.round() as i64)
                .clamp(min, max);
            let before = v;
            let d = ui.add(
                egui::DragValue::new(&mut v)
                    .update_while_editing(false)
                    .range(min..=max)
                    .speed(if fine { 0.05 } else { 0.5 }),
            );
            focus_on(&d);
            decide(&d, v != before).map(|m| (vec![OscType::Long(v)], m))
        }
        WidgetKind::Dropdown { vals } => {
            let current = match pending.and_then(|p| p.first()) {
                Some(OscType::String(s)) => serde_json::Value::String(s.clone()),
                Some(a) => osc_f64(a).map(serde_json::Value::from).unwrap_or_default(),
                None => match node.value.as_ref().and_then(|v| v.entry(0)) {
                    Some(oscq_core::ArgEntry::Value(v)) => v.clone(),
                    _ => serde_json::Value::Null,
                },
            };
            let shown = |v: &serde_json::Value| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let mut chosen = None;
            let r = egui::ComboBox::from_id_salt(&path)
                .selected_text(shown(&current))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for v in &vals {
                        if ui.selectable_label(*v == current, shown(v)).clicked() {
                            chosen = Some(v.clone());
                        }
                    }
                });
            focus_on(&r.response);
            chosen.map(|v| {
                let arg = match v {
                    serde_json::Value::String(s) => OscType::String(s),
                    serde_json::Value::Number(n) => match n.as_i64() {
                        Some(i) => OscType::Long(i),
                        None => OscType::Double(n.as_f64().unwrap_or(0.0)),
                    },
                    other => OscType::String(other.to_string()),
                };
                (vec![arg], SendMode::Final)
            })
        }
        WidgetKind::Text => {
            let initial = match pending.and_then(|p| p.first()) {
                Some(OscType::String(s)) => s.clone(),
                _ => widget::value_str(node.value.as_ref(), 0).unwrap_or_default(),
            };
            let buf = view.text_buffers.entry(path).or_insert(initial);
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(160.0));
            focus_on(&r);
            (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                .then(|| (vec![OscType::String(buf.clone())], SendMode::Final))
        }
        WidgetKind::Toggle => {
            let mut b = match pending.and_then(|p| p.first()) {
                Some(OscType::Bool(b)) => *b,
                _ => widget::value_bool(node.value.as_ref(), 0).unwrap_or(false),
            };
            let r = ui.checkbox(&mut b, "");
            focus_on(&r);
            r.changed()
                .then(|| (vec![OscType::Bool(b)], SendMode::Final))
        }
        WidgetKind::Momentary => {
            let r = ui.button("send");
            focus_on(&r);
            let arg = match node.signature.as_ref().map(|s| s.args()) {
                Some([ArgType::Tag(TypeTag::Nil)]) => OscType::Nil,
                _ => OscType::Inf,
            };
            r.clicked().then(|| (vec![arg], SendMode::Final))
        }
        WidgetKind::ColourR | WidgetKind::ColourFloats => {
            let mut c = current_colour(node, pending);
            let r = egui::widgets::color_picker::color_edit_button_srgba(
                ui,
                &mut c,
                egui::widgets::color_picker::Alpha::OnlyBlend,
            );
            focus_on(&r);
            r.changed().then(|| {
                let [red, green, blue, alpha] = c.to_srgba_unmultiplied();
                let args = if kind == WidgetKind::ColourR {
                    vec![OscType::Color(OscColor {
                        red,
                        green,
                        blue,
                        alpha,
                    })]
                } else {
                    [red, green, blue, alpha]
                        .iter()
                        .map(|b| OscType::Double(f64::from(*b) / 255.0))
                        .collect()
                };
                (args, SendMode::Drag)
            })
        }
        WidgetKind::Grouped { ranges } => {
            let mut values: Vec<f64> = (0..ranges.len())
                .map(|i| {
                    let v = current_f64(node, pending, i).unwrap_or(0.0);
                    ranges[i].map_or(v, |(a, b)| v.clamp(a, b))
                })
                .collect();
            let mut mode = None;
            for (i, v) in values.iter_mut().enumerate() {
                let before = *v;
                let mut d = egui::DragValue::new(v)
                    .update_while_editing(false)
                    .min_decimals(2)
                    .speed(if fine { 0.001 } else { 0.01 });
                if let Some((a, b)) = ranges[i] {
                    d = d
                        .range(a..=b)
                        .speed((b - a) / if fine { 2000.0 } else { 200.0 });
                }
                let r = ui.add(d);
                if i == 0 {
                    focus_on(&r);
                }
                let moved = *v != before;
                mode = mode.or_else(|| decide(&r, moved));
            }
            mode.map(|m| (values.into_iter().map(OscType::Double).collect(), m))
        }
        WidgetKind::Raw => {
            let buf = view.text_buffers.entry(path).or_default();
            let r = ui.add(
                egui::TextEdit::singleline(buf)
                    .hint_text("arguments, space separated")
                    .desired_width(160.0),
            );
            focus_on(&r);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let sig = signature_of(node)?;
                let words: Vec<&str> = buf.split_whitespace().collect();
                let args: Option<Vec<OscType>> = sig
                    .args()
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| match ty {
                        ArgType::Tag(t) if t.carries_no_payload() => arg_from_text(ty, "").ok(),
                        _ => arg_from_text(ty, words.get(i).copied().unwrap_or("")).ok(),
                    })
                    .collect();
                return args.map(|a| (a, SendMode::Final));
            }
            None
        }
    }
}

// -------------------------------------------------------------------- JSON tab

fn json_tab(ui: &mut egui::Ui, entry: &ServerEntry, view: &mut View, actions: &mut Vec<Action>) {
    let Some(t) = entry.tree.as_ref() else {
        return;
    };
    let key = (entry.server.id, t.version, view.selected_path.clone());
    if view.json.key.as_ref() != Some(&key) {
        let (exact, note) = selected_raw(t, view.selected_path.as_deref());
        view.json.lines = json_view::pretty(&exact);
        view.json.exact = exact;
        view.json.note = note;
        view.json.key = Some(key);
    }
    ui.horizontal(|ui| {
        ui.label(match &view.selected_path {
            Some(p) => format!("Subtree at {p}, as received"),
            None => "Whole document, as received".to_owned(),
        });
        if ui.button("Copy").clicked() {
            actions.push(Action::Copy(view.json.exact.clone()));
        }
        if ui.button("Save to file").clicked() {
            actions.push(Action::Save {
                stem: "document",
                ext: "json",
                contents: view.json.exact.clone(),
            });
        }
    });
    if let Some(n) = &view.json.note {
        ui.weak(n);
    }
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    let dark = ui.visuals().dark_mode;
    let lines = &view.json.lines;
    egui::ScrollArea::both()
        .auto_shrink(false)
        .show_rows(ui, row_h, lines.len(), |ui, range| {
            for line in &lines[range] {
                ui.label(json_line(line, dark));
            }
        });
}

/// The exact received text for the selection: from the nearest subtree fetched on its own, or
/// from the root document.
fn selected_raw(t: &crate::state::TreeState, selected: Option<&str>) -> (String, Option<String>) {
    let Some(sel) = selected else {
        return (t.raw.to_string(), None);
    };
    let Some(index) = tree::find(&t.root, sel) else {
        return (
            t.raw.to_string(),
            Some("selection not found, whole document shown".into()),
        );
    };
    let names = tree::names_at(&t.root, &index).unwrap_or_default();
    // Walk from the selection up to the root, looking for a subtree body fetched on its own.
    for depth in (0..=index.len()).rev() {
        let path = tree::node_at(&t.root, &index[..depth]).map(|n| n.full_path.clone());
        let raw = match (depth, &path) {
            (0, _) => Some(t.raw.clone()),
            (_, Some(p)) => t.raw_subtrees.get(p).cloned(),
            _ => None,
        };
        if let Some(raw) = raw {
            return match json_view::raw_subtree(&raw, &names[depth..]) {
                Some(s) => (s.to_owned(), None),
                None => (
                    raw.to_string(),
                    Some("subtree not found in the received text, whole body shown".into()),
                ),
            };
        }
    }
    (t.raw.to_string(), None)
}

fn json_line(line: &Line, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::monospace(12.0);
    let colour = |k: Token| match (k, dark) {
        (Token::Key, true) => Color32::from_rgb(130, 180, 240),
        (Token::Key, false) => Color32::from_rgb(20, 80, 160),
        (Token::Str, true) => Color32::from_rgb(170, 210, 140),
        (Token::Str, false) => Color32::from_rgb(40, 120, 40),
        (Token::Number, true) => Color32::from_rgb(230, 180, 110),
        (Token::Number, false) => Color32::from_rgb(170, 90, 0),
        (Token::Literal, true) => Color32::from_rgb(210, 150, 220),
        (Token::Literal, false) => Color32::from_rgb(140, 40, 150),
        (Token::Punct, true) => Color32::from_gray(170),
        (Token::Punct, false) => Color32::from_gray(90),
    };
    job.append(
        &"  ".repeat(line.indent),
        0.0,
        TextFormat::simple(font.clone(), Color32::GRAY),
    );
    for (k, text) in &line.tokens {
        job.append(text, 0.0, TextFormat::simple(font.clone(), colour(*k)));
    }
    job
}

// -------------------------------------------------------------------- traffic tab

fn traffic_tab(
    ui: &mut egui::Ui,
    entry: &mut ServerEntry,
    view: &mut View,
    started: Instant,
    actions: &mut Vec<Action>,
) {
    use traffic::DirFilter;
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_H);
        ui.add(
            egui::TextEdit::singleline(&mut view.traffic_filter)
                .font(egui::TextStyle::Monospace)
                .hint_text("filter path or type tags")
                .desired_width(48.0 * GRID),
        );
        ui.spacing_mut().item_spacing.x = 0.0;
        for (dir, text) in [
            (DirFilter::All, "all"),
            (DirFilter::In, "in"),
            (DirFilter::Out, "out"),
        ] {
            ui.selectable_value(&mut view.traffic_dir, dir, text);
        }
        ui.spacing_mut().item_spacing.x = style::ITEM_GAP;
        ui.add_space(GROUP_GAP);
        let mut paused = entry.traffic.paused();
        if ui.toggle_value(&mut paused, "Pause").changed() {
            entry.traffic.set_paused(paused);
        }
        if ui.button("Clear").clicked() {
            entry.traffic.clear();
        }
        if ui.button("Export").clicked() {
            let view_records = entry.traffic.view(&view.traffic_filter, view.traffic_dir);
            actions.push(Action::Save {
                stem: "traffic",
                ext: "txt",
                contents: traffic::export(started, &view_records),
            });
        }
        ui.add_space(GROUP_GAP);
        ui.weak(format!(
            "{} of max {} kept",
            entry.traffic.len(),
            traffic::CAPACITY
        ));
    });
    ui.add_space(style::ITEM_GAP);
    if entry.traffic.is_empty() {
        ui.weak("No traffic yet. Every message sent, and every value received over the WebSocket, is logged here.");
        return;
    }
    // One line per record; a block opens to its messages, indented under it.
    let records = entry.traffic.view(&view.traffic_filter, view.traffic_dir);
    let mut lines: Vec<(Option<Instant>, String)> = Vec::with_capacity(records.len());
    for r in &records {
        if r.block.is_empty() {
            lines.push((None, traffic::line(started, r)));
        } else {
            let open = view.traffic_open.contains(&r.at);
            let mark = if open { "-" } else { "+" };
            lines.push((Some(r.at), format!("{mark} {}", traffic::line(started, r))));
            if open {
                lines.extend(r.block.iter().map(|m| (None, traffic::block_line(m))));
            }
        }
    }
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, row_h, lines.len(), |ui, range| {
            for (block, text) in &lines[range] {
                match block {
                    Some(at) => {
                        let r = ui.add(
                            egui::Label::new(style::data(text.as_str()))
                                .sense(egui::Sense::click()),
                        );
                        if r.on_hover_text("Click to show or hide the messages")
                            .clicked()
                            && !view.traffic_open.remove(at)
                        {
                            view.traffic_open.insert(*at);
                        }
                    }
                    None => {
                        ui.label(style::data(text.as_str()));
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use oscq_core::{Access, AttributeReply, DiscoveredServer, Origin, parse_document};

    struct Harness {
        ctx: egui::Context,
        app: VoidApp,
        commands: flume::Receiver<Command>,
        events: flume::Sender<Event>,
        _rt: tokio::runtime::Runtime,
    }

    fn harness() -> Harness {
        harness_with(Config::default())
    }

    fn harness_with(config: Config) -> Harness {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .unwrap();
        let (cmd_tx, cmd_rx) = flume::unbounded();
        let (ev_tx, ev_rx) = flume::unbounded();
        let ctx = egui::Context::default();
        let mut app = VoidApp::with_backend(
            &ctx,
            Backend {
                commands: cmd_tx,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            config,
            None,
        );
        app.started = Instant::now() - Duration::from_secs(10);
        Harness {
            ctx,
            app,
            commands: cmd_rx,
            events: ev_tx,
            _rt: rt,
        }
    }

    impl Harness {
        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            self.frame_sized(egui::vec2(1100.0, 700.0), events)
        }

        /// One frame in a window of `size`.
        fn frame_sized(&mut self, size: egui::Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..egui::RawInput::default()
            };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| app.frame(ui));
            // No renderer here: consume the texture uploads as one would.
            out.textures_delta.clear();
            out
        }

        /// One frame at an explicit time, in seconds, as an event loop would run it.
        fn frame_at(&mut self, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
            let input = egui::RawInput {
                events,
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 700.0),
                )),
                ..egui::RawInput::default()
            };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| app.frame(ui));
            out.textures_delta.clear();
            out
        }

        /// Frames an event loop runs from `time` on: the next one at once for a repaint asked
        /// with no delay, after the delay otherwise, until none is asked for. `moves` are pointer
        /// positions fed one per frame first. Returns the frames run, the time reached, and where
        /// each repaint was asked for.
        fn frames_until_idle(
            &mut self,
            mut time: f64,
            moves: &[egui::Pos2],
        ) -> (usize, f64, Vec<String>) {
            let mut frames = 0;
            let mut causes = Vec::new();
            let mut pending: std::collections::VecDeque<egui::Pos2> =
                moves.iter().copied().collect();
            loop {
                let events = pending
                    .pop_front()
                    .map(|p| vec![egui::Event::PointerMoved(p)])
                    .unwrap_or_default();
                let moving = !events.is_empty();
                let out = self.frame_at(time, events);
                frames += 1;
                causes.extend(self.ctx.repaint_causes().iter().map(|c| c.to_string()));
                let delay = idle_delay(&out);
                if delay == Duration::MAX && !moving && pending.is_empty() {
                    return (frames, time, causes);
                }
                assert!(frames < 2000, "never idle: {causes:?}");
                time += delay.as_secs_f64().clamp(1.0 / 60.0, 10.0);
            }
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
            self.frame(Vec::new());
        }

        fn sent(&self) -> Vec<Command> {
            self.commands.try_iter().collect()
        }
    }

    fn server() -> DiscoveredServer {
        DiscoveredServer {
            id: ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: "OSCQuery Helper".into(),
            host: "127.0.0.1".into(),
            port: 2345,
            addresses: Vec::new(),
            origin: Origin::Mdns {
                instance: "OSCQuery Helper".into(),
                service_type: "_oscjson._tcp.local.".into(),
            },
            stale: false,
        }
    }

    fn idle_delay(out: &egui::FullOutput) -> Duration {
        out.viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .unwrap()
    }

    #[test]
    fn no_repaint_when_idle() {
        let mut h = harness();
        h.events.send(Event::ServerFound(server())).unwrap();
        for _ in 0..5 {
            h.frame(Vec::new());
        }
        let out = h.frame(Vec::new());
        assert_eq!(
            idle_delay(&out),
            Duration::MAX,
            "an idle frame asks for no repaint"
        );
        assert_eq!(h.app.state.servers.len(), 1);
    }

    #[test]
    fn event_flood_is_drained_over_several_frames() {
        let mut h = harness();
        let s = server();
        h.events.send(Event::ServerFound(s.clone())).unwrap();
        for _ in 0..(EVENTS_PER_FRAME * 2) {
            h.events.send(Event::ServerChanged(s.clone())).unwrap();
        }
        let out = h.frame(Vec::new());
        assert!(!h.app.backend.events.is_empty(), "bounded per frame");
        assert_eq!(idle_delay(&out), Duration::ZERO, "comes back for the rest");
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(h.app.backend.events.is_empty());
    }

    #[test]
    fn select_connects_and_keyboard_expands_with_a_subtree_fetch() {
        let mut h = harness();
        let s = server();
        let id = s.id;
        h.events.send(Event::ServerFound(s)).unwrap();
        h.frame(Vec::new());
        h.app.act(&h.ctx.clone(), Action::SelectServer(id));
        assert!(matches!(h.sent().as_slice(), [Command::Connect { .. }]));

        let raw = r#"{"FULL_PATH":"/","CONTENTS":{"test":{"FULL_PATH":"/test","CONTENTS":{"my_float":{"FULL_PATH":"/test/my_float","TYPE":"f","ACCESS":3,"VALUE":[0.5],"RANGE":[{"MIN":0,"MAX":1}]}}}}}"#;
        h.events
            .send(Event::TreeLoaded {
                server_id: id,
                raw: Arc::from(raw),
                report: parse_document(raw.as_bytes()).unwrap(),
            })
            .unwrap();
        h.frame(Vec::new());
        h.key(Key::ArrowDown);
        assert_eq!(h.app.view.selected_path.as_deref(), Some("/test"));
        h.key(Key::ArrowRight);
        let t = h.app.state.entry(id).unwrap().tree.as_ref().unwrap();
        assert!(t.expanded.contains("/test"));
        assert!(matches!(
            h.sent().as_slice(),
            [Command::RefreshTree { from_path, .. }] if from_path == "/test"
        ));
        h.key(Key::ArrowDown);
        assert_eq!(h.app.view.selected_path.as_deref(), Some("/test/my_float"));
        h.key(Key::ArrowLeft);
        assert_eq!(h.app.view.selected_path.as_deref(), Some("/test"));
        h.key(Key::ArrowLeft);
        let t = h.app.state.entry(id).unwrap().tree.as_ref().unwrap();
        assert!(!t.expanded.contains("/test"), "collapsed");
        h.key(Key::ArrowRight);
        assert!(
            h.sent().is_empty(),
            "subtree fetched once, not on every expand"
        );
    }

    #[test]
    fn sends_are_rate_limited_final_always_sent_and_refreshed() {
        let mut h = harness();
        let s = server();
        let id = s.id;
        h.events.send(Event::ServerFound(s)).unwrap();
        h.frame(Vec::new());
        let req = |v: f64| SendReq {
            server_id: id,
            path: OscPath::parse("/test/my_float").unwrap(),
            signature: TypeSignature::parse("f").unwrap(),
            args: vec![OscType::Double(v)],
            readable: true,
        };
        let ctx = h.ctx.clone();
        h.app.act(&ctx, Action::Send(req(0.1), SendMode::Drag));
        h.app.act(&ctx, Action::Send(req(0.2), SendMode::Drag));
        h.app.act(&ctx, Action::Send(req(0.3), SendMode::Drag));
        let first: Vec<Command> = h.sent();
        assert_eq!(first.len(), 1, "one per interval while dragging");
        assert!(matches!(
            &first[0],
            Command::SendOsc {
                refresh_after: false,
                ..
            }
        ));
        h.app.act(&ctx, Action::Send(req(0.4), SendMode::Final));
        let fin = h.sent();
        assert!(matches!(
            fin.as_slice(),
            [Command::SendOsc { refresh_after: true, args, .. }] if args == &vec![OscType::Double(0.4)]
        ));
        std::thread::sleep(INTERVAL * 2);
        h.frame(Vec::new());
        assert!(
            h.sent().is_empty(),
            "held drag value dropped after the final one"
        );
    }

    #[test]
    fn a_click_without_move_sends_nothing_a_release_after_drag_sends_final() {
        assert_eq!(send_mode(false, true, false, false), None, "plain click");
        assert_eq!(
            send_mode(false, true, false, true),
            Some(SendMode::Final),
            "release after drag sends"
        );
        assert_eq!(send_mode(true, false, true, false), Some(SendMode::Drag));
        assert_eq!(send_mode(true, false, false, true), None);
        assert_eq!(send_mode(false, false, true, false), Some(SendMode::Final));
    }

    fn sent_long(cmds: &[Command]) -> i64 {
        match cmds {
            [
                Command::SendOsc {
                    refresh_after: false,
                    args,
                    ..
                },
            ] => match args.as_slice() {
                [OscType::Long(v)] => *v,
                other => panic!("unexpected arguments {other:?}"),
            },
            other => panic!("expected one send without refresh, got {other:?}"),
        }
    }

    /// The real Helper: `/test/my_longlong` is `h`, ACCESS 2, RANGE 0 to 100, no VALUE, and a value
    /// query answers `{}`.
    #[test]
    fn a_sent_value_stays_shown_on_a_write_only_node() {
        let root = include_str!("../../../tests/fixtures/helper-root.json");
        let host = include_str!("../../../tests/fixtures/helper-hostinfo.json");
        let path = "/test/my_longlong";
        let mut h = harness();
        let s = server();
        let id = s.id;
        h.events.send(Event::ServerFound(s)).unwrap();
        h.frame(Vec::new());
        let ctx = h.ctx.clone();
        h.app.act(&ctx, Action::SelectServer(id));
        let (info, _) = oscq_core::HostInfo::parse(host.as_bytes()).unwrap();
        let destination = info.osc_destination("127.0.0.1", 2345);
        let tree_loaded = || Event::TreeLoaded {
            server_id: id,
            raw: Arc::from(root),
            report: parse_document(root.as_bytes()).unwrap(),
        };
        h.events
            .send(Event::HostInfo {
                server_id: id,
                info,
                destination,
            })
            .unwrap();
        h.events.send(tree_loaded()).unwrap();
        // The service's order: the tree, then the connection is up.
        h.events
            .send(Event::TransportState {
                server_id: id,
                state: oscq_core::TransportState::HttpOnly,
            })
            .unwrap();
        h.frame(Vec::new());
        h.app.act(&ctx, Action::Toggle("/test".into()));
        h.app.view.selected_path = Some(path.into());
        h.frame(Vec::new());
        h.sent();

        h.key(Key::Enter);
        h.key(Key::ArrowRight);
        let first = sent_long(&h.sent());
        assert!(first > 0, "moved off the minimum");

        let sub = r#"{"FULL_PATH":"/test","CONTENTS":{"my_longlong":{"FULL_PATH":"/test/my_longlong","TYPE":"h","ACCESS":2,"VALUE":[0],"RANGE":[{"MIN":0,"MAX":100}]}}}"#;
        for event in [
            Event::ValueChanged {
                server_id: id,
                path: path.into(),
                value: AttributeReply::Unsupported,
            },
            Event::ValueChanged {
                server_id: id,
                path: path.into(),
                value: AttributeReply::NoContent,
            },
            Event::SubtreeLoaded {
                server_id: id,
                path: "/test".into(),
                raw: Arc::from(sub),
                report: parse_document(sub.as_bytes()).unwrap(),
            },
            tree_loaded(),
        ] {
            h.events.send(event).unwrap();
            h.frame(Vec::new());
            h.frame(Vec::new());
            let t = h.app.state.entry(id).unwrap().tree.as_ref().unwrap();
            let node = t.root.find(path).unwrap();
            assert_eq!(
                shown_value(t, node, false),
                Shown::Sent(first.to_string()),
                "still the sent value, unconfirmed"
            );
            assert_eq!(
                current_f64(node, t.pending.get(path), 0),
                Some(first as f64),
                "the widget stays where it was put"
            );
        }
        // Listens follow the rows on screen; what matters is that nothing is sent or read again.
        let sent: Vec<Command> = h
            .sent()
            .into_iter()
            .filter(|c| !matches!(c, Command::Listen { .. } | Command::Ignore { .. }))
            .collect();
        assert!(sent.is_empty(), "no refresh, no send again: {sent:?}");

        h.key(Key::Enter);
        h.key(Key::ArrowRight);
        assert!(
            sent_long(&h.sent()) > first,
            "the next step starts from the sent value"
        );
    }

    /// A flat tree of `n` methods `/p000`... and one container `/g`.
    fn flat_tree(n: usize) -> String {
        let mut items: Vec<String> = (0..n)
            .map(|i| {
                format!(
                    r#""p{i:03}":{{"FULL_PATH":"/p{i:03}","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}"#
                )
            })
            .collect();
        items.push(
            r#""g":{"FULL_PATH":"/g","CONTENTS":{"x":{"FULL_PATH":"/g/x","TYPE":"f"}}}"#.into(),
        );
        format!(r#"{{"FULL_PATH":"/","CONTENTS":{{{}}}}}"#, items.join(","))
    }

    fn connected(h: &mut Harness, doc: &str) -> ServerId {
        let s = server();
        let id = s.id;
        h.events.send(Event::ServerFound(s)).unwrap();
        h.frame(Vec::new());
        h.app.act(&h.ctx.clone(), Action::SelectServer(id));
        h.events
            .send(Event::TreeLoaded {
                server_id: id,
                raw: Arc::from(doc),
                report: parse_document(doc.as_bytes()).unwrap(),
            })
            .unwrap();
        h.events
            .send(Event::TransportState {
                server_id: id,
                state: oscq_core::TransportState::HttpOnly,
            })
            .unwrap();
        id
    }

    /// A server at `host:port`, connected with `HOST_INFO` sending OSC to it and `doc` loaded.
    fn controllable(h: &mut Harness, host: &str, port: u16, name: &str, doc: &str) -> ServerId {
        let s = DiscoveredServer {
            id: ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: name.into(),
            host: host.into(),
            port,
            addresses: Vec::new(),
            origin: Origin::Manual,
            stale: false,
        };
        let id = s.id;
        h.events.send(Event::ServerFound(s)).unwrap();
        h.frame(Vec::new());
        h.app.act(&h.ctx.clone(), Action::SelectServer(id));
        let (info, _) =
            oscq_core::HostInfo::parse(br#"{"NAME":"x","EXTENSIONS":{"VALUE":true}}"#).unwrap();
        let destination = info.osc_destination(host, port);
        for event in [
            Event::HostInfo {
                server_id: id,
                info,
                destination,
            },
            Event::TreeLoaded {
                server_id: id,
                raw: Arc::from(doc),
                report: parse_document(doc.as_bytes()).unwrap(),
            },
            Event::TransportState {
                server_id: id,
                state: oscq_core::TransportState::HttpOnly,
            },
        ] {
            h.events.send(event).unwrap();
        }
        h.frame(Vec::new());
        id
    }

    /// PRD 9.1: a manual entry is removed with its history line and its saved OSC port; a
    /// discovered server has no remove button and is not removed.
    #[test]
    fn a_manual_entry_is_removed_with_its_osc_port_a_discovered_one_is_not() {
        let mut h = harness_with(Config {
            manual_entries: vec!["localhost:30339".into(), "10.0.0.2:2345".into()],
            osc_ports: [("localhost:30339".to_owned(), 30338)].into(),
            last_selected: Some("localhost:30339".into()),
            ..Config::default()
        });
        let discovered = server();
        let discovered_id = discovered.id;
        h.events.send(Event::ServerFound(discovered)).unwrap();
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert_eq!(
            texts.iter().filter(|t| *t == "x").count(),
            2,
            "one remove button per manual entry, none for the discovered one: {texts:?}"
        );
        let local = h
            .app
            .state
            .servers
            .iter()
            .find(|e| e.key() == "localhost:30339")
            .unwrap()
            .server
            .id;
        assert_eq!(h.app.state.selected, Some(local));
        h.sent();

        h.app.act(&h.ctx.clone(), Action::RemoveManual(local));
        assert!(h.app.state.entry(local).is_none());
        assert_eq!(h.app.state.selected, None);
        assert_eq!(h.app.config.manual_entries, ["10.0.0.2:2345"]);
        assert!(
            h.app.config.osc_ports.is_empty(),
            "its OSC port goes with it"
        );
        assert_eq!(h.app.config.last_selected, None);
        assert!(matches!(
            h.sent().as_slice(),
            [Command::Disconnect { server_id }] if *server_id == local
        ));
        assert_eq!(h.app.history.len(), 1);

        h.app
            .act(&h.ctx.clone(), Action::RemoveManual(discovered_id));
        assert!(
            h.app.state.entry(discovered_id).is_some(),
            "a discovered server stays: it would come back"
        );
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert_eq!(texts.iter().filter(|t| *t == "x").count(), 1);
    }

    /// PRD 9.8: nodes pinned from the tree are listed in their own panel, whatever the tree
    /// shows, are listened to, and are kept per server in the config.
    #[test]
    fn pinned_nodes_stay_in_their_panel_and_persist_per_server() {
        let mut h = harness();
        let id = connected(&mut h, &flat_tree(30));
        h.frame(Vec::new());
        let ctx = h.ctx.clone();
        for path in ["/p025", "/g/x"] {
            h.app.act(&ctx, Action::TogglePin(path.into()));
        }
        let key = h.app.state.entry(id).unwrap().key();
        assert_eq!(h.app.config.pinned_paths[&key], ["/p025", "/g/x"]);

        // Another tab, and a tree rooted elsewhere: the pinned panel still shows both.
        h.app.view.tab = Tab::Traffic;
        h.app.view.prefix = "/g".into();
        h.frame(Vec::new());
        let texts = drawn_texts(&h.frame(Vec::new()));
        for path in ["/p025", "/g/x"] {
            assert!(
                texts.iter().any(|t| t == path),
                "{path} in the panel: {texts:?}"
            );
        }
        let (listen, _) = listen_commands(&h.sent());
        assert!(
            listen.contains(&"/p025".to_owned()) && listen.contains(&"/g/x".to_owned()),
            "pinned methods are listened to: {listen:?}"
        );

        let saved: Config = serde_json::from_slice(&h.app.config.to_bytes()).unwrap();
        assert_eq!(saved.pinned_paths[&key], ["/p025", "/g/x"]);

        h.app.act(&ctx, Action::TogglePin("/p025".into()));
        h.app.act(&ctx, Action::TogglePin("/g/x".into()));
        assert!(!h.app.config.pinned_paths.contains_key(&key), "none left");
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert!(!texts.iter().any(|t| t == "/p025"));
    }

    /// PRD 9.9: capture then recall on the same server sends every captured value as one block;
    /// captured with pins, only the pinned ones; recalled on another server, what matches is
    /// sent and the rest is counted, without blocking. Slots are kept in the config.
    #[test]
    fn a_preset_is_captured_and_recalled_here_and_on_another_server() {
        let mut h = harness();
        let ctx = h.ctx.clone();
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{
            "gain":{"FULL_PATH":"/gain","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
            "name":{"FULL_PATH":"/name","TYPE":"s","ACCESS":3,"VALUE":["kick"]},
            "mute":{"FULL_PATH":"/mute","TYPE":"T","ACCESS":3,"VALUE":[false]},
            "meter":{"FULL_PATH":"/meter","TYPE":"f","ACCESS":1,"VALUE":[0.1]}}}"#;
        let a = controllable(&mut h, "127.0.0.1", 2345, "Max", doc);
        h.sent();
        h.app.act(&ctx, Action::Capture(3));
        let preset = h.app.config.presets[&3].clone();
        assert_eq!(preset.server, "127.0.0.1:2345");
        assert_eq!(preset.values.len(), 3, "the writable methods, not /meter");

        h.app.act(&ctx, Action::Recall(3));
        let blocks: Vec<(String, Vec<String>)> = h
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::SendBlock {
                    server_id,
                    label,
                    messages,
                } if server_id == a => Some((
                    label,
                    messages
                        .iter()
                        .map(|m| m.path.as_str().to_owned())
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            blocks,
            [(
                "preset 03".to_owned(),
                vec!["/gain".to_owned(), "/mute".to_owned(), "/name".to_owned()]
            )],
            "one block holding every value"
        );
        let t = h.app.state.entry(a).unwrap().tree.as_ref().unwrap();
        assert!(t.pending.contains_key("/gain"), "shown as sent until read");

        // With a pin, the capture takes only the pinned node.
        h.app.act(&ctx, Action::TogglePin("/name".into()));
        h.app.act(&ctx, Action::Capture(4));
        assert_eq!(h.app.config.presets[&4].values.len(), 1);

        // Another server: /gain matches, /name has another type, /mute does not exist.
        let other = r#"{"FULL_PATH":"/","CONTENTS":{
            "gain":{"FULL_PATH":"/gain","TYPE":"f","ACCESS":3},
            "name":{"FULL_PATH":"/name","TYPE":"i","ACCESS":3}}}"#;
        let b = controllable(&mut h, "10.0.0.9", 9000, "Helper", other);
        h.sent();
        h.app.act(&ctx, Action::Recall(3));
        let sent: Vec<Vec<String>> = h
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::SendBlock {
                    server_id,
                    messages,
                    ..
                } if server_id == b => Some(
                    messages
                        .iter()
                        .map(|m| m.path.as_str().to_owned())
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(sent, [vec!["/gain".to_owned()]]);
        let note = h.app.view.note.clone().unwrap();
        assert!(
            note.contains("1 sent") && note.contains("2 not found on this server"),
            "{note}"
        );
        assert!(note.contains("captured on Max"), "{note}");

        let saved: Config = serde_json::from_slice(&h.app.config.to_bytes()).unwrap();
        assert_eq!(
            saved.presets, h.app.config.presets,
            "slots kept in the config"
        );
    }

    /// Splits commands into listened and ignored paths, in order.
    fn listen_commands(cmds: &[Command]) -> (Vec<String>, Vec<String>) {
        let mut listen = Vec::new();
        let mut ignore = Vec::new();
        for c in cmds {
            match c {
                Command::Listen { path, .. } => listen.push(path.clone()),
                Command::Ignore { path, .. } => ignore.push(path.clone()),
                _ => {}
            }
        }
        (listen, ignore)
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    /// PRD 7.4: one `LISTEN` per visible or pinned method, none for containers, `IGNORE` when a
    /// method leaves, nothing sent while nothing changes, and all dropped with the connection.
    /// PRD 7.4: listens follow the rows on screen, so a reload that leaves the same rows on screen
    /// sends nothing: no `IGNORE` then `LISTEN` of every visible method. A reload of a subtree, of
    /// the root as after a WebSocket reopens, of the whole tree, and a value read, each with the
    /// document unchanged.
    #[test]
    fn a_reload_that_leaves_the_rows_on_screen_sends_no_listen_or_ignore() {
        let mut h = harness();
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{
            "a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
            "g":{"FULL_PATH":"/g","CONTENTS":{
                "x":{"FULL_PATH":"/g/x","TYPE":"f","ACCESS":3,"VALUE":[0.1]},
                "y":{"FULL_PATH":"/g/y","TYPE":"i","ACCESS":3,"VALUE":[2]}}}}}"#;
        let sub = r#"{"FULL_PATH":"/g","CONTENTS":{
            "x":{"FULL_PATH":"/g/x","TYPE":"f","ACCESS":3,"VALUE":[0.1]},
            "y":{"FULL_PATH":"/g/y","TYPE":"i","ACCESS":3,"VALUE":[2]}}}"#;
        let id = connected(&mut h, doc);
        h.frame(Vec::new());
        h.app.act(&h.ctx.clone(), Action::Toggle("/g".into()));
        for _ in 0..3 {
            h.frame(Vec::new());
        }
        let visible = sorted(h.app.view.visible_methods.clone());
        assert_eq!(visible, ["/a", "/g/x", "/g/y"]);
        h.sent();
        for event in [
            Event::SubtreeLoaded {
                server_id: id,
                path: "/g".into(),
                raw: Arc::from(sub),
                report: parse_document(sub.as_bytes()).unwrap(),
            },
            // The reload after a lost WebSocket reopens.
            Event::SubtreeLoaded {
                server_id: id,
                path: "/".into(),
                raw: Arc::from(doc),
                report: parse_document(doc.as_bytes()).unwrap(),
            },
            Event::TreeLoaded {
                server_id: id,
                raw: Arc::from(doc),
                report: parse_document(doc.as_bytes()).unwrap(),
            },
            Event::ValueChanged {
                server_id: id,
                path: "/g/x".into(),
                value: AttributeReply::Value(serde_json::json!([0.2])),
            },
        ] {
            h.events.send(event).unwrap();
            for _ in 0..3 {
                h.frame(Vec::new());
            }
            assert_eq!(sorted(h.app.view.visible_methods.clone()), visible);
            let (listen, ignore) = listen_commands(&h.sent());
            assert!(
                listen.is_empty() && ignore.is_empty(),
                "listen {listen:?} ignore {ignore:?}"
            );
        }
    }

    #[test]
    fn listens_follow_the_visible_and_pinned_methods() {
        let mut config = Config::default();
        config.pinned_paths.insert(
            "127.0.0.1:2345".into(),
            vec!["/p399".into(), "/nope".into(), "/g".into()],
        );
        let mut h = harness_with(config);
        let id = connected(&mut h, &flat_tree(400));
        h.frame(Vec::new());
        h.frame(Vec::new());
        let (listen, ignore) = listen_commands(&h.sent());
        assert!(ignore.is_empty());
        let visible = h.app.view.visible_methods.clone();
        assert!(
            (10..100).contains(&visible.len()),
            "only the rows on screen: {}",
            visible.len()
        );
        assert!(!visible.contains(&"/p399".to_owned()));
        let mut expected = visible.clone();
        expected.push("/p399".into());
        assert_eq!(sorted(listen), sorted(expected.clone()));
        assert!(h.sent().is_empty(), "no command while nothing changes");
        h.frame(Vec::new());
        assert!(h.sent().is_empty(), "no command while nothing changes");

        h.app.view.selected_path = Some("/p300".into());
        h.app.view.scroll_to_selected = true;
        h.frame(Vec::new());
        h.frame(Vec::new());
        let (listen, ignore) = listen_commands(&h.sent());
        let now = h.app.view.visible_methods.clone();
        assert!(now.contains(&"/p300".to_owned()));
        assert_eq!(
            sorted(ignore),
            sorted(
                visible
                    .iter()
                    .filter(|p| !now.contains(p))
                    .cloned()
                    .collect()
            ),
            "every method that left"
        );
        assert_eq!(
            sorted(listen),
            sorted(
                now.iter()
                    .filter(|p| !visible.contains(p) && *p != "/p399")
                    .cloned()
                    .collect()
            ),
            "every method that came in, the pinned one already listened"
        );
        let entry = h.app.state.entry(id).unwrap();
        let mut all = now.clone();
        all.push("/p399".into());
        all.dedup();
        assert_eq!(
            sorted(entry.listening.iter().cloned().collect()),
            sorted(all)
        );

        h.events
            .send(Event::TransportState {
                server_id: id,
                state: oscq_core::TransportState::Disconnected,
            })
            .unwrap();
        h.frame(Vec::new());
        assert!(
            h.sent().is_empty(),
            "the service drops them with the connection"
        );
        assert!(h.app.state.entry(id).unwrap().listening.is_empty());
    }

    fn count_causes(causes: &[String]) -> std::collections::BTreeMap<String, usize> {
        let mut counted: std::collections::BTreeMap<String, usize> = Default::default();
        for c in causes {
            let loc = c
                .split("/src/")
                .last()
                .unwrap_or_default()
                .trim()
                .to_owned();
            *counted.entry(loc).or_default() += 1;
        }
        counted
    }

    /// PRD 6.2: input wakes the window, then it goes back to sleep. The pointer moved onto a
    /// tree row, whose name has a tooltip, then near the scroll bar, each time left still: the
    /// frames egui asks for afterwards are bounded, none comes from this app's code, and once
    /// still nothing more is asked for.
    #[test]
    fn a_pointer_move_costs_a_bounded_number_of_frames_then_none() {
        let mut h = harness();
        connected(&mut h, &flat_tree(400));
        // Load and the scroll bar appearing, then rest.
        let (_, mut t, _) = h.frames_until_idle(0.0, &[]);
        for (place, x0, y) in [
            ("over a row name, with its tooltip", 260.0, 100.0),
            ("over a row, no tooltip", 300.0, 90.0),
            ("near the scroll bar", 1080.0, 300.0),
        ] {
            let path: Vec<egui::Pos2> =
                (0..6).map(|i| egui::pos2(x0 + 2.0 * i as f32, y)).collect();
            let (frames, end, causes) = h.frames_until_idle(t + 5.0, &path);
            println!(
                "POINTER {place}: {frames} frames for 6 moves, causes {:?}",
                count_causes(&causes)
            );
            // Only what input needs: egui's frame after input, the tooltip's delay, the tooltip
            // laid out then shown. No fade, no waiting for a perfectly still pointer.
            for c in &causes {
                assert!(
                    [
                        "context.rs:538",
                        "tooltip.rs:374",
                        "area.rs:554",
                        "area.rs:686"
                    ]
                    .iter()
                    .any(|allowed| c.contains(allowed)),
                    "{place}: frame asked for by {c}"
                );
            }
            let out = h.frame_at(end + 1.0 / 60.0, Vec::new());
            assert_eq!(idle_delay(&out), Duration::MAX, "still and idle");
            assert!(frames <= MOVE_FRAMES, "{place}: {frames} frames");
            t = end;
        }
    }

    /// Frames for six pointer moves and the stillness after them: one per move, egui's frame
    /// after the last input, and a tooltip waited for then laid out and shown. Measured 8 away
    /// from tooltips and 12 on a row name; 14 and 28 before `calm_style`.
    const MOVE_FRAMES: usize = 12;

    /// PRD 9.6: no WebSocket, a WebSocket accepted and silent, and a WebSocket that notifies are
    /// three states the window tells apart, and only the last one is called live.
    #[test]
    fn no_websocket_silent_websocket_and_notifying_websocket_are_told_apart() {
        let mut h = harness();
        let id = connected(&mut h, &flat_tree(3));
        let mut seen = Vec::new();
        for state in [
            None,
            Some(oscq_core::TransportState::WebSocketOpen),
            Some(oscq_core::TransportState::WebSocketLive),
        ] {
            if let Some(state) = state {
                h.events
                    .send(Event::TransportState {
                        server_id: id,
                        state,
                    })
                    .unwrap();
            }
            h.frame(Vec::new());
            let e = h.app.state.entry(id).unwrap();
            let (text, warning) = values_text(e).unwrap();
            assert!(!warning);
            seen.push((e.ws.clone(), text, conn_text(e), dot_colour(e)));
        }
        let [absent, open, live] = &seen[..] else {
            panic!()
        };
        assert_eq!(absent.0, WsState::Absent);
        assert_eq!(absent.1, "values on refresh only");
        assert_eq!(absent.2, "connected over HTTP");
        assert_eq!(open.0, WsState::Open);
        assert_eq!(
            open.1,
            "WebSocket connected, no update received yet: values on refresh only"
        );
        assert_eq!(open.2, "connected, WebSocket open, nothing received yet");
        assert_eq!(live.0, WsState::Live);
        assert_eq!(live.1, "values live over the WebSocket");
        assert_eq!(live.2, "connected, WebSocket live");
        for (a, b) in [(absent, open), (open, live), (absent, live)] {
            assert!(a.1 != b.1 && a.2 != b.2 && a.3 != b.3, "{a:?} {b:?}");
        }
        assert!(
            !absent.1.contains("live") && !open.1.contains("live") && !open.2.contains("live"),
            "only a notifying socket is called live"
        );
    }

    /// A value the server pushes replaces what is shown, and the status says where values come
    /// from.
    #[test]
    fn a_pushed_value_is_shown_as_read() {
        let mut h = harness();
        let id = connected(&mut h, &flat_tree(3));
        h.frame(Vec::new());
        for state in [
            oscq_core::TransportState::WebSocketLive,
            oscq_core::TransportState::WebSocketLost {
                retry_in: Duration::from_millis(500),
                reason: "closed by the server".into(),
            },
            oscq_core::TransportState::WebSocketLive,
        ] {
            h.events
                .send(Event::TransportState {
                    server_id: id,
                    state,
                })
                .unwrap();
        }
        h.events
            .send(Event::ValueChanged {
                server_id: id,
                path: "/p001".into(),
                value: AttributeReply::Value(serde_json::json!([0.75])),
            })
            .unwrap();
        h.frame(Vec::new());
        let e = h.app.state.entry(id).unwrap();
        assert_eq!(e.ws, WsState::Live);
        assert_eq!(conn_text(e), "connected, WebSocket live");
        let t = e.tree.as_ref().unwrap();
        let node = t.node("/p001").unwrap();
        assert_eq!(shown_value(t, node, true), Shown::Read("0.750".into()));
    }

    /// The window on the real network service, connected to a synthetic server publishing the
    /// 5000 node document over HTTP and WebSocket, with OSC received on `osc`.
    fn live_harness(
        config: impl FnOnce(&str) -> Config,
        extra: Vec<(String, crate::synthetic::Reply)>,
    ) -> (
        Harness,
        crate::synthetic::SyntheticServer,
        crate::synthetic::OscReceiver,
    ) {
        use crate::synthetic::{OscReceiver, SyntheticServer, borrowed, large_routes};
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let osc = rt.block_on(OscReceiver::bind("127.0.0.1:0")).unwrap();
        let mut routes = large_routes(5000, osc.port());
        routes.extend(extra);
        let server = rt
            .block_on(SyntheticServer::start_ws("127.0.0.1:0", borrowed(&routes)))
            .unwrap();
        let (ev_tx, ev_rx) = flume::unbounded();
        let commands = oscq_core::service::spawn(rt.handle(), ev_tx, Arc::new(|| {}));
        let ctx = egui::Context::default();
        let app = VoidApp::with_backend(
            &ctx,
            Backend {
                commands,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            config(&format!("127.0.0.1:{}", server.addr.port())),
            None,
        );
        let mut h = Harness {
            ctx,
            app,
            commands: flume::unbounded().1,
            events: flume::unbounded().0,
            _rt: rt,
        };
        wait_for(&mut h, "connected and live", |a| {
            a.state.selected_entry().is_some_and(|e| {
                e.tree.is_some() && matches!(e.destination, Some(Ok(_))) && e.ws == WsState::Open
            })
        });
        (h, server, osc)
    }

    fn wait_for(h: &mut Harness, what: &str, done: impl Fn(&VoidApp) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(&h.app) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            h.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// PRD 9.6, as met with Max 9: the restored server is connected at launch before it serves,
    /// the attempt fails, a later one succeeds. The status bar then shows the connection, not the
    /// superseded failure. Real service, synthetic server started on the refused port.
    #[test]
    fn a_failed_connection_is_not_shown_once_a_new_one_succeeds() {
        use crate::synthetic::{SyntheticServer, borrowed, large_routes};
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let key = format!("127.0.0.1:{port}");
        let (ev_tx, ev_rx) = flume::unbounded();
        let commands = oscq_core::service::spawn(rt.handle(), ev_tx, Arc::new(|| {}));
        let ctx = egui::Context::default();
        let app = VoidApp::with_backend(
            &ctx,
            Backend {
                commands,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            Config {
                manual_entries: vec![key.clone()],
                last_selected: Some(key.clone()),
                ..Config::default()
            },
            None,
        );
        let mut h = Harness {
            ctx,
            app,
            commands: flume::unbounded().1,
            events: flume::unbounded().0,
            _rt: rt,
        };
        wait_for(&mut h, "the launch attempt to fail", |a| {
            a.state
                .selected_entry()
                .is_some_and(|e| matches!(e.conn, ConnState::Failed(_)))
        });
        let e = h.app.state.selected_entry().unwrap();
        let text = conn_text(e);
        assert!(text.starts_with("error: could not connect to "), "{text}");
        assert_eq!(
            error_text(e),
            None,
            "the failure is said once, by the connection"
        );
        assert!(e.last_error.is_some());

        let routes = large_routes(10, 9);
        let _server = h
            ._rt
            .block_on(SyntheticServer::start_ws(&key, borrowed(&routes)))
            .unwrap();
        let id = h.app.state.selected.unwrap();
        h.app.act(&h.ctx.clone(), Action::SelectServer(id));
        wait_for(&mut h, "connected with its WebSocket open", |a| {
            a.state.selected_entry().is_some_and(|e| {
                e.conn == ConnState::Connected && e.tree.is_some() && e.ws == WsState::Open
            })
        });
        h.frame(Vec::new());
        let e = h.app.state.selected_entry().unwrap();
        assert_eq!(error_text(e), None, "the superseded failure is gone");
        assert_eq!(e.last_error, None);
    }

    /// Every text the frame drew.
    fn drawn_texts(out: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, texts)),
                egui::Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts
    }

    /// Every text the frame drew, with its layout rectangle on screen and the clip it was drawn in.
    fn drawn_text_rects(out: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
        fn walk(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, out)),
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    clip,
                )),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, clipped.clip_rect, &mut texts);
        }
        texts
    }

    /// What the frame shows of each text: the characters of the glyphs laid out, an elided
    /// text's ellipsis included, not the text it was given, which `Galley::text` returns whole.
    /// With where it was drawn and its clip.
    fn shown_texts(out: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
        fn walk(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, out)),
                egui::Shape::Text(t) => {
                    let shown: String = t
                        .galley
                        .rows
                        .iter()
                        .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
                        .collect();
                    out.push((shown, t.galley.rect.translate(t.pos.to_vec2()), clip));
                }
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, clipped.clip_rect, &mut texts);
        }
        texts
    }

    /// A click at the centre of `rect`: pointer moved there, pressed, released.
    fn click(h: &mut Harness, rect: egui::Rect, button: egui::PointerButton) {
        let p = rect.center();
        h.frame(vec![egui::Event::PointerMoved(p)]);
        for pressed in [true, false] {
            h.frame(vec![egui::Event::PointerButton {
                pos: p,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
        h.frame(vec![egui::Event::PointerGone]);
    }

    /// The place of the only drawn text equal to `text`.
    fn place(out: &egui::FullOutput, text: &str) -> egui::Rect {
        let found: Vec<_> = drawn_text_rects(out)
            .into_iter()
            .filter(|(t, _, _)| t == text)
            .collect();
        assert_eq!(found.len(), 1, "{text:?} drawn once");
        found[0].1
    }

    /// On a wide window, 1060 points or more, under the default width of 1100, the side panes keep
    /// their width, the centre takes the rest, and the toolbar holds on one line: the three tabs,
    /// Refresh, Writable, the filter and the root fields side by side, frame after frame.
    #[test]
    fn the_toolbar_holds_one_line_on_a_wide_window() {
        const CONTROLS: [&str; 7] = [
            "Tree",
            "JSON",
            "Traffic",
            "Refresh",
            "Writable",
            "filter  /",
            "root at path",
        ];
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        for width in [1060.0, 1100.0, 1440.0, 1800.0, 2560.0, 3840.0] {
            let mut h = harness();
            controllable(&mut h, "127.0.0.1", 2345, "S", doc);
            h.app
                .config
                .pinned_paths
                .insert("127.0.0.1:2345".into(), vec!["/a".into()]);
            let size = egui::vec2(width, 900.0);
            let mut out = h.frame_sized(size, Vec::new());
            for _ in 0..50 {
                out = h.frame_sized(size, Vec::new());
            }
            for (pane, expected) in [("servers", 56.0 * GRID), ("work", style::WORK_DEFAULT)] {
                let w = egui::PanelState::load(&h.ctx, egui::Id::new(pane))
                    .unwrap()
                    .outer_rect
                    .width();
                assert!(
                    (w - expected).abs() < 0.5,
                    "the {pane} pane {w} px wide, not {expected}, window {width}"
                );
            }
            let rows: Vec<f32> = CONTROLS.iter().map(|c| place(&out, c).center().y).collect();
            let (top, bottom) = rows
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
            assert!(
                bottom - top < ROW_H / 2.0,
                "the toolbar on more than one line at {width}: {:?}",
                CONTROLS.iter().zip(&rows).collect::<Vec<_>>()
            );
        }
    }

    /// A window of 900 points, a Retina screen's 1800 pixels: the centre holds the tabs and
    /// Refresh on one line and the two fields together on the next, never three lines.
    #[test]
    fn at_900_points_the_toolbar_takes_two_lines_the_fields_together() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        let mut h = harness();
        controllable(&mut h, "127.0.0.1", 2345, "S", doc);
        let size = egui::vec2(900.0, 700.0);
        h.frame_sized(size, Vec::new());
        let out = h.frame_sized(size, Vec::new());
        let y = |c: &str| place(&out, c).center().y;
        for c in ["JSON", "Traffic", "Refresh"] {
            assert!((y(c) - y("Tree")).abs() < 2.0, "{c} beside Tree");
        }
        assert!(
            (y("filter  /") - y("root at path")).abs() < 2.0,
            "the two fields on one line"
        );
        assert!(
            y("filter  /") - y("Tree") < 1.5 * ROW_H,
            "the fields on the line right after the tabs"
        );
    }

    /// In filter mode a hit shows its name first, then the containers nearest to it: two hits of
    /// one name under long common containers, told apart only by their parents, stay distinct on
    /// screen, each label whole inside its clip.
    #[test]
    fn filter_hits_of_one_name_in_different_containers_stay_distinct() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"vdmx":{"FULL_PATH":"/vdmx","CONTENTS":{
            "layers":{"FULL_PATH":"/vdmx/layers","CONTENTS":{
            "layer_one":{"FULL_PATH":"/vdmx/layers/layer_one","CONTENTS":{
            "effects":{"FULL_PATH":"/vdmx/layers/layer_one/effects","CONTENTS":{
            "fader":{"FULL_PATH":"/vdmx/layers/layer_one/effects/fader","CONTENTS":{
              "normalized":{"FULL_PATH":"/vdmx/layers/layer_one/effects/fader/normalized","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}},
            "gain":{"FULL_PATH":"/vdmx/layers/layer_one/effects/gain","CONTENTS":{
              "normalized":{"FULL_PATH":"/vdmx/layers/layer_one/effects/gain/normalized","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}
            }}}}}}}}}}"#;
        for width in [900.0, 1100.0, 1800.0] {
            let mut h = harness();
            controllable(&mut h, "127.0.0.1", 2345, "S", doc);
            // Not itself the start of a hit's label: the field's text is not taken for a hit.
            h.app.view.filter = "rmalized".into();
            let size = egui::vec2(width, 700.0);
            h.frame_sized(size, Vec::new());
            let out = h.frame_sized(size, Vec::new());
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let hits: Vec<_> = shown_texts(&out)
                .into_iter()
                .filter(|(t, _, _)| t.starts_with("normalized"))
                .collect();
            assert_eq!(hits.len(), 2, "two hits, named first, at {width}: {hits:?}");
            assert_ne!(
                hits[0].0, hits[1].0,
                "the two hits look the same on screen at {width}"
            );
            for (t, r, c) in &hits {
                assert!(
                    c.intersect(screen).expand(0.5).contains_rect(*r),
                    "{t:?} cut by its clip at {width}"
                );
            }
        }
    }

    /// When every hit carries its own name, as hits in one container do, no container path is
    /// shown: the name takes the whole column. The case of Max, where the hits of a parameter
    /// showed three different cuts of the same path.
    #[test]
    fn filter_hits_of_distinct_names_show_no_path() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"test":{"FULL_PATH":"/test","CONTENTS":{
            "param":{"FULL_PATH":"/test/param","CONTENTS":{
            "fader":{"FULL_PATH":"/test/param/fader","CONTENTS":{
              "normalized":{"FULL_PATH":"/test/param/fader/normalized","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
              "raw":{"FULL_PATH":"/test/param/fader/raw","TYPE":"f","ACCESS":3,"VALUE":[64.0]}}},
            "gain":{"FULL_PATH":"/test/param/gain","CONTENTS":{
              "level":{"FULL_PATH":"/test/param/gain/level","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}
            }}}}}}"#;
        for (filter, names) in [
            // Three hits, one container: name alone.
            ("fader/", vec!["normalized", "raw"]),
            // Two containers, names apart: name alone.
            ("l", vec!["normalized", "level"]),
        ] {
            let mut h = harness();
            controllable(&mut h, "127.0.0.1", 2345, "S", doc);
            h.app.view.filter = filter.into();
            h.frame(Vec::new());
            let shown: Vec<String> = shown_texts(&h.frame(Vec::new()))
                .into_iter()
                .map(|(t, _, _)| t)
                .collect();
            // Each hit's label is its name and nothing after it.
            for name in names {
                let labels: Vec<_> = shown.iter().filter(|t| t.starts_with(name)).collect();
                assert_eq!(
                    labels,
                    [name],
                    "filter {filter:?}: {name:?} shown alone, no path after it"
                );
            }
        }
    }

    /// Writable only: the tree hides the methods that cannot be written and the containers left
    /// with none, keeps a container whose children are not known, hides read only hits of the
    /// filter too, and the status bar counts what is hidden. The setting is kept in the config.
    #[test]
    fn writable_only_hides_read_only_nodes_and_says_how_many() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{
            "normalized":{"FULL_PATH":"/normalized","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
            "longname":{"FULL_PATH":"/longname","TYPE":"s","ACCESS":1,"VALUE":["Fader"]},
            "exponent":{"FULL_PATH":"/exponent","TYPE":"f","ACCESS":1,"VALUE":[1.0]},
            "info":{"FULL_PATH":"/info","CONTENTS":{
              "min":{"FULL_PATH":"/info/min","TYPE":"f","ACCESS":1,"VALUE":[0.0]},
              "max":{"FULL_PATH":"/info/max","TYPE":"f","ACCESS":1,"VALUE":[1.0]}}},
            "later":{"FULL_PATH":"/later","CONTENTS":{}},
            "mixed":{"FULL_PATH":"/mixed","CONTENTS":{
              "gain":{"FULL_PATH":"/mixed/gain","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
              "type":{"FULL_PATH":"/mixed/type","TYPE":"s","ACCESS":1,"VALUE":["float"]}}}}}"#;
        let mut h = harness();
        controllable(&mut h, "127.0.0.1", 2345, "S", doc);
        let texts = drawn_texts(&h.frame(Vec::new()));
        for name in [
            "normalized",
            "longname",
            "exponent",
            "info",
            "later",
            "mixed",
        ] {
            assert!(texts.iter().any(|t| t == name), "{name} shown before");
        }
        let out = h.frame(Vec::new());
        click(
            &mut h,
            place(&out, "Writable"),
            egui::PointerButton::Primary,
        );
        assert!(h.app.config.hide_read_only, "the toggle set");
        h.frame(Vec::new());
        let texts = drawn_texts(&h.frame(Vec::new()));
        for name in ["normalized", "later", "mixed"] {
            assert!(
                texts.iter().any(|t| t == name),
                "{name} still shown: {texts:?}"
            );
        }
        for name in ["longname", "exponent", "info"] {
            assert!(!texts.iter().any(|t| t == name), "{name} hidden: {texts:?}");
        }
        // longname, exponent, info, min, max, type.
        assert!(
            texts.iter().any(|t| t.contains("nodes, 6 hidden")),
            "the count in the status bar: {texts:?}"
        );
        // The filter hides them too.
        // Not the start of a label itself: the field's text is not taken for a hit.
        h.app.view.filter = "ongname".into();
        h.frame(Vec::new());
        let shown: Vec<String> = shown_texts(&h.frame(Vec::new()))
            .into_iter()
            .map(|(t, _, _)| t)
            .collect();
        assert!(
            !shown.iter().any(|t| t.starts_with("longname")),
            "no read only hit: {shown:?}"
        );
        // Kept in the config; an older config without the field reads as off.
        let saved: Config = serde_json::from_slice(&h.app.config.to_bytes()).unwrap();
        assert!(saved.hide_read_only);
        let older: Config = serde_json::from_str(r#"{"manual_entries":[]}"#).unwrap();
        assert!(!older.hide_read_only && !older.presets_open);
    }

    /// The preset section is folded until opened: its header only, no slot, and the pinned
    /// panel takes the room. Opened by its header, it shows the sixteen slots, and stays open in
    /// the config.
    #[test]
    fn the_preset_section_is_folded_until_opened() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        let mut h = harness();
        controllable(&mut h, "127.0.0.1", 2345, "S", doc);
        let out = h.frame(Vec::new());
        let texts = drawn_texts(&out);
        assert!(!h.app.config.presets_open, "folded by default");
        assert!(
            !texts.iter().any(|t| t == "Recall" || t == "Capture"),
            "no slot while folded"
        );
        click(
            &mut h,
            place(&out, "+ PRESETS"),
            egui::PointerButton::Primary,
        );
        assert!(h.app.config.presets_open, "opened by its header");
        let out = h.frame(Vec::new());
        let texts = drawn_texts(&out);
        assert_eq!(
            texts.iter().filter(|t| *t == "Recall").count(),
            presets::SLOTS
        );
        assert!(texts.iter().any(|t| t == "- PRESETS"));
        let saved: Config = serde_json::from_slice(&h.app.config.to_bytes()).unwrap();
        assert!(saved.presets_open, "kept open in the config");
    }

    /// WCAG contrast of two colours, the first blended onto `ground` when translucent.
    fn contrast(c: Color32, ground: Color32) -> f32 {
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let a = a as f32 / 255.0;
        let mix = |x: u8, y: u8| (x as f32 * a + y as f32 * (1.0 - a)).round() as u8;
        let c = Color32::from_rgb(mix(r, ground.r()), mix(g, ground.g()), mix(b, ground.b()));
        let (l1, l2) = (lum(c), lum(ground));
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }

    /// The light theme reads as the dark one does: each control stands out from the panel at
    /// least as much, text and weak text at least as much, a control's text on its own ground at
    /// least as much. The dark theme is egui's, unchanged.
    #[test]
    fn the_light_theme_contrasts_as_the_dark_one_does() {
        let h = harness();
        let dark = h.ctx.style_of(egui::Theme::Dark).visuals.clone();
        let light = h.ctx.style_of(egui::Theme::Light).visuals.clone();
        assert_eq!(dark, egui::Visuals::dark(), "the dark theme unchanged");
        // A button or field at rest: its ground, or its edge when it has one.
        fn edge(v: &egui::Visuals) -> f32 {
            let w = &v.widgets.inactive;
            let ground = contrast(w.weak_bg_fill, v.panel_fill);
            if w.bg_stroke.width > 0.0 {
                ground.max(contrast(w.bg_stroke.color, v.panel_fill))
            } else {
                ground
            }
        }
        type Measure = fn(&egui::Visuals) -> f32;
        let measures: [(&str, Measure); 5] = [
            ("a button or field against the panel", edge),
            ("a slider rail against the panel", |v| {
                contrast(v.widgets.inactive.bg_fill, v.panel_fill)
            }),
            ("text against the panel", |v| {
                contrast(v.text_color(), v.panel_fill)
            }),
            ("weak text against the panel", |v| {
                contrast(v.weak_text_color(), v.panel_fill)
            }),
            ("a control's text on its ground", |v| {
                contrast(
                    v.widgets.inactive.fg_stroke.color,
                    v.widgets.inactive.weak_bg_fill,
                )
            }),
        ];
        for (what, measure) in measures {
            let (d, l) = (measure(&dark), measure(&light));
            println!("CONTRAST {what}: dark {d:.2}, light {l:.2}");
            assert!(l >= d, "{what}: light {l:.2} below dark {d:.2}");
        }
        assert!(contrast(light.text_color(), light.panel_fill) >= 4.5);
    }

    /// A pinned node is unpinned from the pinned panel by the button on its row, as from the
    /// tree by its menu; the context menu of the panel row unpins too.
    #[test]
    fn a_pinned_node_is_unpinned_from_its_panel() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]},"b":{"FULL_PATH":"/b","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        let mut h = harness();
        controllable(&mut h, "127.0.0.1", 2345, "S", doc);
        let key = "127.0.0.1:2345".to_owned();
        h.app
            .config
            .pinned_paths
            .insert(key.clone(), vec!["/a".into()]);
        let out = h.frame(Vec::new());
        click(&mut h, place(&out, "\u{d7}"), egui::PointerButton::Primary);
        assert!(
            !h.app.config.pinned_paths.contains_key(&key),
            "unpinned by its button"
        );
        // And by the menu of the row, the other way in.
        h.app
            .config
            .pinned_paths
            .insert(key.clone(), vec!["/b".into()]);
        let out = h.frame(Vec::new());
        let label = place(&out, "/b");
        click(&mut h, label, egui::PointerButton::Secondary);
        let out = h.frame(Vec::new());
        click(&mut h, place(&out, "Unpin"), egui::PointerButton::Primary);
        assert!(
            !h.app.config.pinned_paths.contains_key(&key),
            "unpinned by its menu"
        );
    }

    /// Each preset slot names its two actions, and a slot holding values is told from an empty
    /// one at a glance: tinted ground and plain number against a faint one. Capture stores, Recall
    /// sends, and an empty slot's Recall does nothing.
    #[test]
    fn preset_slots_name_their_actions_and_show_what_they_hold() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        let mut h = harness();
        controllable(&mut h, "127.0.0.1", 2345, "S", doc);
        h.app.config.presets_open = true;
        let out = h.frame(Vec::new());
        let texts = drawn_texts(&out);
        for action in ["Recall", "Capture"] {
            assert_eq!(
                texts.iter().filter(|t| *t == action).count(),
                presets::SLOTS,
                "{action} on every slot"
            );
        }
        // The Capture of slot 01: on the row of "01", the nearest to its right.
        let one = place(&out, "01");
        let capture = drawn_text_rects(&out)
            .into_iter()
            .filter(|(t, r, _)| t == "Capture" && (r.center().y - one.center().y).abs() < 2.0)
            .map(|(_, r, _)| r)
            .filter(|r| r.left() > one.right())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap();
        click(&mut h, capture, egui::PointerButton::Primary);
        assert!(
            h.app.config.presets.contains_key(&1),
            "Capture stored slot 01"
        );
        let out = h.frame(Vec::new());
        // Held and empty, told apart: the tint behind the slot and the colour of its number.
        let fill = h
            .ctx
            .global_style()
            .visuals
            .selection
            .bg_fill
            .gamma_multiply(0.35);
        let tinted = |r: egui::Rect| {
            painted_spans(&out, fill, r.center().y)
                .iter()
                .any(|(lo, hi)| *lo <= r.center().x && r.center().x <= *hi)
        };
        let (one, two) = (place(&out, "01"), place(&out, "02"));
        assert!(tinted(one), "the held slot 01 is tinted");
        assert!(!tinted(two), "the empty slot 02 is not");
        let colour = |text: &str| {
            fn walk(shape: &egui::Shape, text: &str, out: &mut Vec<Color32>) {
                match shape {
                    egui::Shape::Vec(s) => s.iter().for_each(|s| walk(s, text, out)),
                    egui::Shape::Text(t) if t.galley.text() == text => {
                        let c = t.galley.job.sections[0].format.color;
                        out.push(if c == Color32::PLACEHOLDER {
                            t.fallback_color
                        } else {
                            c
                        });
                    }
                    _ => {}
                }
            }
            let mut found = Vec::new();
            out.shapes
                .iter()
                .for_each(|c| walk(&c.shape, text, &mut found));
            found[0]
        };
        assert_ne!(
            colour("01"),
            colour("02"),
            "a held number reads apart from an empty one"
        );
        // Recall: slot 01 sends, the empty slot 02 does nothing.
        let recall_of = |out: &egui::FullOutput, row: egui::Rect| {
            drawn_text_rects(out)
                .into_iter()
                .filter(|(t, r, _)| t == "Recall" && (r.center().y - row.center().y).abs() < 2.0)
                .map(|(_, r, _)| r)
                .filter(|r| r.left() > row.right())
                .min_by(|a, b| a.left().total_cmp(&b.left()))
                .unwrap()
        };
        h.app.view.note = None;
        click(&mut h, recall_of(&out, two), egui::PointerButton::Primary);
        assert_eq!(h.app.view.note, None, "an empty slot recalls nothing");
        click(&mut h, recall_of(&out, one), egui::PointerButton::Primary);
        assert!(
            h.app
                .view
                .note
                .as_deref()
                .is_some_and(|n| n.starts_with("Slot 01 recalled")),
            "{:?}",
            h.app.view.note
        );
    }

    /// The toolbar of the centre never runs off screen or under a side pane: from the narrowest
    /// window up, with the side panes at their default width or dragged as wide as they go, each
    /// of its controls is drawn once, whole, inside its clip and inside the window. Refresh above
    /// all: with a server that never notifies, it is the only way to read a value again. The same
    /// for the sixteen preset slots, the unpin button, and every text along the bottom of the
    /// window: the theme and About under the server list, the status bar and its message.
    #[test]
    fn every_toolbar_control_stays_on_screen_whatever_the_width() {
        const CONTROLS: [&str; 7] = [
            "filter  /",
            "root at path",
            "Tree",
            "JSON",
            "Traffic",
            "Refresh",
            "Writable",
        ];
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        for width in [
            640.0, 720.0, 800.0, 960.0, 1100.0, 1440.0, 1920.0, 2560.0, 3840.0,
        ] {
            for dragged in [false, true] {
                let mut h = harness();
                controllable(&mut h, "127.0.0.1", 2345, "S", doc);
                h.app
                    .config
                    .pinned_paths
                    .insert("127.0.0.1:2345".into(), vec!["/a".into()]);
                h.app.view.note = Some("a note long enough to need the whole width ".repeat(4));
                h.app.config.presets_open = true;
                if dragged {
                    // Both side panes as a previous session left them, wider than any window.
                    for pane in ["servers", "work"] {
                        h.ctx.data_mut(|d| {
                            d.insert_persisted(
                                egui::Id::new(pane),
                                egui::PanelState {
                                    outer_rect: egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(2000.0, 600.0),
                                    ),
                                },
                            )
                        });
                    }
                }
                let size = egui::vec2(width, 600.0);
                h.frame_sized(size, Vec::new());
                let out = h.frame_sized(size, Vec::new());
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let drawn = drawn_text_rects(&out);
                let whole = |text: &str, rect: &egui::Rect, clip: &egui::Rect| {
                    assert!(
                        // Half a pixel of slack: clip rectangles are rounded to the pixel.
                        clip.intersect(screen).expand(0.5).contains_rect(*rect),
                        "{text:?} at {rect:?} cut by {clip:?} at width {width}, panes dragged {dragged}"
                    );
                };
                for control in CONTROLS {
                    let found: Vec<_> = drawn.iter().filter(|(t, _, _)| t == control).collect();
                    assert_eq!(
                        found.len(),
                        1,
                        "{control:?} drawn once at width {width}, panes dragged {dragged}"
                    );
                    whole(control, &found[0].1, &found[0].2);
                }
                let slots: Vec<_> = drawn
                    .iter()
                    .filter(|(t, _, _)| {
                        t == "Recall"
                            || t == "Capture"
                            || t.parse::<usize>().is_ok_and(|n| (1..=16).contains(&n))
                    })
                    .collect();
                assert_eq!(
                    slots.len(),
                    3 * presets::SLOTS,
                    "every slot, its number and its two buttons, at width {width}"
                );
                slots.iter().for_each(|(t, r, c)| whole(t, r, c));
                // The access flags close each row on the right, of the tree and of the pinned
                // panel: seen whole, the widget before them is on screen too.
                let flags: Vec<_> = drawn.iter().filter(|(t, _, _)| t == "rw").collect();
                assert_eq!(
                    flags.len(),
                    2,
                    "the tree row and the pinned row at width {width}"
                );
                flags.iter().for_each(|(t, r, c)| whole(t, r, c));
                let pinned: Vec<_> = drawn.iter().filter(|(t, _, _)| t == "/a").collect();
                assert_eq!(pinned.len(), 1, "the pinned row at width {width}");
                whole("/a", &pinned[0].1, &pinned[0].2);
                let unpin: Vec<_> = drawn.iter().filter(|(t, _, _)| t == "\u{d7}").collect();
                assert_eq!(unpin.len(), 1, "the unpin button at width {width}");
                whole("unpin", &unpin[0].1, &unpin[0].2);
                let band = size.y - ROW_H - 2.0 * GRID;
                let status: Vec<_> = drawn.iter().filter(|(_, r, _)| r.min.y >= band).collect();
                assert!(
                    status.len() >= 4,
                    "the status bar drawn at width {width}: {status:?}"
                );
                status.iter().for_each(|(t, r, c)| whole(t, r, c));
            }
        }
    }

    /// The about box draws its six pieces of information, each found by its text among what the
    /// frame painted, whole, inside the window, inside its clip and not transparent: the name, the
    /// version, Julien Bayle, Structure Void, the license and the repository. It is opened as both
    /// its entries open it, the About button and the macOS application menu: `request_about`.
    /// A box that opens empty, or without any one of them, fails.
    #[test]
    fn the_about_box_draws_its_six_pieces_of_information() {
        let mut h = harness();
        let size = egui::vec2(1100.0, 700.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        request_about(&h.ctx);
        // The request is taken on the next frame; a modal is sized on its first frame and
        // drawn on the one after.
        let mut out = h.frame(Vec::new());
        for _ in 0..3 {
            out = h.frame(Vec::new());
        }
        assert!(h.app.view.about_open, "the request opened the box");
        let mut drawn: Vec<(String, egui::Rect, egui::Rect, f32)> = Vec::new();
        fn walk(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect, f32)>,
        ) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, clip, out)),
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    clip,
                    t.opacity_factor,
                )),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, clipped.clip_rect, &mut drawn);
        }
        let license = env!("CARGO_PKG_LICENSE");
        assert_eq!(
            license, "GPL-3.0-or-later",
            "the manifest declares the license"
        );
        let version = format!("version {}", env!("CARGO_PKG_VERSION"));
        let license_line = format!("License {license}");
        let six: [(&str, &str); 6] = [
            ("the name", "Void OSCQ"),
            ("the version", &version),
            ("the author", "Julien Bayle"),
            ("the studio", "Structure Void"),
            ("the license", &license_line),
            ("the repository", "github.com/gluon/void-oscq"),
        ];
        for (what, text) in six {
            let found: Vec<_> = drawn.iter().filter(|(t, ..)| t.contains(text)).collect();
            assert!(
                !found.is_empty(),
                "{what}, {text:?}, is not drawn: {:?}",
                drawn.iter().map(|d| &d.0).collect::<Vec<_>>()
            );
            for (t, rect, clip, opacity) in found {
                assert!(
                    clip.intersect(screen).expand(0.5).contains_rect(*rect),
                    "{what}, {t:?}, at {rect:?}, cut by {clip:?}"
                );
                assert!(*opacity > 0.0, "{what}, {t:?}, drawn transparent");
            }
        }
    }

    /// With no server found, the server pane says since when it has been searching, as a time
    /// of day, so the text holds still: a running count would repaint the window at rest.
    #[test]
    fn with_no_server_found_the_pane_says_since_when_it_searches() {
        let mut h = harness();
        let texts = drawn_texts(&h.frame(Vec::new()));
        let line = texts
            .iter()
            .find(|t| t.starts_with("Searching since "))
            .unwrap_or_else(|| panic!("no search line: {texts:?}"));
        assert!(line.contains("none found"), "{line}");
        let time = line["Searching since ".len()..].split(',').next().unwrap();
        let hm = time.trim_end_matches(" UTC");
        assert!(
            hm.len() == 5 && hm.as_bytes()[2] == b':' && hm.replace(':', "").parse::<u32>().is_ok(),
            "a time of day, {time:?}"
        );
        // Start up, then rest: from then on, one frame and nothing asked for.
        let (_, t, _) = h.frames_until_idle(20.0, &[]);
        let (frames, _, causes) = h.frames_until_idle(t + 60.0, &[]);
        assert_eq!(
            (frames, causes.len()),
            (1, 0),
            "the line asks for no repaint: {causes:?}"
        );
        assert!(
            causes.is_empty(),
            "the line asks for no repaint: {causes:?}"
        );
    }

    /// On a narrow tree the name gives way first, then the metadata down to the flags, then the
    /// name again; the widget keeps its width throughout, and the columns never take more than
    /// the width given while there is room for the widget and the flags.
    #[test]
    fn narrow_tree_columns_give_up_the_name_then_the_metadata_never_the_widget() {
        let mut previous: Option<Columns> = None;
        for w in (184..=700).rev().step_by(4).map(|w| w as f32) {
            let c = Columns::tree(w);
            assert!(
                c.name + c.control + c.meta <= w + 0.5,
                "{w}: columns wider than the pane"
            );
            assert!(c.control >= style::CONTROL_W, "{w}: the widget narrowed");
            if c.meta < style::META_W {
                assert_eq!(c.meta, style::FLAGS_W, "{w}");
                assert!(
                    w < style::NAME_FLOOR + style::CONTROL_W + style::META_W,
                    "{w}: the metadata gave way while the name could"
                );
            }
            if let Some(p) = previous
                && p.meta == c.meta
            {
                assert!(
                    c.name <= p.name && c.control <= p.control,
                    "{w}: grew while narrowing"
                );
            }
            previous = Some(c);
        }
        assert_eq!(Columns::tree(200.0).meta, style::FLAGS_W);
        assert_eq!(Columns::tree(440.0).meta, style::META_W);
    }

    /// Where the panes' backgrounds were painted, left to right, along the line at `y`: the
    /// spans of every rectangle filled with the panel colour that crosses it, merged.
    fn painted_spans(out: &egui::FullOutput, fill: Color32, y: f32) -> Vec<(f32, f32)> {
        fn walk(shape: &egui::Shape, fill: Color32, y: f32, spans: &mut Vec<(f32, f32)>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, fill, y, spans)),
                egui::Shape::Rect(r) if r.fill == fill && r.rect.y_range().contains(y) => {
                    spans.push((r.rect.min.x, r.rect.max.x))
                }
                _ => {}
            }
        }
        let mut spans = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, fill, y, &mut spans);
        }
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f32, f32)> = Vec::new();
        for (lo, hi) in spans {
            match merged.last_mut() {
                // A separator line, one pixel wide, is drawn between two panes, not filled.
                Some(last) if lo <= last.1 + 1.5 => last.1 = last.1.max(hi),
                _ => merged.push((lo, hi)),
            }
        }
        merged
    }

    /// The handles of the side panes stop dead at their limits, and each pane follows its handle
    /// whole: dragged past what the centre can give, past the pane's least width, and out of the
    /// window on either side, with nothing pinned to fill the right pane. On every frame of the
    /// drag the window is painted from edge to edge, the handle never passes its limit, and on
    /// release the pane keeps the width it was dragged to.
    #[test]
    fn the_pane_handles_stop_dead_at_their_limits() {
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]}}}"#;
        for width in [640.0, 900.0, 1100.0, 1600.0] {
            let mut h = harness();
            controllable(&mut h, "127.0.0.1", 2345, "S", doc);
            let size = egui::vec2(width, 600.0);
            h.frame_sized(size, Vec::new());
            h.frame_sized(size, Vec::new());
            let ctx = h.ctx.clone();
            let fill = ctx.global_style().visuals.panel_fill;
            let y = 300.0;
            let pointer = |x: f32| egui::pos2(x, y);
            for pane in ["work", "servers"] {
                // The handle's place, after checking the window is painted whole.
                let edge = |out: &egui::FullOutput, when: &str| {
                    let spans = painted_spans(out, fill, y);
                    assert!(
                        spans.len() == 1 && spans[0].0 <= 0.5 && spans[0].1 >= width - 0.5,
                        "unpainted gaps {when}, {pane} handle, window {width}: painted {spans:?}"
                    );
                    ctx.read_response(egui::Id::new(pane).with("__resize"))
                        .unwrap()
                        .rect
                        .center()
                        .x
                };
                for targets in [
                    vec![300.0, 100.0, 0.0, -200.0],
                    vec![width - 100.0, width - 20.0, width + 200.0],
                ] {
                    let out = h.frame_sized(size, Vec::new());
                    let start = edge(&out, "before the drag");
                    h.frame_sized(size, vec![egui::Event::PointerMoved(pointer(start))]);
                    h.frame_sized(
                        size,
                        vec![egui::Event::PointerButton {
                            pos: pointer(start),
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::NONE,
                        }],
                    );
                    let servers = egui::PanelState::load(&h.ctx, egui::Id::new("servers"))
                        .unwrap()
                        .outer_rect
                        .max
                        .x;
                    // The leftmost the handle may go, and the rightmost.
                    let (lo, hi) = if pane == "work" {
                        (
                            (servers + style::CENTRE_MIN).min(width - style::WORK_MIN),
                            width - style::WORK_MIN,
                        )
                    } else {
                        (
                            style::SERVERS_MIN,
                            (width - style::CENTRE_MIN - style::WORK_DEFAULT)
                                .max(style::SERVERS_MIN),
                        )
                    };
                    let mut last = start;
                    for x in &targets {
                        let out = h.frame_sized(size, vec![egui::Event::PointerMoved(pointer(*x))]);
                        last = edge(&out, &format!("dragging to {x}"));
                        assert!(
                            (lo - 0.5..=hi + 0.5).contains(&last),
                            "{pane} handle at {last}, dragging to {x}, its range {lo}..{hi}, window {width}"
                        );
                    }
                    if targets[targets.len() - 1] < 0.0 {
                        assert!(
                            (last - lo).abs() < 1.0,
                            "{pane} dragged left stops at {last}, its limit {lo}, window {width}"
                        );
                    } else {
                        assert!(
                            (last - hi).abs() < 1.0,
                            "{pane} dragged right stops at {last}, its limit {hi}, window {width}"
                        );
                    }
                    let end = *targets.last().unwrap();
                    h.frame_sized(
                        size,
                        vec![egui::Event::PointerButton {
                            pos: pointer(end),
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: Modifiers::NONE,
                        }],
                    );
                    h.frame_sized(size, vec![egui::Event::PointerGone]);
                    let out = h.frame_sized(size, Vec::new());
                    let released = edge(&out, "after release");
                    assert!(
                        (released - last).abs() < 0.5,
                        "{pane} handle went from {last} to {released} on release, window {width}"
                    );
                }
            }
        }
    }

    /// A node's access is what its server declares, whatever the connection: when a reconnection
    /// fails, a read write node stays read write, explicit or inferred from `HOST_INFO`, and the
    /// row says nothing can be sent, with the reason, never "read only". The failure is said once.
    /// Real service, synthetic server stopped between the two connections.
    #[test]
    fn a_failed_connection_keeps_the_declared_access_and_says_nothing_can_be_sent() {
        use crate::synthetic::{Reply, SyntheticServer};
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let doc = r#"{"FULL_PATH":"/","CONTENTS":{
            "declared":{"FULL_PATH":"/declared","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
            "inferred":{"FULL_PATH":"/inferred","TYPE":"f","VALUE":[0.25]}}}"#;
        let routes = vec![
            ("/", Reply::json(doc)),
            (
                "/?HOST_INFO",
                Reply::json(r#"{"NAME":"rw","OSC_PORT":9,"EXTENSIONS":{"VALUE":true}}"#),
            ),
        ];
        let server = rt
            .block_on(SyntheticServer::start("127.0.0.1:0", routes))
            .unwrap();
        let key = format!("127.0.0.1:{}", server.addr.port());
        let (ev_tx, ev_rx) = flume::unbounded();
        let commands = oscq_core::service::spawn(rt.handle(), ev_tx, Arc::new(|| {}));
        let ctx = egui::Context::default();
        let app = VoidApp::with_backend(
            &ctx,
            Backend {
                commands,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            Config {
                manual_entries: vec![key.clone()],
                last_selected: Some(key),
                ..Config::default()
            },
            None,
        );
        let mut h = Harness {
            ctx,
            app,
            commands: flume::unbounded().1,
            events: flume::unbounded().0,
            _rt: rt,
        };
        wait_for(&mut h, "connected", |a| {
            a.state
                .selected_entry()
                .is_some_and(|e| e.conn == ConnState::Connected && e.tree.is_some())
        });
        let rows = |a: &VoidApp| {
            let e = a.state.selected_entry().unwrap();
            let t = e.tree.as_ref().unwrap();
            ["/declared", "/inferred"].map(|p| {
                let node = t.node(p).unwrap();
                let access = node.effective_access(e.value_supported());
                let shown = widget::present(node, access, e.send_blocked().as_deref());
                (access, shown.widget, shown.blocked)
            })
        };
        for (access, widget, blocked) in rows(&h.app) {
            assert_eq!(access, Access::READ_WRITE);
            assert!(widget && blocked.is_none(), "{blocked:?}");
        }

        drop(server);
        let id = h.app.state.selected.unwrap();
        h.app.act(&h.ctx.clone(), Action::Connect(id));
        wait_for(&mut h, "the reconnection to fail", |a| {
            a.state
                .selected_entry()
                .is_some_and(|e| matches!(e.conn, ConnState::Failed(_)))
        });
        for (access, widget, blocked) in rows(&h.app) {
            assert_eq!(
                access,
                Access::READ_WRITE,
                "the server's declaration stands"
            );
            assert!(!widget);
            let reason = blocked.unwrap();
            assert!(
                reason.starts_with("not connected: could not connect to "),
                "{reason}"
            );
        }
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert!(!texts.iter().any(|t| t == "read only"), "{texts:?}");
        assert_eq!(
            texts.iter().filter(|t| *t == CANNOT_SEND).count(),
            2,
            "{texts:?}"
        );
        assert_eq!(texts.iter().filter(|t| *t == "rw").count(), 2, "{texts:?}");
        assert_eq!(
            texts
                .iter()
                .filter(|t| t.contains("could not connect"))
                .count(),
            1,
            "said once: {texts:?}"
        );
    }

    /// The window on the real network service, with `config`, on its own runtime.
    fn service_harness(config: Config) -> Harness {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (ev_tx, ev_rx) = flume::unbounded();
        let commands = oscq_core::service::spawn(rt.handle(), ev_tx, Arc::new(|| {}));
        let ctx = egui::Context::default();
        let app = VoidApp::with_backend(
            &ctx,
            Backend {
                commands,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            config,
            None,
        );
        Harness {
            ctx,
            app,
            commands: flume::unbounded().1,
            events: flume::unbounded().0,
            _rt: rt,
        }
    }

    /// Max 9 serves OSCQuery on one port and receives OSC on another, and ignores `HOST_INFO`: the
    /// inferred port is wrong. The user sets the port in the status bar; the status bar tells the
    /// three origins apart, sends reach the user's port, the port is kept in the config, and after
    /// a restart the connection takes it again instead of inferring.
    #[test]
    fn a_user_osc_port_replaces_the_inferred_one_and_is_kept() {
        use crate::synthetic::{OscReceiver, Reply, SyntheticServer};
        assert_eq!(
            [
                PortSource::Advertised,
                PortSource::InferredFromHttp,
                PortSource::User
            ]
            .map(port_source_text),
            [
                "port advertised",
                "port inferred from the HTTP port",
                "port set by the user"
            ]
        );
        let root = include_str!("../../../tests/fixtures/max-root.json");
        let host = include_str!("../../../tests/fixtures/max-hostinfo.json");
        // The server and the OSC receiver outlive both windows below.
        let srv = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let osc = srv.block_on(OscReceiver::bind("127.0.0.1:0")).unwrap();
        let server = srv
            .block_on(SyntheticServer::start(
                "127.0.0.1:0",
                vec![("/", Reply::json(root)), ("/?HOST_INFO", Reply::json(host))],
            ))
            .unwrap();
        let key = format!("127.0.0.1:{}", server.addr.port());
        let mut h = service_harness(Config {
            manual_entries: vec![key.clone()],
            last_selected: Some(key.clone()),
            ..Config::default()
        });
        let port_of =
            |a: &VoidApp| match a.state.selected_entry().and_then(|e| e.destination.clone()) {
                Some(Ok(d)) => Some((d.port, d.port_source)),
                _ => None,
            };
        wait_for(&mut h, "connected", |a| {
            a.state
                .selected_entry()
                .is_some_and(|e| e.conn == ConnState::Connected)
        });
        assert_eq!(
            port_of(&h.app),
            Some((server.addr.port(), PortSource::InferredFromHttp))
        );
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert!(
            texts
                .iter()
                .any(|t| t.ends_with("port inferred from the HTTP port")),
            "{texts:?}"
        );

        let id = h.app.state.selected.unwrap();
        h.app
            .act(&h.ctx.clone(), Action::SetOscPort(id, Some(osc.port())));
        assert_eq!(h.app.config.osc_ports.get(&key), Some(&osc.port()));
        wait_for(&mut h, "the user's port", |a| {
            port_of(a) == Some((osc.port(), PortSource::User))
        });
        let texts = drawn_texts(&h.frame(Vec::new()));
        assert!(
            texts.iter().any(|t| t.ends_with("port set by the user")),
            "{texts:?}"
        );
        let send = |h: &mut Harness| {
            let req = SendReq {
                server_id: h.app.state.selected.unwrap(),
                path: OscPath::parse("/test/param/fader/normalized").unwrap(),
                signature: TypeSignature::parse("f").unwrap(),
                args: vec![OscType::Double(0.5)],
                readable: false,
            };
            h.app
                .act(&h.ctx.clone(), Action::Send(req, SendMode::Final));
            srv.block_on(osc.recv())
        };
        assert!(matches!(
            send(&mut h),
            rosc::OscPacket::Message(m) if m.addr == "/test/param/fader/normalized"
        ));

        // The config as saved, read back: a restart connects with the user's port.
        let dir = std::env::temp_dir().join(format!("void-oscq-port-{}", std::process::id()));
        let path = config::config_path(&dir);
        config::write_atomic(&path, &h.app.config.to_bytes()).unwrap();
        let (saved, warning) = Config::load(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(warning, None);
        assert_eq!(saved.osc_ports.get(&key), Some(&osc.port()));
        let mut restarted = service_harness(saved);
        wait_for(&mut restarted, "reconnected with the user's port", |a| {
            a.state
                .selected_entry()
                .is_some_and(|e| e.conn == ConnState::Connected)
        });
        assert_eq!(
            port_of(&restarted.app),
            Some((osc.port(), PortSource::User)),
            "never replaced by the inference"
        );
        assert!(matches!(send(&mut restarted), rosc::OscPacket::Message(_)));
    }

    /// M4: a value changed inside the server reaches the window with no user action. Real
    /// service and WebSocket, synthetic server: the visible method is listened to, the server
    /// pushes a new value, and the row shows it as read.
    #[test]
    fn a_value_changed_on_the_server_appears_without_user_action() {
        let (mut h, server, _osc) = live_harness(
            |key| Config {
                manual_entries: vec![key.to_owned()],
                last_selected: Some(key.to_owned()),
                ..Config::default()
            },
            Vec::new(),
        );
        h.app.act(&h.ctx.clone(), Action::Toggle("/g0".into()));
        wait_for(&mut h, "/g0 visible and listened to", |a| {
            a.view.visible_methods.contains(&"/g0/p0".to_owned())
                && a.state
                    .selected_entry()
                    .is_some_and(|e| e.listening.contains("/g0/p0"))
        });
        h._rt.block_on(
            server
                .ws
                .wait("LISTEN /g0/p0", |w| w.listens().contains("/g0/p0")),
        );
        for _ in 0..20 {
            h.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
        let e = h.app.state.selected_entry().unwrap();
        assert_eq!(e.ws, WsState::Open, "listening, nothing received: not live");
        assert!(!values_text(e).unwrap().0.contains("live"));
        // Two methods, two values: each row shows its own, nothing copied from one to another.
        server.ws.push(crate::synthetic::Push::Osc(
            "/g0/p0".into(),
            vec![OscType::Float(0.25)],
        ));
        server.ws.push(crate::synthetic::Push::Osc(
            "/g0/p1".into(),
            vec![OscType::Float(0.75)],
        ));
        let shown = |a: &VoidApp, path: &str| {
            a.state
                .selected_entry()
                .and_then(|e| e.tree.as_ref())
                .and_then(|t| t.node(path).map(|n| shown_value(t, n, true)))
        };
        wait_for(&mut h, "the pushed values", |a| {
            shown(a, "/g0/p0") == Some(Shown::Read("0.250".into()))
                && shown(a, "/g0/p1") == Some(Shown::Read("0.750".into()))
        });
        assert_eq!(
            shown(&h.app, "/g0/p2"),
            Some(Shown::Read("0.500".into())),
            "a method nothing was pushed to keeps its document value"
        );
        // The server's `?VALUE` answers what it pushed, without an error.
        h.app
            .act(&h.ctx.clone(), Action::RefreshValue("/g0/p1".into()));
        for _ in 0..30 {
            h.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(shown(&h.app, "/g0/p1"), Some(Shown::Read("0.750".into())));
        assert_eq!(h.app.state.selected_entry().unwrap().last_error, None);
        let e = h.app.state.selected_entry().unwrap();
        assert_eq!(e.ws, WsState::Live, "live once the server has sent a value");
        assert_eq!(values_text(e).unwrap().0, "values live over the WebSocket");
        assert!(
            e.traffic
                .view("", traffic::DirFilter::All)
                .iter()
                .any(|r| r.direction == oscq_core::Direction::In && r.path == "/g0/p0"),
            "logged as received"
        );
    }

    /// PRD 7.4: values are fetched after a send only when nothing pushes them. A listened method on
    /// a WebSocket that notifies gets no refresh after a committed send; on a silent socket, or no
    /// socket, it does.
    #[test]
    fn no_refresh_after_a_send_when_the_value_is_pushed() {
        let mut h = harness();
        let id = connected(&mut h, &flat_tree(3));
        h.frame(Vec::new());
        h.frame(Vec::new());
        h.sent();
        let req = || SendReq {
            server_id: id,
            path: OscPath::parse("/p001").unwrap(),
            signature: TypeSignature::parse("f").unwrap(),
            args: vec![OscType::Double(0.3)],
            readable: true,
        };
        let refresh_of = |cmds: Vec<Command>| -> Vec<bool> {
            cmds.into_iter()
                .filter_map(|c| match c {
                    Command::SendOsc { refresh_after, .. } => Some(refresh_after),
                    _ => None,
                })
                .collect()
        };
        let ctx = h.ctx.clone();
        let mut seen = Vec::new();
        for state in [
            None,
            Some(oscq_core::TransportState::WebSocketOpen),
            Some(oscq_core::TransportState::WebSocketLive),
        ] {
            if let Some(state) = state {
                h.events
                    .send(Event::TransportState {
                        server_id: id,
                        state,
                    })
                    .unwrap();
                h.frame(Vec::new());
            }
            assert!(h.app.state.entry(id).unwrap().listening.contains("/p001"));
            h.app.act(&ctx, Action::Send(req(), SendMode::Final));
            seen.extend(refresh_of(h.sent()));
        }
        assert_eq!(
            seen,
            [true, true, false],
            "no socket, silent socket, notifying socket"
        );
    }

    /// PRD 10, measured on the synthetic server's 5000 node document. Run on a release build:
    /// `cargo test --release -p void-oscq -- --ignored budget_ --nocapture --test-threads=1`.
    mod budgets {
        use super::*;
        use crate::synthetic::large_document;

        fn release_only() {
            if cfg!(debug_assertions) {
                panic!("budgets are measured on a release build: cargo test --release");
            }
        }

        /// Median, 95th percentile and maximum, printed with the budget.
        fn report(name: &str, budget: Duration, mut v: Vec<Duration>) -> Duration {
            v.sort();
            let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            let p95 = at(0.95);
            println!(
                "BUDGET {name}: n {} median {:.3} ms, p95 {:.3} ms, max {:.3} ms, budget {:.0} ms",
                v.len(),
                ms(at(0.5)),
                ms(p95),
                ms(at(1.0)),
                ms(budget)
            );
            p95
        }

        fn key_event(key: Key) -> egui::Event {
            egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }
        }

        /// Budget 2: parse the document, model it as the window does (state, path index, the
        /// rows of the collapsed tree), under 150 ms.
        #[test]
        #[ignore = "budget, release build"]
        fn budget_parse_and_model_5000_nodes() {
            release_only();
            let doc = large_document(5000);
            let mut times = Vec::new();
            for _ in 0..30 {
                let t0 = Instant::now();
                let report = parse_document(doc.as_bytes()).unwrap();
                let mut st = AppState::default();
                let s = server();
                let id = s.id;
                st.apply(Event::ServerFound(s));
                st.apply(Event::TreeLoaded {
                    server_id: id,
                    raw: Arc::from(doc.as_str()),
                    report,
                });
                let t = st.entry(id).unwrap().tree.as_ref().unwrap();
                let rows = tree::visible_rows(&t.root, &[], &t.expanded);
                times.push(t0.elapsed());
                assert_eq!((t.paths.len(), rows.len()), (5051, 50));
            }
            let budget = Duration::from_millis(150);
            assert!(report("parse and model 5000 nodes", budget, times) < budget);
        }

        /// Budget 3: frames while scrolling the fully expanded tree, 5050 rows, under 8 ms at
        /// p95. Measured on the CPU: the whole frame plus tessellation; GPU time is not included.
        #[test]
        #[ignore = "budget, release build"]
        fn budget_frame_time_scrolling_5000_nodes() {
            release_only();
            let mut h = harness();
            let id = connected(&mut h, &large_document(5000));
            h.frame(Vec::new());
            let t = h.app.state.entry_mut(id).unwrap().tree.as_mut().unwrap();
            t.expanded.extend((0..50).map(|g| format!("/g{g}")));
            h.app.view.expanded_gen += 1;
            h.ctx
                .all_styles_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
            h.frame(Vec::new());
            let first = h.app.view.visible_methods.first().cloned();
            let mut times = Vec::new();
            for _ in 0..400 {
                let events = vec![
                    egui::Event::PointerMoved(egui::pos2(700.0, 350.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -45.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: Modifiers::NONE,
                    },
                ];
                let t0 = Instant::now();
                let out = h.frame(events);
                let _ = h.ctx.tessellate(out.shapes, out.pixels_per_point);
                times.push(t0.elapsed());
                h.sent();
            }
            assert_ne!(
                h.app.view.visible_methods.first().cloned(),
                first,
                "the tree scrolled"
            );
            let budget = Duration::from_millis(8);
            assert!(report("frame while scrolling 5050 rows", budget, times) < budget);
        }

        /// Budget 5: connected, WebSocket live, 50 pinned listens: an idle frame asks for no
        /// repaint and sends nothing.
        #[test]
        #[ignore = "budget, release build"]
        fn budget_idle_repaints_with_50_listens() {
            release_only();
            let mut config = Config::default();
            config.pinned_paths.insert(
                "127.0.0.1:2345".into(),
                (0..50).map(|i| format!("/g0/p{i}")).collect(),
            );
            let mut h = harness_with(config);
            let id = connected(&mut h, &large_document(5000));
            h.events
                .send(Event::TransportState {
                    server_id: id,
                    state: oscq_core::TransportState::WebSocketLive,
                })
                .unwrap();
            // A tree appearing starts egui's scroll bar fade in, a few frames long: let it end.
            let mut settling = 0;
            while idle_delay(&h.frame(Vec::new())) != Duration::MAX {
                settling += 1;
                assert!(settling < 60, "still animating after 60 frames");
            }
            println!("BUDGET settle: {settling} animation frames after the tree appeared");
            assert_eq!(h.app.state.entry(id).unwrap().listening.len(), 50);
            h.sent();
            let mut repaints = 0;
            for _ in 0..100 {
                let out = h.frame(Vec::new());
                repaints += usize::from(idle_delay(&out) != Duration::MAX);
            }
            println!("BUDGET idle repaints: {repaints} of 100 idle frames asked for one, budget 0");
            assert_eq!(repaints, 0);
            assert!(h.sent().is_empty());
        }

        /// Budget 6: a slider moved with the keyboard, from the frame that reads the key to the
        /// UDP packet arriving on loopback, through the real network service, under 2 ms at p95.
        /// Loopback delivery is included, so this is an upper bound of the packet leaving.
        #[test]
        #[ignore = "budget, release build"]
        fn budget_slider_to_udp_packet() {
            release_only();
            let (mut h, _server, target) = live_harness(
                |key| Config {
                    manual_entries: vec![key.to_owned()],
                    last_selected: Some(key.to_owned()),
                    ..Config::default()
                },
                Vec::new(),
            );
            h.app.act(&h.ctx.clone(), Action::Toggle("/g0".into()));
            wait_for(&mut h, "/g0 loaded", |a| {
                a.state
                    .selected_entry()
                    .and_then(|e| e.tree.as_ref())
                    .is_some_and(|t| t.fetched.contains("/g0"))
            });
            h.app.view.selected_path = Some("/g0/p0".into());
            h.frame(Vec::new());
            h.key(Key::Enter);
            let mut times = Vec::new();
            for i in 0..300 {
                let k = if i % 2 == 0 {
                    Key::ArrowRight
                } else {
                    Key::ArrowLeft
                };
                let t0 = Instant::now();
                h.frame(vec![key_event(k)]);
                let packet = h._rt.block_on(target.recv());
                times.push(t0.elapsed());
                let rosc::OscPacket::Message(m) = packet else {
                    panic!("not a message")
                };
                assert_eq!(m.addr, "/g0/p0");
                h.frame(Vec::new());
            }
            let budget = Duration::from_millis(2);
            assert!(report("slider to UDP packet", budget, times) < budget);
        }
    }

    #[test]
    fn json_selection_is_the_exact_received_subtree() {
        let raw =
            r#"{"FULL_PATH":"/","CONTENTS":{"a":{"FULL_PATH":"/a","VALUE":[1.0],"TYPE":"f"}}}"#;
        let t = crate::state::TreeState {
            root: parse_document(raw.as_bytes()).unwrap().root,
            raw: Arc::from(raw),
            ..Default::default()
        };
        assert_eq!(selected_raw(&t, None).0, raw);
        assert_eq!(
            selected_raw(&t, Some("/a")).0,
            r#"{"FULL_PATH":"/a","VALUE":[1.0],"TYPE":"f"}"#
        );
    }

    #[test]
    fn manual_entries_from_config_are_listed() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (cmd_tx, _cmd_rx) = flume::unbounded();
        let (_ev_tx, ev_rx) = flume::unbounded();
        let config = Config {
            manual_entries: vec!["10.0.0.2:2345".into(), "bad".into()],
            ..Config::default()
        };
        let app = VoidApp::with_backend(
            &egui::Context::default(),
            Backend {
                commands: cmd_tx,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            config,
            None,
        );
        assert_eq!(app.state.servers.len(), 1);
        assert_eq!(app.state.servers[0].server.origin, Origin::Manual);
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.state.selected, None);
    }

    #[test]
    fn last_selected_manual_entry_is_restored_and_connected() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (cmd_tx, cmd_rx) = flume::unbounded();
        let (_ev_tx, ev_rx) = flume::unbounded();
        let config = Config {
            manual_entries: vec!["10.0.0.2:2345".into()],
            last_selected: Some("10.0.0.2:2345".into()),
            ..Config::default()
        };
        let app = VoidApp::with_backend(
            &egui::Context::default(),
            Backend {
                commands: cmd_tx,
                events: ev_rx,
                io: rt.handle().clone(),
            },
            config,
            None,
        );
        assert!(app.state.selected.is_some());
        let sent: Vec<Command> = cmd_rx.try_iter().collect();
        assert!(matches!(
            sent.as_slice(),
            [Command::Connect { http_base, .. }] if http_base.as_str() == "http://10.0.0.2:2345/"
        ));
    }
}
