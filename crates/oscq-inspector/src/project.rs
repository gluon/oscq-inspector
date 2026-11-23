//! The files the File menu reads and writes (PRD 9.11): a project, a server list and a preset
//! bank. Each one may come from anywhere, so it is read with a size limit and checked whole before
//! anything of it is applied (CLAUDE.md law 5).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use oscq_discovery::{HISTORY_LEN, ManualEntry};
use serde::{Deserialize, Serialize};

use crate::config::{Config, MapPosition};
use crate::presets::{Preset, SLOTS};

pub const PROJECT_FORMAT: &str = "oscq-inspector-project";
pub const SERVERS_FORMAT: &str = "oscq-inspector-servers";
pub const PRESETS_FORMAT: &str = "oscq-inspector-presets";
/// The version of the three formats this application writes, and the newest it reads.
pub const VERSION: u32 = 1;
/// The extension of a project file.
pub const PROJECT_EXT: &str = "oscqi";
/// The largest file read. Sixteen presets of 5000 methods each stay well under it.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// What a project holds: everything kept about the servers, and nothing about this machine. The
/// window, the theme, the writable filter, the folded preset section and the map's poll interval
/// stay in the config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub manual_entries: Vec<String>,
    #[serde(default)]
    pub osc_ports: BTreeMap<String, u16>,
    #[serde(default)]
    pub pinned_paths: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub presets: BTreeMap<usize, Preset>,
    #[serde(default)]
    pub map_positions: Vec<MapPosition>,
}

