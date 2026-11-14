//! The single config file (PRD 9.7): window geometry, manual entries, last selected server,
//! theme preference, pinned paths, the OSC ports the user set, the preset slots, whether the tree
//! shows writable nodes only, whether the preset section is open, the peers poll interval, and
//! the positions of the map's nodes set by hand. Nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The peers poll interval when the config names none (PRD 7.6, 9.10).
pub const PEERS_POLL_DEFAULT_MS: u64 = 2000;
/// The shortest and the longest interval a config can ask for. A config is edited by hand: an
/// interval of zero would repaint the window without rest and ask every server without pause.
pub const PEERS_POLL_MIN_MS: u64 = 250;
pub const PEERS_POLL_MAX_MS: u64 = 60_000;
/// Positions of the map's nodes kept at most. Past it, the position moved least recently goes.
pub const MAP_POSITIONS_MAX: usize = 256;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A position of a node of the map set by hand (PRD 9.10): the node's identity, and its centre
/// as a fraction of the frame of the map, so that it keeps its place whatever the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPosition {
    pub node: String,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    /// Follow the OS setting.
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub window: Option<WindowGeometry>,
    /// Manual `host:port` entries, most recent first, as `ManualHistory` keeps them.
    pub manual_entries: Vec<String>,
    /// `host:port` of the last selected server.
    pub last_selected: Option<String>,
    pub theme: Theme,
    /// Pinned full paths, per server `host:port`.
    pub pinned_paths: BTreeMap<String, Vec<String>>,
    /// OSC ports set by the user, per server `host:port`, for servers that do not advertise one:
    /// used in place of the port inferred from the HTTP port.
    pub osc_ports: BTreeMap<String, u16>,
    /// Preset slots 1 to 16 that hold something (PRD 9.9).
    pub presets: BTreeMap<usize, crate::presets::Preset>,
    /// The tree hides the nodes that cannot be written, and the containers left with none.
    pub hide_read_only: bool,
    /// The preset section is open; it is folded until the user opens it.
    pub presets_open: bool,
    /// Milliseconds between two readings of `?PEERS` while the map tab is visible (PRD 9.10).
    /// Read through [`Config::peers_poll`], which keeps it within bounds.
    pub peers_poll_ms: u64,
    /// The positions of the map's nodes set by hand, the one moved least recently first, at most
    /// [`MAP_POSITIONS_MAX`]. Only nodes whose identity outlives a connection (`map::Node::kept`).
    pub map_positions: Vec<MapPosition>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window: None,
            manual_entries: Vec::new(),
            last_selected: None,
            theme: Theme::default(),
            pinned_paths: BTreeMap::new(),
            osc_ports: BTreeMap::new(),
            presets: BTreeMap::new(),
            hide_read_only: false,
            presets_open: false,
            peers_poll_ms: PEERS_POLL_DEFAULT_MS,
            map_positions: Vec::new(),
        }
    }
}

impl Config {
    /// The peers poll interval, kept between [`PEERS_POLL_MIN_MS`] and [`PEERS_POLL_MAX_MS`].
    pub fn peers_poll(&self) -> Duration {
        Duration::from_millis(
            self.peers_poll_ms
                .clamp(PEERS_POLL_MIN_MS, PEERS_POLL_MAX_MS),
        )
    }

    /// The position set by hand for the map's node `node`, as a fraction of the map's frame.
    pub fn map_position(&self, node: &str) -> Option<egui::Vec2> {
        self.map_positions
            .iter()
            .find(|p| p.node == node)
            .map(|p| egui::Vec2::new(p.x, p.y))
    }

    /// Keeps `at` as the position of `node`, the most recent; the least recent goes past
    /// [`MAP_POSITIONS_MAX`].
    pub fn set_map_position(&mut self, node: &str, at: egui::Vec2) {
        self.map_positions.retain(|p| p.node != node);
        self.map_positions.push(MapPosition {
            node: node.to_owned(),
            x: at.x,
            y: at.y,
        });
        self.bound_map_positions();
    }

    /// No position that is not a number, none twice, at most [`MAP_POSITIONS_MAX`], the most
    /// recent kept. A config is edited by hand.
    fn bound_map_positions(&mut self) {
        self.map_positions
            .retain(|p| p.x.is_finite() && p.y.is_finite() && !p.node.is_empty());
        let mut seen = std::collections::HashSet::new();
        let mut kept: Vec<MapPosition> = Vec::new();
        for p in self.map_positions.drain(..).rev() {
            if seen.insert(p.node.clone()) {
                kept.push(p);
            }
        }
        kept.truncate(MAP_POSITIONS_MAX);
        kept.reverse();
        self.map_positions = kept;
    }

