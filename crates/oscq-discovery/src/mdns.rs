//! `mdns-sd` backend.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mdns_sd::{ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent, VERIFY_TIMEOUT_DEFAULT};
use oscq_core::{InterfaceRef, ServiceKind};

use crate::registry::{Observation, Registry};
use crate::{Discovery, DiscoveryError, DiscoveryEvent, OSC_SERVICE, OSCQUERY_SERVICE, Result};

#[derive(Clone)]
pub struct MdnsSdConfig {
    /// Period of the liveness check, which asks every advertised instance to answer again and
    /// marks it stale if it does not. `None` disables it. On by default on Windows only, where
    /// removal events may not arrive.
    pub liveness_interval: Option<Duration>,
    /// How long a liveness query waits for an answer.
    pub verify_timeout: Duration,
    /// How often the system is asked whether a query may leave this machine, while it is
    /// refusing. Once a query has left, the question is settled and is not asked again.
    pub access_interval: Duration,
    /// How that question is asked. `None` uses [`multicast_query`], a real query for the
    /// OSCQuery service type; a test gives its own answer instead.
    pub access_probe: Option<crate::AccessProbe>,
}

impl std::fmt::Debug for MdnsSdConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdnsSdConfig")
            .field("liveness_interval", &self.liveness_interval)
            .field("verify_timeout", &self.verify_timeout)
            .field("access_interval", &self.access_interval)
            .field("access_probe", &self.access_probe.is_some())
            .finish()
    }
}

impl Default for MdnsSdConfig {
    fn default() -> Self {
        Self {
            liveness_interval: cfg!(windows).then_some(Duration::from_secs(15)),
            verify_timeout: VERIFY_TIMEOUT_DEFAULT,
            access_interval: Duration::from_secs(2),
            access_probe: None,
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
    // The state of the local network, asked for at once and then every `access_interval` until
    // a query leaves. Only a change is reported, so a window at rest is not woken by the asking.
    let probe = config.access_probe.clone().unwrap_or_else(default_probe);
    let mut access: Option<crate::LocalNetwork> = None;
    let mut next_access = Some(Instant::now());
    loop {
        if let Some(due) = next_access
            && due <= Instant::now()
        {
            let now = match probe() {
                Ok(()) => crate::LocalNetwork::Open,
                Err(e) => crate::LocalNetwork::Refused {
                    reason: e.to_string(),
                },
            };
            let open = now == crate::LocalNetwork::Open;
            if access.as_ref() != Some(&now) {
                match &now {
                    crate::LocalNetwork::Refused { reason } => {
                        tracing::warn!(target: "oscq_core::failure", %reason, "the system refused a query to the local network: nothing has been asked of it");
                    }
                    // The gap between this line and the first server resolved is what the
                    // application costs once the system lets go; what came before it is the
                    // system taking its time, or waiting on the user.
                    crate::LocalNetwork::Open => {
                        tracing::info!(target: "oscq_inspector::startup", "a query left this machine: the local network is open");
                    }
                }
                access = Some(now.clone());
                if out.send(DiscoveryEvent::LocalNetwork(now)).is_err() {
                    return;
                }
            }
            // The probe is a real query, so the send that proves access is also the query that
            // brings the answers in: `mdns-sd` hears them on the port it already listens on,
            // whatever its own schedule, which doubles to an hour and would otherwise leave a
            // late answer from the system unnoticed for minutes.
            next_access = (!open).then(|| Instant::now() + config.access_interval);
        }
        let wait = [
            next_check.map(|t| t.saturating_duration_since(Instant::now())),
            next_access.map(|t| t.saturating_duration_since(Instant::now())),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(Duration::from_secs(3600));
        let msg = flume::Selector::new()
            .recv(&oscquery, |r| Msg::Service(ServiceKind::OscQuery, r))
            .recv(&osc, |r| Msg::Service(ServiceKind::OscOnly, r))
            .recv(&stop, |_| Msg::Stop)
            .wait_timeout(wait);
        let events = match msg {
            Err(_) => {
                if next_check.is_some_and(|t| t <= Instant::now()) {
                    for fullname in registry.live_sources() {
                        if let Err(e) = daemon.verify(fullname.clone(), config.verify_timeout) {
                            tracing::warn!(%fullname, error = %e, "liveness check not sent");
                        }
                    }
                    next_check = config.liveness_interval.map(|i| Instant::now() + i);
                }
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

/// The address every mDNS query goes to.
const MDNS_V4: &str = "224.0.0.251:5353";

/// Asks the system whether a query may leave, by sending one.
///
/// The packet is a real mDNS query for the OSCQuery service type, so it costs nothing a browse
/// would not send anyway, and the answers it draws are multicast back to port 5353, where the
/// `mdns-sd` daemon is already listening: a query that gets through is also a query that fills
/// the daemon's cache, whatever its own schedule says.
///
/// A send is tried on every interface that is not loopback, and the first that succeeds settles
/// it. The refusal reported is the last one, in the system's own words; on macOS, while the
/// local network has not been granted, that is "No route to host".
fn default_probe() -> crate::AccessProbe {
    std::sync::Arc::new(|| {
        let dest: std::net::SocketAddr = MDNS_V4.parse().expect("a literal address");
        let mut last = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no interface to send a query on",
        );
        let interfaces = if_addrs::get_if_addrs()?;
        for ifa in interfaces {
            if ifa.is_loopback() {
                continue;
            }
            let std::net::IpAddr::V4(v4) = ifa.ip() else {
                continue;
            };
            let socket = match std::net::UdpSocket::bind(std::net::SocketAddr::new(
                std::net::IpAddr::V4(v4),
                0,
            )) {
                Ok(s) => s,
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            // One hop: a query for this link, not for the wider network.
            let _ = socket.set_multicast_ttl_v4(1);
            match socket.send_to(&ptr_query(OSCQUERY_SERVICE), dest) {
                Ok(_) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    })
}

/// An mDNS query for the PTR records of `service`, which must end in a dot. The response bit is
/// left clear, so answers are multicast and every listener on this machine hears them, the
/// `mdns-sd` daemon among them.
fn ptr_query(service: &str) -> Vec<u8> {
    // Header: id 0, no flags, one question, no answers.
    let mut packet = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in service.trim_end_matches('.').split('.') {
        let bytes = label.as_bytes();
        // A label is at most 63 bytes; the service types of this crate are far shorter.
        if bytes.is_empty() || bytes.len() > 63 {
            continue;
        }
        packet.push(bytes.len() as u8);
        packet.extend_from_slice(bytes);
    }
    packet.push(0);
    // QTYPE PTR, QCLASS IN.
    packet.extend_from_slice(&[0, 12, 0, 1]);
    packet
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