impl Project {
    /// The project part of `config`.
    pub fn of(config: &Config) -> Self {
        Self {
            format: PROJECT_FORMAT.to_owned(),
            version: VERSION,
            manual_entries: config.manual_entries.clone(),
            osc_ports: config.osc_ports.clone(),
            pinned_paths: config.pinned_paths.clone(),
            presets: config.presets.clone(),
            map_positions: config.map_positions.clone(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        pretty(self)
    }

    /// A project read from a file, checked whole: its format and version, every manual entry,
    /// every preset slot. Nothing of a file that fails one check is used.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let p: Project =
            serde_json::from_slice(bytes).map_err(|e| format!("not a project file: {e}"))?;
        check_format(&p.format, p.version, PROJECT_FORMAT, "a project")?;
        if p.manual_entries.len() > HISTORY_LEN {
            return Err(format!(
                "{} manual entries, more than the {HISTORY_LEN} the list holds",
                p.manual_entries.len()
            ));
        }
        for e in &p.manual_entries {
            ManualEntry::parse(e).map_err(|err| format!("manual entry {e:?}: {err}"))?;
        }
        check_slots(&p.presets)?;
        Ok(p)
    }

    /// Replaces in `config` everything a project holds. The settings of this machine stay.
    pub fn apply(self, config: &mut Config) {
        config.manual_entries = self.manual_entries;
        config.osc_ports = self.osc_ports;
        config.pinned_paths = self.pinned_paths;
        config.presets = self.presets;
        config.map_positions = self.map_positions;
        config.bound_map_positions();
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PresetBank {
    format: String,
    version: u32,
    presets: BTreeMap<usize, Preset>,
}

/// The sixteen preset slots as a file of their own.
pub fn presets_bytes(presets: &BTreeMap<usize, Preset>) -> Vec<u8> {
    pretty(&PresetBank {
        format: PRESETS_FORMAT.to_owned(),
        version: VERSION,
        presets: presets.clone(),
    })
}

/// A preset bank read from a file, checked whole. It replaces every slot, the empty ones
/// included, as the `read` message of Max's `preset` object does.
pub fn parse_presets(bytes: &[u8]) -> Result<BTreeMap<usize, Preset>, String> {
    let bank: PresetBank =
        serde_json::from_slice(bytes).map_err(|e| format!("not a preset file: {e}"))?;
    check_format(&bank.format, bank.version, PRESETS_FORMAT, "a preset file")?;
    check_slots(&bank.presets)?;
    Ok(bank.presets)
}

#[derive(Debug, Deserialize)]
struct ServerList {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    version: Option<u32>,
    servers: Vec<ServerRecord>,
}

#[derive(Debug, Deserialize)]
struct ServerRecord {
    host: String,
    port: u16,
    #[serde(default)]
    kind: Option<String>,
}

/// The servers of a list file, as manual entries in the order of the file, and how many of its
/// servers cannot be one: a device that speaks OSC alone, or a host and port that is not valid.
/// A list from another network is a list of addresses, so every server becomes a manual entry.
pub fn parse_servers(bytes: &[u8]) -> Result<(Vec<ManualEntry>, usize), String> {
    let list: ServerList =
        serde_json::from_slice(bytes).map_err(|e| format!("not a server list: {e}"))?;
    // The first lists exported carried no format; one that names another is refused.
    if let Some(format) = &list.format {
        check_format(
            format,
            list.version.unwrap_or(VERSION),
            SERVERS_FORMAT,
            "a server list",
        )?;
    }
    let mut entries: Vec<ManualEntry> = Vec::new();
    let mut skipped = 0;
    for s in list.servers {
        if s.kind.as_deref() == Some("osc") {
            skipped += 1;
            continue;
        }
        match ManualEntry::new(&s.host, s.port) {
            Ok(e) if !entries.iter().any(|k| same(k, &e)) => entries.push(e),
            Ok(_) => {}
            Err(_) => skipped += 1,
        }
    }
    Ok((entries, skipped))
}

/// Two entries for the same server, as `ManualHistory` compares them.
pub fn same(a: &ManualEntry, b: &ManualEntry) -> bool {
    a.port() == b.port() && a.host().eq_ignore_ascii_case(b.host())
}

/// Reads a file of at most [`MAX_BYTES`]. A larger one is refused before it is read, and one
/// that grows while it is read is refused too.
pub fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!(
            "{}: larger than {} MB, not read",
            path.display(),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

fn check_format(format: &str, version: u32, want: &str, what: &str) -> Result<(), String> {
    if format != want {
        return Err(format!("not {what}: its format is {format:?}"));
    }
    if version > VERSION {
        return Err(format!(
            "{what} of version {version}, newer than this application reads ({VERSION})"
        ));
    }
    Ok(())
}

fn check_slots(presets: &BTreeMap<usize, Preset>) -> Result<(), String> {
    match presets.keys().find(|&&k| !(1..=SLOTS).contains(&k)) {
        Some(k) => Err(format!("preset slot {k}: the slots are 1 to {SLOTS}")),
        None => Ok(()),
    }
}

fn pretty(value: &impl Serialize) -> Vec<u8> {
    // Serialising these plain structs cannot fail; an empty object if it ever did.
    let mut out = serde_json::to_vec_pretty(value).unwrap_or_else(|_| b"{}".to_vec());
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::presets::{PresetArg, PresetValue};

    fn preset(server: &str) -> Preset {
        Preset {
            server: server.into(),
            server_name: "rig".into(),
            values: vec![PresetValue {
                path: "/a".into(),
                type_tags: "f".into(),
                args: vec![PresetArg::Float(0.5)],
            }],
        }
    }

    fn config() -> Config {
        let mut c = Config {
            manual_entries: vec!["10.0.0.2:5678".into(), "[::1]:9000".into()],
            ..Config::default()
        };
        c.osc_ports.insert("10.0.0.2:5678".into(), 9001);
        c.pinned_paths
            .insert("10.0.0.2:5678".into(), vec!["/a".into()]);
        c.presets.insert(3, preset("10.0.0.2:5678"));
        c.set_map_position("server 10.0.0.2:5678", egui::vec2(0.25, 0.75));
        c
    }

    #[test]
    fn a_project_round_trips_and_leaves_the_machine_settings_alone() {
        let saved = config();
        let bytes = Project::of(&saved).to_bytes();
        let mut other = Config {
            theme: crate::config::Theme::Light,
            hide_read_only: true,
            peers_poll_ms: 5000,
            manual_entries: vec!["gone:1".into()],
            ..Config::default()
        };
        other.presets.insert(9, preset("gone:1"));
        Project::parse(&bytes).unwrap().apply(&mut other);
        assert_eq!(other.manual_entries, saved.manual_entries);
        assert_eq!(other.osc_ports, saved.osc_ports);
        assert_eq!(other.pinned_paths, saved.pinned_paths);
        assert_eq!(other.presets, saved.presets, "slot 9 is replaced, not kept");
        assert_eq!(other.map_positions, saved.map_positions);
        assert_eq!(other.theme, crate::config::Theme::Light);
        assert!(other.hide_read_only);
        assert_eq!(other.peers_poll_ms, 5000);
    }

    #[test]
    fn a_file_that_fails_a_check_is_refused_whole() {
        let mut p = Project::of(&config());
        p.format = PRESETS_FORMAT.into();
        assert!(
            Project::parse(&p.to_bytes())
                .unwrap_err()
                .contains("format")
        );

        let mut p = Project::of(&config());
        p.version = VERSION + 1;
        assert!(Project::parse(&p.to_bytes()).unwrap_err().contains("newer"));

        let mut p = Project::of(&config());
        p.presets.insert(SLOTS + 1, preset("a:1"));
        assert!(Project::parse(&p.to_bytes()).unwrap_err().contains("slot"));

        let mut p = Project::of(&config());
        p.manual_entries.push("no port".into());
        assert!(
            Project::parse(&p.to_bytes())
                .unwrap_err()
                .contains("manual")
        );

        let mut p = Project::of(&config());
        p.manual_entries = (1..=HISTORY_LEN as u16 + 1)
            .map(|i| format!("h:{i}"))
            .collect();
        assert!(
            Project::parse(&p.to_bytes())
                .unwrap_err()
                .contains("more than")
        );

        assert!(Project::parse(b"[[[[").is_err());
        // Deeper than serde_json's recursion limit: an error, never a stack overflow.
        let deep = "[".repeat(100_000);
        assert!(Project::parse(deep.as_bytes()).is_err());
    }

    #[test]
    fn a_preset_bank_round_trips_and_is_checked() {
        let c = config();
        let bytes = presets_bytes(&c.presets);
        assert_eq!(parse_presets(&bytes).unwrap(), c.presets);
        let project = Project::of(&c).to_bytes();
        assert!(
            parse_presets(&project).is_err(),
            "a project is not a preset bank"
        );
        let mut bad = c.presets.clone();
        bad.insert(0, preset("a:1"));
        assert!(parse_presets(&presets_bytes(&bad)).is_err());
    }

    #[test]
    fn a_server_list_gives_manual_entries_once_each() {
        let list = serde_json::json!({
            "format": SERVERS_FORMAT,
            "version": 1,
            "servers": [
                {"host": "teststale.local", "port": 5680, "kind": "oscquery"},
                {"host": "TESTSTALE.local", "port": 5680},
                {"host": "::1", "port": 9000},
                {"host": "osc-only.local", "port": 8000, "kind": "osc"},
                {"host": "", "port": 1},
            ]
        });
        let (entries, skipped) = parse_servers(list.to_string().as_bytes()).unwrap();
        let shown: Vec<String> = entries.iter().map(ToString::to_string).collect();
        assert_eq!(shown, ["teststale.local:5680", "[::1]:9000"]);
        assert_eq!(skipped, 2);
        // A list exported before the format was named is still read.
        let old = br#"{"servers":[{"host":"a.local","port":1}]}"#;
        assert_eq!(parse_servers(old).unwrap().0.len(), 1);
        let wrong = serde_json::json!({"format": PROJECT_FORMAT, "servers": []});
        assert!(parse_servers(wrong.to_string().as_bytes()).is_err());
    }

    #[test]
    fn a_file_larger_than_the_limit_is_not_read() {
        let dir = std::env::temp_dir().join(format!("oscqi-limit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.oscqi");
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_BYTES + 1).unwrap();
        assert!(read_limited(&path).unwrap_err().contains("larger"));
        f.set_len(16).unwrap();
        assert_eq!(read_limited(&path).unwrap().len(), 16);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
