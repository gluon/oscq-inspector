//! The window. Runs on the UI thread: never blocks, never does I/O. Network work goes out as
//! `Command`s, results come back as `Event`s drained at the top of every frame; file writes are
//! handed to the runtime.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::text::LayoutJob;
use egui::{Color32, FontId, Key, Modifiers, RichText, TextFormat};
use oscq_core::model::parse_rgba_hex;
use oscq_core::osc::arg_from_text;
use oscq_core::{
    ArgType, Command, Event, OscPath, PortSource, ServerId, ServiceKind, TypeSignature, TypeTag,
};
use oscq_discovery::{ManualEntry, ManualHistory};
use rosc::{OscColor, OscType};

use crate::config::{self, Config, Theme, WindowGeometry};
use crate::json_view::{self, Line, Token};
use crate::rate::{INTERVAL, RateLimiter};
use crate::state::{AppState, ConnState, ServerEntry, WsState};
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
    key: Option<(ServerId, u64, u64, String)>,
    rows: Vec<Row>,
}

#[derive(Default)]
struct FilterCache {
    key: Option<(ServerId, u64, String)>,
    hits: Vec<Hit>,
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
    json: JsonCache,
    text_buffers: std::collections::HashMap<String, String>,
    /// Paths with a drag send not yet followed by its final send.
    drag_open: std::collections::HashSet<String>,
    /// Methods on the rows drawn this frame, the visible part of the listen set (PRD 7.4).
    visible_methods: Vec<String>,
    traffic_filter: String,
    manual_open: bool,
    manual_text: String,
    manual_error: Option<String>,
    about_open: bool,
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
        tracing::trace!(target: "void_oscq::frame", causes = ?ctx.repaint_causes(), "frame");
        if let Some(t) = self.launched.take() {
            tracing::info!(
                target: "void_oscq::startup",
                "first frame {:.1} ms after launch",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        self.drain(&ctx);
        self.flush_limiter(&ctx);
        self.track_geometry(&ctx);
        while let Ok(note) = self.io_rx.try_recv() {
            self.view.note = Some(note);
        }

        let mut actions = Vec::new();
        self.view.visible_methods.clear();
        egui::Panel::left("servers")
            .resizable(true)
            .default_size(230.0)
            .show(ui, |ui| self.server_pane(ui, &mut actions));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
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
                        let _ = self.backend.commands.send(Command::Connect {
                            server_id: id,
                            http_base: url,
                        });
                    }
                    None => e.conn = ConnState::Failed("no usable address".to_owned()),
                }
            }
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
                        let refresh = r.readable;
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
        ui.heading("Servers");
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(ui.available_height() - 70.0)
            .show(ui, |ui| {
                if self.state.servers.is_empty() {
                    let elapsed = self.started.elapsed();
                    if elapsed < DISCOVERY_HINT_AFTER {
                        ui.weak("Searching the network");
                        ui.ctx()
                            .request_repaint_after(DISCOVERY_HINT_AFTER - elapsed);
                    } else {
                        ui.weak(
                            "No OSCQuery server found. Use + manual for a host on a VPN, \
                             a routed subnet or behind a firewall.",
                        );
                    }
                }
                for e in &self.state.servers {
                    let selected = self.state.selected == Some(e.server.id);
                    ui.horizontal(|ui| {
                        status_dot(ui, dot_colour(e));
                        let mut text = RichText::new(&e.server.name);
                        if e.server.kind == ServiceKind::OscOnly || e.server.stale {
                            text = text.weak();
                        }
                        let label = if e.server.kind == ServiceKind::OscOnly {
                            ui.add_enabled(false, egui::Button::selectable(false, text))
                                .on_disabled_hover_text("OSC only, no query support")
                        } else {
                            ui.add(egui::Button::selectable(selected, text))
                        };
                        let label = label.on_hover_text(server_tooltip(e));
                        if label.clicked() {
                            actions.push(Action::SelectServer(e.server.id));
                        }
                    });
                    if e.server.kind == ServiceKind::OscOnly {
                        ui.weak("    OSC only, no query support");
                    }
                }
            });
        ui.separator();
        if ui.button("+ manual").clicked() {
            self.view.manual_open = true;
            self.view.manual_error = None;
        }
        ui.horizontal(|ui| {
            let mut theme = self.config.theme;
            egui::ComboBox::from_id_salt("theme")
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
                self.view.about_open = true;
            }
        });
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
            ui.heading("Void OSCQ");
            ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
            ui.label("OSCQuery client: browse an address space, edit values, send OSC.");
            if ui.button("Close").clicked() {
                self.view.about_open = false;
            }
        });
        if response.should_close() {
            self.view.about_open = false;
        }
    }

    // ---------------------------------------------------------------- status bar

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if let Some(e) = self.state.selected_entry() {
                ui.strong(&e.server.name);
                ui.label(conn_text(e));
                match &e.destination {
                    Some(Ok(d)) => {
                        let source = match d.port_source {
                            PortSource::Advertised => "port advertised",
                            PortSource::InferredFromHttp => "port inferred from the HTTP port",
                        };
                        let transport = match d.transport {
                            oscq_core::OscTransport::Udp => "UDP",
                            oscq_core::OscTransport::Tcp => "TCP",
                        };
                        ui.label(format!("OSC {}:{} {transport}, {source}", d.host, d.port));
                    }
                    Some(Err(blocked)) => {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("read only: {blocked}"),
                        );
                    }
                    None => {}
                }
                if let Some(reason) = e.send_blocked().filter(|_| e.conn == ConnState::Connected) {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("not controllable: {reason}"),
                    );
                }
                match values_text(e) {
                    Some((text, true)) => {
                        ui.colored_label(ui.visuals().warn_fg_color, text);
                    }
                    Some((text, false)) => {
                        ui.label(text);
                    }
                    None => {}
                }
                if e.host_info.as_ref().is_some_and(|h| h.query_ignored) {
                    ui.weak("HOST_INFO ignored by the server, defaults used")
                        .on_hover_text(
                            "The server answered the HOST_INFO query with its address space: \
                             OSC and WebSocket are taken on the HTTP address and port.",
                        );
                }
                if let Some(t) = &e.tree {
                    ui.label(format!("{} nodes", t.root.count().saturating_sub(1)));
                    if !t.diagnostics.is_empty() {
                        ui.weak(format!("{} document warnings", t.diagnostics.len()));
                    }
                }
                if let Some(err) = &e.last_error {
                    let text = match &err.path {
                        Some(p) => format!("{p}: {}", err.message),
                        None => err.message.clone(),
                    };
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
            } else {
                ui.weak("No server selected");
            }
            if let Some(g) = &self.state.global_error {
                ui.colored_label(ui.visuals().error_fg_color, g);
            }
            if let Some(n) = &self.view.note {
                ui.weak(n);
            }
        });
    }

    // ---------------------------------------------------------------- main pane

    fn main_pane(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(id) = self.state.selected else {
            ui.centered_and_justified(|ui| ui.weak("Select a server, or use + manual"));
            return;
        };
        ui.horizontal(|ui| {
            let filter = ui.add(
                egui::TextEdit::singleline(&mut self.view.filter)
                    .id(egui::Id::new("filter"))
                    .hint_text("Path filter  /")
                    .desired_width(220.0),
            );
            if std::mem::take(&mut self.view.focus_filter) {
                filter.request_focus();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.view.prefix)
                    .hint_text("Root at path")
                    .desired_width(160.0),
            );
            ui.separator();
            ui.selectable_value(&mut self.view.tab, Tab::Tree, "Tree");
            ui.selectable_value(&mut self.view.tab, Tab::Json, "JSON");
            ui.selectable_value(&mut self.view.tab, Tab::Traffic, "Traffic");
            ui.separator();
            if ui
                .button("Refresh")
                .on_hover_text("Refresh the selected node, or the whole tree (F5)")
                .clicked()
            {
                actions.push(self.refresh_action());
            }
        });
        ui.separator();
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
        if entry.conn == ConnState::Connected && ui.small_button("Disconnect").clicked() {
            actions.push(Action::Disconnect(id));
        }
        match self.view.tab {
            Tab::Tree => tree_tab(ui, entry, &mut self.view, actions),
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

fn tree_tab(ui: &mut egui::Ui, entry: &ServerEntry, view: &mut View, actions: &mut Vec<Action>) {
    let Some(t) = entry.tree.as_ref() else {
        return;
    };
    let id = entry.server.id;
    let filtering = !view.filter.trim().is_empty();

    let rows: Vec<Row> = if filtering {
        let key = (id, t.version, view.filter.clone());
        if view.hits.key.as_ref() != Some(&key) {
            view.hits.hits = tree::fuzzy(&t.root, &view.filter);
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
        let key = (id, t.version, view.expanded_gen, prefix.clone());
        if view.rows.key.as_ref() != Some(&key) {
            let base = if prefix.is_empty() {
                Vec::new()
            } else {
                tree::find(&t.root, &prefix)
                    .filter(|i| tree::node_at(&t.root, i).is_some_and(|n| n.is_container()))
                    .unwrap_or_default()
            };
            view.rows.rows = tree::visible_rows(&t.root, &base, &t.expanded);
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

    let row_h = ui.spacing().interact_size.y + 2.0;
    let selected_pos = view.selected_path.as_ref().and_then(|p| {
        rows.iter()
            .position(|r| tree::node_at(&t.root, &r.index).is_some_and(|n| &n.full_path == p))
    });
    let mut area = egui::ScrollArea::both().auto_shrink(false);
    if std::mem::take(&mut view.scroll_to_selected)
        && let Some(pos) = selected_pos
    {
        let spacing = ui.spacing().item_spacing.y;
        let target = (pos as f32 * (row_h + spacing) - ui.available_height() / 2.0).max(0.0);
        area = area.vertical_scroll_offset(target);
    }
    let send_blocked = entry.send_blocked();
    let value_supported = entry.value_supported();
    area.show_rows(ui, row_h, rows.len(), |ui, range| {
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
            ui.horizontal(|ui| {
                ui.set_height(row_h);
                tree_row(
                    ui,
                    entry,
                    t,
                    node,
                    row,
                    hit.as_ref(),
                    send_blocked.as_deref(),
                    value_supported,
                    view,
                    actions,
                );
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn tree_row(
    ui: &mut egui::Ui,
    entry: &ServerEntry,
    t: &crate::state::TreeState,
    node: &oscq_core::Node,
    row: &Row,
    hit: Option<&Hit>,
    send_blocked: Option<&str>,
    value_supported: bool,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    ui.add_space(row.depth as f32 * 14.0);
    if node.is_container() {
        let open = t.expanded.contains(&node.full_path);
        if ui
            .small_button(if open { "-" } else { "+" })
            .on_hover_text(if open { "Collapse" } else { "Expand" })
            .clicked()
        {
            actions.push(Action::Toggle(node.full_path.clone()));
        }
    } else {
        ui.add_space(ui.spacing().interact_size.y);
    }
    let selected = view.selected_path.as_deref() == Some(node.full_path.as_str());
    let name: egui::WidgetText = match hit {
        Some(h) => highlighted(ui, &node.full_path, &h.positions).into(),
        None => leaf_name(node).into(),
    };
    let column = egui::vec2(
        (260.0 - row.depth as f32 * 14.0).max(140.0),
        ui.spacing().interact_size.y,
    );
    let label = ui
        .allocate_ui_with_layout(
            column,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(column);
                ui.add(egui::Button::selectable(selected, name).truncate())
            },
        )
        .inner
        .on_hover_text(&node.full_path);
    if label.clicked() {
        actions.push(Action::SelectPath(node.full_path.clone()));
    }

    let access = node.effective_access(value_supported);
    let p = widget::present(node, access, send_blocked);
    let pending = t.pending.get(&node.full_path);
    if p.widget {
        let focus = selected && std::mem::take(&mut view.focus_widget);
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
    }
    match shown_value(t, node, p.value) {
        Shown::Sent(text) => {
            ui.label(RichText::new(format!("{text} (sent)")).italics().weak())
                .on_hover_text(if p.value {
                    "Sent, not yet confirmed by the server"
                } else {
                    "Sent, not confirmed: this node cannot be read"
                });
        }
        Shown::Unavailable => {
            ui.weak("unavailable");
        }
        Shown::Read(text) => {
            ui.label(text);
        }
        Shown::Nothing => {}
    }
    if let Some(reason) = &p.blocked {
        ui.weak("read only").on_hover_text(reason);
    }
    if let Some(tags) = &node.type_raw {
        ui.weak(RichText::new(tags).monospace());
    }
    if node.is_method() {
        let flags = format!(
            "{}{}",
            if access.readable() { "r" } else { "" },
            if access.writable() { "w" } else { "" }
        );
        ui.weak(RichText::new(flags).monospace());
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

fn highlighted(ui: &egui::Ui, text: &str, positions: &[u32]) -> LayoutJob {
    let mut job = LayoutJob::default();
    let normal = TextFormat {
        color: ui.visuals().text_color(),
        ..TextFormat::default()
    };
    let strong = TextFormat {
        color: ui.visuals().strong_text_color(),
        underline: egui::Stroke::new(1.0, ui.visuals().strong_text_color()),
        ..TextFormat::default()
    };
    for (i, c) in text.chars().enumerate() {
        let f = if positions.binary_search(&(i as u32)).is_ok() {
            strong.clone()
        } else {
            normal.clone()
        };
        job.append(&c.to_string(), 0.0, f);
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
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut view.traffic_filter)
                .hint_text("filter path or type tags")
                .desired_width(200.0),
        );
        let mut paused = entry.traffic.paused();
        if ui.toggle_value(&mut paused, "Pause").changed() {
            entry.traffic.set_paused(paused);
        }
        if ui.button("Clear").clicked() {
            entry.traffic.clear();
        }
        if ui.button("Export").clicked() {
            let view_records = entry.traffic.view(&view.traffic_filter);
            actions.push(Action::Save {
                stem: "traffic",
                ext: "txt",
                contents: traffic::export(started, &view_records),
            });
        }
        ui.weak(format!(
            "{} of max {} kept",
            entry.traffic.len(),
            traffic::CAPACITY
        ));
    });
    ui.weak(
        "time s  dir  path  type tags  arguments. Incoming values arrive over the WebSocket only.",
    );
    if entry.traffic.is_empty() {
        ui.weak("No traffic yet. Every message sent, and every value received, is logged here.");
    }
    let records = entry.traffic.view(&view.traffic_filter);
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, row_h, records.len(), |ui, range| {
            for r in &records[range] {
                ui.label(RichText::new(traffic::line(started, r)).monospace());
            }
        });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use oscq_core::{AttributeReply, DiscoveredServer, Origin, parse_document};

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
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 700.0),
                )),
                ..egui::RawInput::default()
            };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| app.frame(ui));
            // No renderer here: consume the texture uploads as one would.
            out.textures_delta.clear();
            out
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
        assert!(h.sent().is_empty(), "no refresh, no send again");

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
        server.ws.push(crate::synthetic::Push::Osc(
            "/g0/p0".into(),
            vec![OscType::Float(0.25)],
        ));
        wait_for(&mut h, "the pushed value", |a| {
            a.state
                .selected_entry()
                .and_then(|e| e.tree.as_ref())
                .and_then(|t| t.node("/g0/p0").map(|n| shown_value(t, n, true)))
                == Some(Shown::Read("0.250".into()))
        });
        let e = h.app.state.selected_entry().unwrap();
        assert_eq!(e.ws, WsState::Live, "live once the server has sent a value");
        assert_eq!(values_text(e).unwrap().0, "values live over the WebSocket");
        assert!(
            e.traffic
                .view("")
                .iter()
                .any(|r| r.direction == oscq_core::Direction::In && r.path == "/g0/p0"),
            "logged as received"
        );
    }

    /// PRD 10, measured on the synthetic server's 5000 node document. Run on a release build:
    /// `cargo test --release -p void-oscq -- --ignored budget_ --nocapture --test-threads=1`.
    mod budgets {
        use super::*;
        use crate::synthetic::{Reply, large_document};

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
                vec![("/g0/p0?VALUE".into(), Reply::json(r#"{"VALUE":[0.5]}"#))],
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
