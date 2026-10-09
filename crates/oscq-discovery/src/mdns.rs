//! `mdns-sd` backend.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mdns_sd::{ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent, VERIFY_TIMEOUT_DEFAULT};
use oscq_core::{InterfaceRef, ServiceKind};

use crate::registry::{Observation, Registry};
use crate::{Discovery, DiscoveryError, DiscoveryEvent, OSC_SERVICE, OSCQUERY_SERVICE, Result};

#[derive(Debug, Clone)]
pub struct MdnsSdConfig {
    /// Period of the liveness check, which asks every advertised instance to answer again and
    /// marks it stale if it does not. `None` disables it. On by default on Windows only, where
    /// removal events may not arrive.
    pub liveness_interval: Option<Duration>,
    /// How long a liveness query waits for an answer.
    pub verify_timeout: Duration,
}

impl Default for MdnsSdConfig {
    fn default() -> Self {
        Self {
            liveness_interval: cfg!(windows).then_some(Duration::from_secs(15)),
            verify_timeout: VERIFY_TIMEOUT_DEFAULT,
        }
    }
}

/// Browses `_oscjson._tcp` and `_osc._udp` with `mdns-sd`, on every interface including loopback.
pub struct MdnsSdDiscovery {
    config: MdnsSdConfig,
    running: Option<Running>,
}

struct Running {
    daemon: ServiceDaemon,
    stop: flume::Sender<()>,
    worker: JoinHandle<()>,
}

impl MdnsSdDiscovery {
    pub fn new(config: MdnsSdConfig) -> Self {
        Self {
            config,
            running: None,
        }
    }
}

impl Default for MdnsSdDiscovery {
    fn default() -> Self {
        Self::new(MdnsSdConfig::default())
    }
}

impl Discovery for MdnsSdDiscovery {
    fn start(&mut self) -> Result<flume::Receiver<DiscoveryEvent>> {
        if self.running.is_some() {
            return Err(DiscoveryError::AlreadyStarted);
        }
        let daemon = ServiceDaemon::new().map_err(daemon_err)?;
        let oscquery = daemon.browse(OSCQUERY_SERVICE).map_err(daemon_err)?;
        let osc = daemon.browse(OSC_SERVICE).map_err(daemon_err)?;
        let (out_tx, out_rx) = flume::unbounded();
        let (stop_tx, stop_rx) = flume::bounded(1);
        let worker_daemon = daemon.clone();
        let config = self.config.clone();
        let worker = std::thread::Builder::new()
            .name("oscq-discovery".into())
            .spawn(move || worker(worker_daemon, config, oscquery, osc, stop_rx, out_tx))
            .map_err(|e| DiscoveryError::Daemon(e.to_string()))?;
        self.running = Some(Running {
            daemon,
            stop: stop_tx,
            worker,
        });
        Ok(out_rx)
    }

    fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            let _ = r.stop.try_send(());
            let _ = r.daemon.shutdown();
            let _ = r.worker.join();
        }
    }
}

impl Drop for MdnsSdDiscovery {
    fn drop(&mut self) {
        self.stop();
    }
}

fn daemon_err(e: mdns_sd::Error) -> DiscoveryError {
    DiscoveryError::Daemon(e.to_string())
}

enum Msg {
    Service(
        ServiceKind,
        std::result::Result<ServiceEvent, flume::RecvError>,
    ),
    Stop,
}

fn worker(
    daemon: ServiceDaemon,
    config: MdnsSdConfig,
    oscquery: flume::Receiver<ServiceEvent>,
    osc: flume::Receiver<ServiceEvent>,
    stop: flume::Receiver<()>,
    out: flume::Sender<DiscoveryEvent>,
) {
    let mut registry = Registry::new();
    let mut next_check = config.liveness_interval.map(|i| Instant::now() + i);
    loop {
        let wait = next_check
            .map(|t| t.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        let msg = flume::Selector::new()
            .recv(&oscquery, |r| Msg::Service(ServiceKind::OscQuery, r))
            .recv(&osc, |r| Msg::Service(ServiceKind::OscOnly, r))
            .recv(&stop, |_| Msg::Stop)
            .wait_timeout(wait);
        let events = match msg {
            Err(_) => {
                for fullname in registry.live_sources() {
                    if let Err(e) = daemon.verify(fullname.clone(), config.verify_timeout) {
                        tracing::warn!(%fullname, error = %e, "liveness check not sent");
                    }
                }
                next_check = config.liveness_interval.map(|i| Instant::now() + i);
                continue;
            }
            Ok(Msg::Stop) | Ok(Msg::Service(_, Err(_))) => return,
            Ok(Msg::Service(kind, Ok(event))) => handle(&mut registry, kind, event),
        };
        for e in events {
            if out.send(e).is_err() {
                return;
            }
        }
    }
}

fn handle(registry: &mut Registry, kind: ServiceKind, event: ServiceEvent) -> Vec<DiscoveryEvent> {
    match event {
        ServiceEvent::ServiceResolved(rs) => {
            tracing::debug!(fullname = %rs.fullname, host = %rs.host, port = rs.port, "resolved");
            observation(kind, &rs)
                .and_then(|obs| registry.observe(obs))
                .into_iter()
                .collect()
        }
        ServiceEvent::ServiceRemoved(_, fullname) => {
            tracing::debug!(%fullname, "removed");
            registry.source_removed(&fullname)
        }
        _ => Vec::new(),
    }
}

fn observation(kind: ServiceKind, rs: &ResolvedService) -> Option<Observation> {
    if !rs.is_valid() {
        return None;
    }
    Some(Observation {
        kind,
        service_type: rs.ty_domain.clone(),
        fullname: rs.fullname.clone(),
        host: rs.host.clone(),
        port: rs.port,
        addresses: rs.addresses.iter().map(scoped_address).collect(),
    })
}

/// An address with the interfaces it was seen on: every interface for IPv4, the scope for IPv6.
pub fn scoped_address(ip: &ScopedIp) -> (std::net::IpAddr, Vec<InterfaceRef>) {
    let to_ref = |i: &mdns_sd::InterfaceId| InterfaceRef {
        name: i.name.clone(),
        index: i.index,
    };
    let interfaces = match ip {
        ScopedIp::V4(v4) => v4.interface_ids().iter().map(to_ref).collect(),
        ScopedIp::V6(v6) => vec![to_ref(v6.scope_id())],
        _ => Vec::new(),
    };
    (ip.to_ip_addr(), interfaces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::{InterfaceId, ScopedIpV4};

    #[test]
    fn ipv4_address_keeps_its_interface() {
        let ip = ScopedIp::V4(ScopedIpV4::new(
            "192.168.1.20".parse().unwrap(),
            InterfaceId {
                name: "en1".into(),
                index: 14,
            },
        ));
        let (addr, ifaces) = scoped_address(&ip);
        assert_eq!(addr.to_string(), "192.168.1.20");
        assert_eq!(
            ifaces,
            vec![InterfaceRef {
                name: "en1".into(),
                index: 14
            }]
        );
    }

    #[test]
    fn default_liveness_only_on_windows() {
        assert_eq!(
            MdnsSdConfig::default().liveness_interval.is_some(),
            cfg!(windows)
        );
    }
}
