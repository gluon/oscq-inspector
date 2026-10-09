//! Lists OSCQuery and OSC servers on the LAN as they are discovered.
//!
//! ```text
//! cargo run -p oscq-discovery --example oscq_discover
//! cargo run -p oscq-discovery --example oscq_discover -- --for 10
//! cargo run -p oscq-discovery --example oscq_discover -- --liveness 15
//! ```
//!
//! Every event is printed with the time since start, each address with the interfaces it was
//! seen on. Two seconds after start, a summary lists the servers known at that point.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use oscq_core::{DiscoveredServer, ServerId, ServiceKind};
use oscq_discovery::{Discovery, DiscoveryEvent, MdnsSdConfig, MdnsSdDiscovery};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut run_for = None;
    let mut config = MdnsSdConfig::default();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).and_then(|v| v.parse::<u64>().ok());
        match (args[i].as_str(), value) {
            ("--for", Some(s)) => run_for = Some(Duration::from_secs(s)),
            ("--liveness", Some(s)) => config.liveness_interval = Some(Duration::from_secs(s)),
            _ => {
                eprintln!("usage: oscq_discover [--for SECONDS] [--liveness SECONDS]");
                std::process::exit(2);
            }
        }
        i += 2;
    }

    let mut discovery = MdnsSdDiscovery::new(config.clone());
    let rx = match discovery.start() {
        Ok(rx) => rx,
        Err(e) => {
            eprintln!("discovery failed to start: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "browsing {} and {}, liveness check {}",
        oscq_discovery::OSCQUERY_SERVICE,
        oscq_discovery::OSC_SERVICE,
        match config.liveness_interval {
            Some(d) => format!("every {} s", d.as_secs()),
            None => "off".to_owned(),
        }
    );

    let start = Instant::now();
    let summary_at = start + Duration::from_secs(2);
    let mut summary_done = false;
    let mut known: BTreeMap<ServerId, DiscoveredServer> = BTreeMap::new();
    loop {
        let now = Instant::now();
        if run_for.is_some_and(|d| now >= start + d) {
            break;
        }
        let next_deadline = if summary_done {
            run_for.map(|d| start + d)
        } else {
            Some(summary_at)
        };
        let event = match next_deadline {
            Some(t) => rx.recv_timeout(t.saturating_duration_since(now)).ok(),
            None => rx.recv().ok(),
        };
        if let Some(event) = event {
            let (label, s) = match &event {
                DiscoveryEvent::Found(s) => ("FOUND  ", s),
                DiscoveryEvent::Changed(s) => ("CHANGED", s),
                DiscoveryEvent::Stale(s) => ("STALE  ", s),
            };
            println!(
                "+{:>6.2}s {label} {}",
                start.elapsed().as_secs_f64(),
                describe(s)
            );
            known.insert(s.id, s.clone());
        } else if rx.is_disconnected() {
            eprintln!("discovery stopped");
            break;
        }
        if !summary_done && Instant::now() >= summary_at {
            summary_done = true;
            let live = known.values().filter(|s| !s.stale);
            let query = live
                .clone()
                .filter(|s| s.kind == ServiceKind::OscQuery)
                .count();
            let osc = live.filter(|s| s.kind == ServiceKind::OscOnly).count();
            println!("--- after 2 s: {query} OSCQuery server(s), {osc} OSC only");
            for s in known.values() {
                println!("    {}", describe(s));
            }
            println!("---");
        }
    }
    discovery.stop();
}

fn describe(s: &DiscoveredServer) -> String {
    let kind = match s.kind {
        ServiceKind::OscQuery => "OSCQuery".to_owned(),
        ServiceKind::OscOnly => "OSC only, not connectable".to_owned(),
    };
    let mut out = format!(
        "\"{}\" [{kind}]{} {}:{}",
        s.name,
        if s.stale { " [stale]" } else { "" },
        s.host,
        s.port
    );
    if let Some(url) = s.http_base() {
        out.push_str(&format!(" -> {url}"));
    }
    for a in &s.addresses {
        let ifaces: Vec<String> = a
            .interfaces
            .iter()
            .map(|i| format!("{}#{}", i.name, i.index))
            .collect();
        out.push_str(&format!(
            "\n             {} {:?} on {}",
            a.ip,
            a.class,
            if ifaces.is_empty() {
                "unknown interface".to_owned()
            } else {
                ifaces.join(", ")
            }
        ));
    }
    out
}
