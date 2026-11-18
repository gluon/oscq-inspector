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
    /// How often a query is sent while the system is refusing to send it.
    pub access_interval: Duration,
    /// How often a query is sent once the system takes them and nothing has answered, for the
    /// first [`QUIET_AFTER`] of a session: the floor under `mdns-sd`'s own schedule, whose gaps
    /// double to an hour, so that access granted late is acted on within this.
    pub retry_interval: Duration,
    /// The same, after [`QUIET_AFTER`]: nobody is at the window any more, and a query every few
    /// seconds for an hour would make this a client nobody runs twice (CLAUDE.md law 7).
    pub quiet_interval: Duration,
    /// How a query is sent. `None` uses [`default_probe`], a real query for the OSCQuery
    /// service type on every interface; a test gives its own answer instead.
    pub access_probe: Option<crate::AccessProbe>,
}

/// When the retry cadence drops from [`MdnsSdConfig::retry_interval`] to
/// [`MdnsSdConfig::quiet_interval`], measured from the start of discovery.
pub const QUIET_AFTER: Duration = Duration::from_secs(120);

impl std::fmt::Debug for MdnsSdConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdnsSdConfig")
            .field("liveness_interval", &self.liveness_interval)
            .field("verify_timeout", &self.verify_timeout)
            .field("access_interval", &self.access_interval)
            .field("retry_interval", &self.retry_interval)
            .field("quiet_interval", &self.quiet_interval)
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
            retry_interval: Duration::from_secs(5),
            quiet_interval: Duration::from_secs(60),
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
    // A query of our own goes out at once and then on the schedule of `interval`, until
    // something answers from an interface that is not loopback. Only a change of state is
    // reported, so a window at rest is not woken by the asking.
    let probe = config.access_probe.clone().unwrap_or_else(default_probe);
    let loopback = loopback_names();
    let started = Instant::now();
    let mut asking = Asking::default();
    let mut next_access = Some(Instant::now());
    loop {
        if let Some(due) = next_access
            && due <= Instant::now()
        {
            if let Some(now) = asking.probed(probe().err().map(|e| e.to_string())) {
                say(&now, asking.taken);
                if out.send(DiscoveryEvent::LocalNetwork(now)).is_err() {
                    return;
                }
            }
            next_access = asking
                .next(started.elapsed(), &config)
                .map(|d| Instant::now() + d);
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
        // Something answered from an interface that is not loopback. That, and only that, is an
        // observation that this machine's queries are getting out and its answers in, so the
        // asking stops here and the window may speak of the network.
        let answered = off_loopback(&events, &loopback);
        // The servers first, then what they prove: an answer about the network never arrives
        // before the answer it was read from.
        for e in events {
            if out.send(e).is_err() {
                return;
            }
        }
        if answered && let Some(now) = asking.answered() {
            next_access = None;
            tracing::info!(target: "oscq_inspector::startup", ms = started.elapsed().as_secs_f64() * 1000.0, "an answer arrived from the local network");
            if out.send(DiscoveryEvent::LocalNetwork(now)).is_err() {
                return;
            }
        }
    }
}

/// The asking: how many queries the system has taken in a row, whether anything has answered
/// from off this machine's loopback, and what was last reported. It decides what to say and
/// when to ask again, and holds the whole of that behaviour so that a test can hold it too,
/// without a network and without a clock.
#[derive(Debug, Default)]
struct Asking {
    /// Queries the system has taken since the last refusal.
    taken: u32,
    /// Something has answered from an interface that is not loopback.
    heard: bool,
    /// What was last reported, so that only a change is sent and a window at rest is not woken.
    said: Option<crate::LocalNetwork>,
}

impl Asking {
    /// What the system did with a query, `None` for a send it took and `Some(reason)` for one it
    /// refused, into the state to report, or `None` when that is what was reported last.
    fn probed(&mut self, refusal: Option<String>) -> Option<crate::LocalNetwork> {
        // What was heard is never taken back by what this machine's own sends do afterwards.
        // The worker stops asking at that point, and this holds whatever it does.
        if self.heard {
            return None;
        }
        let now = match refusal {
            Some(reason) => {
                self.taken = 0;
                crate::LocalNetwork::Refused { reason }
            }
            // A send the system takes says only that it took it: on macOS it takes the sends of
            // a build it has not been told about and drops the packets. So the first one taken
            // is reported as asked, and from the second on, with nothing heard, as unanswered.
            None => {
                self.taken += 1;
                if self.taken > 1 {
                    crate::LocalNetwork::Unanswered
                } else {
                    crate::LocalNetwork::Asked
                }
            }
        };
        self.change(now)
    }

    /// Something answered from off loopback: the one observation that settles the question.
    fn answered(&mut self) -> Option<crate::LocalNetwork> {
        self.heard = true;
        self.change(crate::LocalNetwork::Heard)
    }