    /// Reads the config. A missing file gives the defaults; an unreadable or invalid file gives the
    /// defaults and the reason, so the user is told rather than silently reset.
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Config>(&bytes) {
                Ok(mut c) => {
                    c.bound_map_positions();
                    (c, None)
                }
                Err(e) => (
                    Config::default(),
                    Some(format!("config {} ignored: {e}", path.display())),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(e) => (
                Config::default(),
                Some(format!("config {} unreadable: {e}", path.display())),
            ),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // Serialising this plain struct cannot fail; fall back to an empty object regardless.
        serde_json::to_vec_pretty(self).unwrap_or_else(|_| b"{}".to_vec())
    }
}

/// Writes `bytes` to `path` atomically: a temporary file in the same directory, flushed, then
/// renamed over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config".to_owned())
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// The OS standard config directory for this application.
pub fn app_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("void-oscq"))
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map's positions set by hand are bounded: at most `MAP_POSITIONS_MAX`, the least
    /// recently moved going first; a node moved again becomes the most recent; a config edited
    /// by hand loses what is not a number and what is listed twice, keeping the latest.
    #[test]
    fn map_positions_are_bounded_least_recent_first() {
        let mut c = Config::default();
        for i in 0..MAP_POSITIONS_MAX + 44 {
            c.set_map_position(
                &format!("server 10.0.{}.{}:5000", i / 250, i % 250),
                egui::vec2(0.1, 0.2),
            );
        }
        assert_eq!(c.map_positions.len(), MAP_POSITIONS_MAX);
        assert_eq!(
            c.map_positions[0].node, "server 10.0.0.44:5000",
            "the 44 oldest gone"
        );
        c.set_map_position("server 10.0.0.44:5000", egui::vec2(0.9, 0.9));
        assert_eq!(c.map_positions.len(), MAP_POSITIONS_MAX);
        let last = c.map_positions.last().unwrap();
        assert_eq!((last.node.as_str(), last.x), ("server 10.0.0.44:5000", 0.9));
        assert_eq!(
            c.map_position("server 10.0.0.44:5000"),
            Some(egui::vec2(0.9, 0.9))
        );

        let dir = temp_dir("map-positions");
        let path = config_path(&dir);
        let text = r#"{"map_positions": [
            {"node": "server a:1", "x": 0.1, "y": 0.1},
            {"node": "server b:1", "x": 1e39, "y": 0.1},
            {"node": "", "x": 0.1, "y": 0.1},
            {"node": "server a:1", "x": 0.7, "y": 0.7}
        ]}"#;
        write_atomic(&path, text.as_bytes()).unwrap();
        let (read, warning) = Config::load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(warning, None);
        assert_eq!(
            read.map_positions,
            [MapPosition {
                node: "server a:1".into(),
                x: 0.7,
                y: 0.7
            }]
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("oscq-inspector-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trip() {
        let mut c = Config {
            window: Some(WindowGeometry {
                x: 10.0,
                y: 20.0,
                width: 900.0,
                height: 600.0,
            }),
            manual_entries: vec!["10.0.0.2:2345".into(), "[::1]:80".into()],
            last_selected: Some("studio.local:2345".into()),
            theme: Theme::Dark,
            ..Config::default()
        };
        c.pinned_paths
            .insert("studio.local:2345".into(), vec!["/test/my_float".into()]);
        let dir = temp_dir("round-trip");
        let path = config_path(&dir);
        write_atomic(&path, &c.to_bytes()).unwrap();
        assert_eq!(Config::load(&path), (c, None));
        assert!(
            !dir.join(".config.json.tmp").exists(),
            "temporary file renamed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_gives_defaults_silently() {
        let dir = temp_dir("missing");
        assert_eq!(Config::load(&config_path(&dir)), (Config::default(), None));
    }

    #[test]
    fn invalid_file_gives_defaults_and_says_so() {
        let dir = temp_dir("invalid");
        let path = config_path(&dir);
        write_atomic(&path, b"{not json").unwrap();
        let (c, warning) = Config::load(&path);
        assert_eq!(c, Config::default());
        assert!(warning.unwrap().contains("ignored"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let dir = temp_dir("partial");
        let path = config_path(&dir);
        write_atomic(&path, br#"{"theme":"Light"}"#).unwrap();
        let (c, warning) = Config::load(&path);
        assert_eq!((c.theme, warning), (Theme::Light, None));
        assert!(c.manual_entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PRD 9.7: the peers poll interval is 2000 ms unless the config says otherwise, a config
    /// written before the field existed included; it is kept, and an interval a hand edit made
    /// absurd is brought back within bounds rather than obeyed.
    #[test]
    fn the_peers_poll_interval_defaults_to_2000_ms_and_stays_within_bounds() {
        assert_eq!(Config::default().peers_poll_ms, 2000);
        assert_eq!(Config::default().peers_poll(), Duration::from_millis(2000));
        let older: Config = serde_json::from_str(r#"{"theme":"Dark"}"#).unwrap();
        assert_eq!(older.peers_poll(), Duration::from_millis(2000));
        let set: Config = serde_json::from_str(r#"{"peers_poll_ms":5000}"#).unwrap();
        assert_eq!(set.peers_poll(), Duration::from_millis(5000));
        let saved: Config = serde_json::from_slice(&set.to_bytes()).unwrap();
        assert_eq!(saved.peers_poll_ms, 5000, "kept in the config");
        for (asked, got) in [
            (0, 250),
            (1, 250),
            (250, 250),
            (60_000, 60_000),
            (u64::MAX, 60_000),
        ] {
            let c = Config {
                peers_poll_ms: asked,
                ..Config::default()
            };
            assert_eq!(c.peers_poll(), Duration::from_millis(got), "{asked}");
        }
    }

    #[test]
    fn rewrite_replaces_existing_file() {
        let dir = temp_dir("rewrite");
        let path = config_path(&dir);
        write_atomic(&path, br#"{"theme":"Light"}"#).unwrap();
        write_atomic(&path, br#"{"theme":"Dark"}"#).unwrap();
        assert_eq!(Config::load(&path).0.theme, Theme::Dark);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
