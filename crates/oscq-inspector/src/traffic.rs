//! Traffic log (PRD 9.5): bounded ring buffer, text and direction filters, pause, export text.
//! A block, a preset recall, is one record holding its messages.

use std::collections::VecDeque;
use std::time::Instant;

use oscq_core::{Direction, TrafficRecord};
use rosc::OscType;

pub const CAPACITY: usize = 10_000;

/// Which directions the view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirFilter {
    #[default]
    All,
    In,
    Out,
}

impl DirFilter {
    pub fn matches(self, d: Direction) -> bool {
        match self {
            DirFilter::All => true,
            DirFilter::In => d == Direction::In,
            DirFilter::Out => d == Direction::Out,
        }
    }
}

#[derive(Debug, Default)]
pub struct TrafficLog {
    records: VecDeque<TrafficRecord>,
    /// While paused, the view shows only the records that existed when pause was pressed.
    paused_at: Option<usize>,
    /// Records dropped from the front since pause, so the frozen view stays the same.
    dropped_since_pause: usize,
}

impl TrafficLog {
    pub fn push(&mut self, record: TrafficRecord) {
        if self.records.len() == CAPACITY {
            self.records.pop_front();
            if self.paused_at.is_some() {
                self.dropped_since_pause += 1;
            }
        }
        self.records.push_back(record);
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.paused_at = self.paused_at.map(|_| 0);
        self.dropped_since_pause = 0;
    }

    pub fn paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused_at = paused.then_some(self.records.len());
        self.dropped_since_pause = 0;
    }

    /// The records the view shows, oldest first, in the directions of `dir`, matching `filter` on
    /// path or type tags, a block matching when its label or one of its messages does.
    pub fn view(&self, filter: &str, dir: DirFilter) -> Vec<&TrafficRecord> {
        let end = match self.paused_at {
            Some(n) => n
                .saturating_sub(self.dropped_since_pause)
                .min(self.records.len()),
            None => self.records.len(),
        };
        let f = filter.trim();
        self.records
            .iter()
            .take(end)
            .filter(|r| dir.matches(r.direction))
            .filter(|r| {
                let hit = |r: &TrafficRecord| r.path.contains(f) || r.type_tags.contains(f);
                f.is_empty() || hit(r) || r.block.iter().any(hit)
            })
            .collect()
    }
}

pub fn args_text(args: &[OscType]) -> String {
    args.iter()
        .map(|a| match a {
            OscType::Int(v) => v.to_string(),
            OscType::Long(v) => v.to_string(),
            OscType::Float(v) => format!("{v}"),
            OscType::Double(v) => format!("{v}"),
            OscType::String(s) => format!("{s:?}"),
            OscType::Char(c) => format!("'{c}'"),
            OscType::Bool(b) => b.to_string(),
            OscType::Nil => "nil".to_owned(),
            OscType::Inf => "impulse".to_owned(),
            OscType::Color(c) => {
                format!("#{:02X}{:02X}{:02X}{:02X}", c.red, c.green, c.blue, c.alpha)
            }
            OscType::Array(a) => format!("[{}]", args_text(&a.content)),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One record as a text line: seconds since `origin`, direction, path, type tags, arguments. A
/// block gives its label and the number of its messages.
pub fn line(origin: Instant, r: &TrafficRecord) -> String {
    let t = r.at.saturating_duration_since(origin).as_secs_f64();
    let dir = match r.direction {
        Direction::Out => "out",
        Direction::In => "in ",
    };
    if !r.block.is_empty() {
        return format!("{t:>10.3}  {dir}  {}  ({} messages)", r.path, r.block.len());
    }
    format!(
        "{t:>10.3}  {dir}  {}  ,{}  {}",
        r.path,
        r.type_tags,
        args_text(&r.args)
    )
}

/// A message of a block, indented under the block's line.
pub fn block_line(m: &TrafficRecord) -> String {
    format!(
        "{:>10}  {:>3}    {}  ,{}  {}",
        "",
        "",
        m.path,
        m.type_tags,
        args_text(&m.args)
    )
}

pub fn export(origin: Instant, records: &[&TrafficRecord]) -> String {
    let mut out = String::from("# time_s  dir  path  type_tags  args\n");
    for r in records {
        out.push_str(&line(origin, r));
        out.push('\n');
        for m in &r.block {
            out.push_str(&block_line(m));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oscq_core::ServerId;

    fn rec(path: &str, i: i32) -> TrafficRecord {
        TrafficRecord {
            at: Instant::now(),
            server_id: ServerId::next(),
            direction: Direction::Out,
            path: path.into(),
            type_tags: "i".into(),
            args: vec![OscType::Int(i)],
            block: Vec::new(),
        }
    }

    /// PRD 9.5: the direction filter, alone and with the text filter, and a block found by the
    /// path of one of its messages.
    #[test]
    fn direction_filter_and_blocks() {
        let mut log = TrafficLog::default();
        log.push(rec("/sent", 1));
        log.push(TrafficRecord {
            direction: Direction::In,
            ..rec("/received", 2)
        });
        log.push(TrafficRecord {
            path: "preset 03".into(),
            type_tags: String::new(),
            args: Vec::new(),
            block: vec![rec("/gain", 3), rec("/name", 4)],
            ..rec("", 0)
        });
        let paths = |v: Vec<&TrafficRecord>| v.iter().map(|r| r.path.clone()).collect::<Vec<_>>();
        assert_eq!(log.view("", DirFilter::All).len(), 3);
        assert_eq!(paths(log.view("", DirFilter::In)), ["/received"]);
        assert_eq!(paths(log.view("", DirFilter::Out)), ["/sent", "preset 03"]);
        assert_eq!(paths(log.view("gain", DirFilter::Out)), ["preset 03"]);
        assert!(log.view("gain", DirFilter::In).is_empty());
        assert!(log.view("sent", DirFilter::In).is_empty());
        let origin = Instant::now();
        let block = log.view("preset", DirFilter::All);
        let text = export(origin, &block);
        assert!(text.contains("preset 03  (2 messages)"), "{text}");
        assert!(
            text.contains("/gain  ,i  3") && text.contains("/name  ,i  4"),
            "{text}"
        );
    }

    #[test]
    fn bounded_oldest_dropped() {
        let mut log = TrafficLog::default();
        for i in 0..(CAPACITY as i32 + 5) {
            log.push(rec("/a", i));
        }
        assert_eq!(log.len(), CAPACITY);
        let first = log.view("", DirFilter::All)[0];
        assert_eq!(first.args, vec![OscType::Int(5)]);
    }

    #[test]
    fn filter_pause_clear() {
        let mut log = TrafficLog::default();
        log.push(rec("/layer1/opacity", 1));
        log.push(rec("/master", 2));
        assert_eq!(log.view("layer", DirFilter::All).len(), 1);
        log.set_paused(true);
        log.push(rec("/master", 3));
        assert_eq!(log.view("", DirFilter::All).len(), 2, "paused view frozen");
        assert_eq!(log.len(), 3, "still recorded");
        log.set_paused(false);
        assert_eq!(log.view("", DirFilter::All).len(), 3);
        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn export_text() {
        let origin = Instant::now();
        let r = rec("/bar", 7);
        let text = export(origin, &[&r]);
        assert!(text.lines().nth(1).unwrap().contains("out  /bar  ,i  7"));
    }
}