    fn change(&mut self, now: crate::LocalNetwork) -> Option<crate::LocalNetwork> {
        if self.said.as_ref() == Some(&now) {
            return None;
        }
        self.said = Some(now.clone());
        Some(now)
    }

    /// How long until the next query of our own, or `None` once something has answered and
    /// there is nothing left to ask.
    ///
    /// While the system refuses, it is asked often: the user may be answering its dialog. Once
    /// it takes the queries and nothing has answered, the cadence is a floor under `mdns-sd`'s
    /// own schedule, whose gaps double to an hour with nothing to reset them: `retry_interval`
    /// while someone may still be at the window, `quiet_interval` after [`QUIET_AFTER`], so
    /// that an application left open all day is not a client nobody runs twice (law 7).
    fn next(&self, since_start: Duration, config: &MdnsSdConfig) -> Option<Duration> {
        if self.heard {
            return None;
        }
        Some(
            if matches!(self.said, Some(crate::LocalNetwork::Refused { .. })) {
                config.access_interval
            } else if since_start < QUIET_AFTER {
                config.retry_interval
            } else {
                config.quiet_interval
            },
        )
    }
}

/// The log line of a state, where what it says belongs: a refusal is a failure of the system,
/// the rest is the course of a startup.
fn say(state: &crate::LocalNetwork, taken: u32) {
    match state {
        crate::LocalNetwork::Refused { reason } => {
            tracing::warn!(target: "oscq_core::failure", %reason, "the system refused a query to the local network: nothing has been asked of it");
        }
        crate::LocalNetwork::Asked => {
            tracing::debug!(
                "the system took a query for the local network; nothing has answered yet"
            );
        }
        crate::LocalNetwork::Unanswered => {
            tracing::info!(target: "oscq_core::failure", queries = taken, "queries are going out and nothing has answered from any interface but loopback: either nothing is there, or this application may not hear the local network");
        }
        // Said by the worker with the time it took, which is what a startup is judged on.
        crate::LocalNetwork::Heard => {}
    }
}

