// ABOUTME: Core ping engine with async execution and DNS resolution
// ABOUTME: Handles ICMP ping operations and maintains connection state per host

use anyhow::{Context, Result};
use dns_lookup::lookup_host;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config as SurgePingConfig, PingIdentifier, PingSequence};
use tokio::sync::mpsc;

use crate::config::Host;
use crate::stats::PingResult;

/// Represents a state change or measurement event emitted by the ping loop for one host.
#[derive(Debug, Clone)]
pub enum HostUpdate {
    Resolving,
    ResolveFailed(String),
    // The app records that resolution succeeded; the resolved IpAddr itself is not read.
    #[allow(dead_code)]
    Resolved(IpAddr),
    Pinged(PingResult),
}

/// Event sent from the ping engine to the app for a single host update.
#[derive(Debug, Clone)]
pub struct PingEvent {
    pub host_id: String,
    // Carried with each event but not read; host rows are labeled from host_info instead.
    #[allow(dead_code)]
    pub host_name: String,
    pub update: HostUpdate,
}

/// Exponential backoff with a cap. `next()` returns the current delay then doubles it.
#[derive(Debug)]
pub struct Backoff {
    current: Duration,
    base: Duration,
    max: Duration,
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            current: base,
            base,
            max,
        }
    }

    pub fn next(&mut self) -> Duration {
        let delay = self.current.min(self.max);
        self.current = (self.current * 2).min(self.max);
        delay
    }

    pub fn reset(&mut self) {
        self.current = self.base;
    }
}

pub struct PingEngine {
    hosts: Vec<Host>,
    event_tx: mpsc::Sender<PingEvent>,
    ping_config: crate::config::PingConfig,
}

impl PingEngine {
    pub fn new(
        hosts: Vec<Host>,
        ping_config: crate::config::PingConfig,
        event_tx: mpsc::Sender<PingEvent>,
    ) -> Self {
        Self {
            hosts,
            event_tx,
            ping_config,
        }
    }

    pub async fn start(&self) -> Result<()> {
        let mut handles = Vec::new();
        for host in &self.hosts {
            if !host.enabled {
                continue;
            }
            let host_clone = host.clone();
            let event_tx = self.event_tx.clone();
            let ping_config = self.ping_config.clone();
            handles.push(tokio::spawn(async move {
                Self::ping_host_loop(host_clone, event_tx, ping_config).await
            }));
        }
        for handle in handles {
            let _ = handle.await; // task panics already restore the terminal via panic hook
        }
        Ok(())
    }

    async fn ping_host_loop(
        host: Host,
        event_tx: mpsc::Sender<PingEvent>,
        ping_config: crate::config::PingConfig,
    ) {
        let host_id = Self::generate_host_id(&host.address);
        let interval = Duration::from_secs_f64(host.interval.unwrap_or(ping_config.interval));
        let timeout = Duration::from_secs_f64(ping_config.timeout);
        let payload = vec![0u8; ping_config.packet_size as usize];

        let send = |update: HostUpdate| {
            let _ = event_tx.try_send(PingEvent {
                host_id: host_id.clone(),
                host_name: host.name.clone(),
                update,
            });
        };

        let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
        let mut sequence = 0u16;

        loop {
            // (Re)resolve with backoff until success.
            send(HostUpdate::Resolving);
            let ip_addr = loop {
                match Self::resolve_hostname(&host.address).await {
                    Ok(ip) => {
                        backoff.reset();
                        break ip;
                    }
                    Err(e) => {
                        send(HostUpdate::ResolveFailed(e.to_string()));
                        tokio::time::sleep(backoff.next()).await;
                    }
                }
            };
            send(HostUpdate::Resolved(ip_addr));

            // Build a client; if sockets are denied even after surge-ping's
            // DGRAM->RAW fallback, report it and back off (don't spin).
            let client = match Client::new(&SurgePingConfig::default()) {
                Ok(c) => c,
                Err(e) => {
                    send(HostUpdate::ResolveFailed(format!(
                        "icmp socket denied ({e}); on Linux set net.ipv4.ping_group_range or run elevated"
                    )));
                    tokio::time::sleep(backoff.next()).await;
                    continue;
                }
            };

            // Ping at the configured interval. After several consecutive failures,
            // break out to re-resolve (handles IP changes / reconnects).
            // Create the pinger ONCE and reuse it — this is surge-ping's intended use
            // and avoids a redundant double-timeout. It enforces `pinger.timeout` itself
            // and returns Err(SurgeError::Timeout) when a reply does not arrive in time.
            let mut pinger = client.pinger(ip_addr, PingIdentifier(0)).await;
            pinger.timeout(timeout);
            let mut interval_timer = tokio::time::interval(interval);
            let mut consecutive_failures = 0u32;
            loop {
                interval_timer.tick().await;
                let start_time = Instant::now();

                let result = match pinger.ping(PingSequence(sequence), &payload).await {
                    Ok((_, rtt)) => {
                        consecutive_failures = 0;
                        PingResult::Success {
                            rtt,
                            sequence,
                            timestamp: start_time,
                        }
                    }
                    Err(surge_ping::SurgeError::Timeout { .. }) => {
                        consecutive_failures += 1;
                        PingResult::Timeout {
                            sequence,
                            timestamp: start_time,
                        }
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        PingResult::Error {
                            error: e.to_string(),
                            sequence,
                            timestamp: start_time,
                        }
                    }
                };

                if event_tx
                    .try_send(PingEvent {
                        host_id: host_id.clone(),
                        host_name: host.name.clone(),
                        update: HostUpdate::Pinged(result),
                    })
                    .is_err()
                {
                    // Channel full is fine (UI drops a frame); channel closed = exit.
                    if event_tx.is_closed() {
                        return;
                    }
                }

                sequence = sequence.wrapping_add(1);
                if consecutive_failures >= 5 {
                    break; // re-resolve
                }
            }
        }
    }

