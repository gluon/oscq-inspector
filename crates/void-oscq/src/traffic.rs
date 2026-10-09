//! Traffic log (PRD 9.5): bounded ring buffer, filter, pause, export text.

use std::collections::VecDeque;
use std::time::Instant;

use oscq_core::{Direction, TrafficRecord};
use rosc::OscType;

pub const CAPACITY: usize = 10_000;

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

    /// The records the view shows, oldest first, matching `filter` on path or type tags.
    pub fn view(&self, filter: &str) -> Vec<&TrafficRecord> {
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
            .filter(|r| f.is_empty() || r.path.contains(f) || r.type_tags.contains(f))
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

/// One record as a text line: seconds since `origin`, direction, path, type tags, arguments.
pub fn line(origin: Instant, r: &TrafficRecord) -> String {
    let t = r.at.saturating_duration_since(origin).as_secs_f64();
    let dir = match r.direction {
        Direction::Out => "out",
        Direction::In => "in ",
    };
    format!(
        "{t:>10.3}  {dir}  {}  ,{}  {}",
        r.path,
        r.type_tags,
        args_text(&r.args)
    )
}

pub fn export(origin: Instant, records: &[&TrafficRecord]) -> String {
    let mut out = String::from("# time_s  dir  path  type_tags  args\n");
    for r in records {
        out.push_str(&line(origin, r));
        out.push('\n');
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
        }
    }

    #[test]
    fn bounded_oldest_dropped() {
        let mut log = TrafficLog::default();
        for i in 0..(CAPACITY as i32 + 5) {
            log.push(rec("/a", i));
        }
        assert_eq!(log.len(), CAPACITY);
        let first = log.view("")[0];
        assert_eq!(first.args, vec![OscType::Int(5)]);
    }

    #[test]
    fn filter_pause_clear() {
        let mut log = TrafficLog::default();
        log.push(rec("/layer1/opacity", 1));
        log.push(rec("/master", 2));
        assert_eq!(log.view("layer").len(), 1);
        log.set_paused(true);
        log.push(rec("/master", 3));
        assert_eq!(log.view("").len(), 2, "paused view frozen");
        assert_eq!(log.len(), 3, "still recorded");
        log.set_paused(false);
        assert_eq!(log.view("").len(), 3);
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