/// The names the system gives its loopback interfaces, read once at the start.
///
/// An answer heard on one of them says nothing about the local network: on macOS a build whose
/// permission has not been granted still hears everything announced on loopback, which is how a
/// window could show one server of this machine while being deaf to every other
/// (PITFALLS.md, 2026-11-18).
fn loopback_names() -> Vec<String> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut names: Vec<String> = interfaces
        .into_iter()
        .filter(if_addrs::Interface::is_loopback)
        .map(|i| i.name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Whether `events` carry an answer from somewhere other than this machine's loopback: an
/// address that is not a loopback address, discovered on an interface the system does not call
/// loopback. Both halves are what `mdns-sd` observed, not what this crate inferred.
fn off_loopback(events: &[DiscoveryEvent], loopback: &[String]) -> bool {
    events
        .iter()
        .filter_map(DiscoveryEvent::server)
        .flat_map(|s| &s.addresses)
        .any(|a| !a.ip.is_loopback() && a.interfaces.iter().any(|i| !loopback.contains(&i.name)))
}

/// The address every mDNS query goes to.
const MDNS_V4: &str = "224.0.0.251:5353";

/// Sends a query for the OSCQuery service type, and reports what the system did with it.
///
/// The packet is a real mDNS query, so it costs nothing a browse would not send anyway, and the
/// answers it draws are multicast back to port 5353, where the `mdns-sd` daemon is already
/// listening: a query that gets through is also a query that fills the daemon's cache, whatever
/// its own schedule says. That is the point of sending it ourselves.
///
/// A send is tried on every interface that is not loopback, and `Ok` means one of them took the
/// packet, nothing more: on macOS a build whose local network permission has not been granted
/// has its sends refused for a few seconds and taken after that, with the packets going nowhere
/// (PITFALLS.md, 2026-11-18). The refusal reported is the last one, in the system's own words;
/// while the local network has not been granted, that is "No route to host".
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

    /// The regression of 2026-11-18, on the human's Mac: 0.4.1 reported the local network open
    /// because the system had taken its query, and said "none found" while a server of that
    /// machine was advertising. macOS takes the sends of a build it has not been told about,
    /// for the first seconds it refuses them and after that it takes them and drops the packets
    /// (PITFALLS.md). So a send that is taken proves nothing, and this says so.
    #[test]
    fn a_query_the_system_takes_is_not_an_answer_from_the_network() {
        let mut asking = Asking::default();
        assert_eq!(asking.probed(None), Some(crate::LocalNetwork::Asked));
        assert_eq!(asking.probed(None), Some(crate::LocalNetwork::Unanswered));
        // And it goes on asking: the permission may arrive a minute in, and the recovery is
        // this query, not a relaunch.
        assert!(
            asking
                .next(Duration::ZERO, &MdnsSdConfig::default())
                .is_some()
        );
        assert_eq!(asking.probed(None), None, "said once, not every time");
    }

    #[test]
    fn the_asking_stops_only_once_something_has_answered() {
        let mut asking = Asking::default();
        let config = MdnsSdConfig::default();
        asking.probed(None);
        assert!(asking.next(Duration::ZERO, &config).is_some());
        assert_eq!(asking.answered(), Some(crate::LocalNetwork::Heard));
        assert_eq!(asking.next(Duration::ZERO, &config), None);
        assert_eq!(asking.answered(), None, "said once, not on every answer");
        // What was heard is not taken back by what our own sends do afterwards.
        assert_eq!(asking.probed(None), None);
        assert_eq!(
            asking.probed(Some("No route to host (os error 65)".to_owned())),
            None
        );
    }

    #[test]
    fn a_refusal_is_reported_once_and_asked_again_at_its_own_cadence() {
        let mut asking = Asking::default();
        let config = MdnsSdConfig {
            access_interval: Duration::from_secs(2),
            retry_interval: Duration::from_secs(5),
            quiet_interval: Duration::from_secs(60),
            ..Default::default()
        };
        let no_route = || Some("No route to host (os error 65)".to_owned());
        assert_eq!(
            asking.probed(no_route()),
            Some(crate::LocalNetwork::Refused {
                reason: "No route to host (os error 65)".to_owned()
            })
        );
        assert_eq!(asking.probed(no_route()), None);
        assert_eq!(
            asking.next(Duration::ZERO, &config),
            Some(Duration::from_secs(2)),
            "while the system refuses, it is asked again soon: the user may be answering it"
        );
        // Taken, then refused again: the count of queries taken starts over, so the window does
        // not jump to "nothing has answered" on the first send after a refusal.
        assert_eq!(asking.probed(None), Some(crate::LocalNetwork::Asked));
        assert!(asking.probed(no_route()).is_some());
        assert_eq!(asking.probed(None), Some(crate::LocalNetwork::Asked));
    }

    #[test]
    fn the_cadence_drops_once_nobody_is_at_the_window() {
        let mut asking = Asking::default();
        let config = MdnsSdConfig {
            retry_interval: Duration::from_secs(5),
            quiet_interval: Duration::from_secs(60),
            ..Default::default()
        };
        asking.probed(None);
        assert_eq!(
            asking.next(QUIET_AFTER - Duration::from_secs(1), &config),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            asking.next(QUIET_AFTER, &config),
            Some(Duration::from_secs(60))
        );
    }

    fn server_at(ip: &str, interface: &str) -> oscq_core::DiscoveredServer {
        oscq_core::DiscoveredServer {
            id: oscq_core::ServerId::next(),
            kind: ServiceKind::OscQuery,
            name: "a server".into(),
            host: "a-host.local".into(),
            port: 7120,
            addresses: vec![oscq_core::ServerAddress {
                ip: ip.parse().unwrap(),
                class: oscq_core::AddressClass::Ipv4,
                interfaces: vec![InterfaceRef {
                    name: interface.into(),
                    index: 14,
                }],
            }],
            origin: oscq_core::Origin::Mdns {
                instance: "a server".into(),
                service_type: OSCQUERY_SERVICE.into(),
            },
            stale: false,
        }
    }

    /// Why loopback is excluded: a process macOS is keeping off the local network still hears
    /// everything announced on loopback. On the human's Mac that is exactly what happened: the
    /// window showed a server of its own machine, registered with `dns-sd`, and was deaf to the
    /// one advertised on the real interface.
    #[test]
    fn an_answer_on_loopback_alone_says_nothing_about_the_network() {
        let loopback = vec!["lo0".to_owned()];
        let found = |s| vec![DiscoveryEvent::Found(s)];
        assert!(!off_loopback(
            &found(server_at("127.0.0.1", "lo0")),
            &loopback
        ));
        assert!(!off_loopback(
            &found(server_at("10.0.0.2", "lo0")),
            &loopback
        ));
        assert!(!off_loopback(
            &found(server_at("127.0.0.1", "en0")),
            &loopback
        ));
        assert!(off_loopback(
            &found(server_at("10.0.0.2", "en0")),
            &loopback
        ));
        // An event about this machine carries no server and answers nothing.
        assert!(!off_loopback(
            &[DiscoveryEvent::LocalNetwork(crate::LocalNetwork::Asked)],
            &loopback
        ));
    }

    #[test]
    fn this_machine_has_a_loopback_interface_to_exclude() {
        // The names are read from the system, so a platform where they cannot be read would
        // silently treat every answer as off loopback. It is read here instead of assumed.
        let names = loopback_names();
        assert!(
            !names.is_empty(),
            "no loopback interface found on this machine, the exclusion cannot evaluate"
        );
    }
}