    async fn resolve_hostname(hostname: &str) -> Result<IpAddr> {
        // Try parsing as IP first
        if let Ok(ip) = hostname.parse::<IpAddr>() {
            return Ok(ip);
        }

        // Resolve hostname
        let ips =
            lookup_host(hostname).with_context(|| format!("DNS lookup failed for {}", hostname))?;

        ips.into_iter()
            .next()
            .with_context(|| format!("No IP addresses found for {}", hostname))
    }

    fn generate_host_id(address: &str) -> String {
        // Use a deterministic ID based on address for consistency
        format!(
            "host_{}",
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, address.as_bytes())
        )
    }

    pub fn get_host_info(&self) -> Vec<(String, String)> {
        self.hosts
            .iter()
            .filter(|h| h.enabled)
            .map(|h| (Self::generate_host_id(&h.address), h.name.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PingConfig;
    use tokio::sync::mpsc;

    #[test]
    fn test_ping_engine_creation() {
        let hosts = vec![Host {
            name: "localhost".into(),
            address: "127.0.0.1".into(),
            enabled: true,
            interval: None,
        }];
        let ping_config = PingConfig {
            interval: 1.0,
            timeout: 5.0,
            history_size: 100,
            packet_size: 64,
            portal_check_url: "http://captive.apple.com".to_string(),
        };
        let (tx, _rx) = mpsc::channel(64);
        let _engine = PingEngine::new(hosts, ping_config, tx);
    }

    #[tokio::test]
    async fn test_ip_parse_fast_path() {
        assert!(PingEngine::resolve_hostname("127.0.0.1").await.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires live DNS; run with --ignored"]
    async fn test_hostname_resolution_live() {
        assert!(PingEngine::resolve_hostname("localhost").await.is_ok());
    }

    #[test]
    fn test_host_id_generation() {
        let id1 = PingEngine::generate_host_id("127.0.0.1");
        let id2 = PingEngine::generate_host_id("127.0.0.1");
        let id3 = PingEngine::generate_host_id("8.8.8.8");

        assert_eq!(id1, id2, "Same address should generate same ID");
        assert_ne!(
            id1, id3,
            "Different addresses should generate different IDs"
        );
    }

    #[test]
    fn backoff_doubles_and_caps_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
        assert_eq!(b.next(), Duration::from_secs(1));
        assert_eq!(b.next(), Duration::from_secs(2));
        assert_eq!(b.next(), Duration::from_secs(4));
        assert_eq!(b.next(), Duration::from_secs(8));
        assert_eq!(b.next(), Duration::from_secs(16));
        assert_eq!(b.next(), Duration::from_secs(30)); // capped (would be 32)
        assert_eq!(b.next(), Duration::from_secs(30)); // stays capped
        b.reset();
        assert_eq!(b.next(), Duration::from_secs(1));
    }
}
