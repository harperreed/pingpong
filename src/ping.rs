// ABOUTME: Core ping engine with async execution and DNS resolution
// ABOUTME: Handles ICMP ping operations and maintains connection state per host

use anyhow::{Context, Result};
use dns_lookup::lookup_host;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config as SurgePingConfig, PingIdentifier, PingSequence};
use tokio::sync::{mpsc, RwLock};
use tokio::time;

use crate::config::Host;
use crate::stats::{PingResult, PingStats};


#[derive(Debug, Clone)]
pub struct PingEvent {
    pub host_id: String,
    pub host_name: String,
    pub result: PingResult,
}

pub struct PingEngine {
    hosts: Vec<Host>,
    clients: HashMap<String, Arc<Client>>,
    stats: Arc<RwLock<HashMap<String, PingStats>>>,
    event_tx: mpsc::UnboundedSender<PingEvent>,
    ping_config: crate::config::PingConfig,
}

impl PingEngine {
    pub async fn new(
        hosts: Vec<Host>,
        ping_config: crate::config::PingConfig,
        event_tx: mpsc::UnboundedSender<PingEvent>,
    ) -> Result<Self> {
        let mut clients = HashMap::new();
        let mut stats = HashMap::new();

        // Create ping clients and resolve hosts
        for host in &hosts {
            let host_id = Self::generate_host_id(&host.address);
            
            // Resolve hostname if needed (we don't need to store the IP here as we resolve it again in the ping loop)
            let _ip_addr = if let Ok(ip) = host.address.parse::<IpAddr>() {
                ip
            } else {
                Self::resolve_hostname(&host.address).await
                    .with_context(|| format!("Failed to resolve hostname: {}", host.address))?
            };

            // Create ping client
            let config = SurgePingConfig::default();
            let client = Client::new(&config)?;
            
            clients.insert(host_id.clone(), Arc::new(client));
            stats.insert(host_id, PingStats::new(ping_config.history_size));
        }

        Ok(Self {
            hosts,
            clients,
            stats: Arc::new(RwLock::new(stats)),
            event_tx,
            ping_config,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let mut handles = Vec::new();

        for host in &self.hosts {
            if !host.enabled {
                continue;
            }

            let host_id = Self::generate_host_id(&host.address);
            let host_clone = host.clone();
            let client = self.clients.get(&host_id).unwrap().clone();
            let stats = self.stats.clone();
            let event_tx = self.event_tx.clone();
            let ping_config = self.ping_config.clone();

            let handle = tokio::spawn(async move {
                Self::ping_host_loop(host_clone, client, stats, event_tx, ping_config).await
            });

            handles.push(handle);
        }

        // Wait for all ping tasks to complete (they run indefinitely)
        for handle in handles {
            if let Err(e) = handle.await {
                eprintln!("Ping task failed: {}", e);
            }
        }

        Ok(())
    }

    async fn ping_host_loop(
        host: Host,
        client: Arc<Client>,
        stats: Arc<RwLock<HashMap<String, PingStats>>>,
        event_tx: mpsc::UnboundedSender<PingEvent>,
        ping_config: crate::config::PingConfig,
    ) {
        let host_id = Self::generate_host_id(&host.address);
        let interval = Duration::from_secs_f64(host.interval.unwrap_or(ping_config.interval));
        let timeout = Duration::from_secs_f64(ping_config.timeout);
        
        // Resolve IP address
        let ip_addr = match Self::resolve_hostname(&host.address).await {
            Ok(ip) => ip,
            Err(e) => {
                eprintln!("Failed to resolve {}: {}", host.address, e);
                return;
            }
        };

        let mut sequence = 0u16;
        let mut interval_timer = time::interval(interval);

        loop {
            interval_timer.tick().await;

            let start_time = Instant::now();
            let identifier = PingIdentifier(0);
            let seq_cnt = PingSequence(sequence);

            let result = {
                let mut pinger = client.pinger(ip_addr, identifier).await;
                pinger.timeout(timeout);
                
                match tokio::time::timeout(
                    timeout,
                    pinger.ping(seq_cnt, &[])
                ).await {
                    Ok(Ok((_, duration))) => PingResult::Success {
                        rtt: duration,
                        sequence,
                        timestamp: start_time,
                    },
                    Ok(Err(e)) => PingResult::Error {
                        error: e.to_string(),
                        sequence,
                        timestamp: start_time,
                    },
                    Err(_) => PingResult::Timeout {
                        sequence,
                        timestamp: start_time,
                    },
                }
            };

            // Update stats
            {
                let mut stats_guard = stats.write().await;
                if let Some(host_stats) = stats_guard.get_mut(&host_id) {
                    host_stats.add_result(&result);
                }
            }

            // Send event
            let event = PingEvent {
                host_id: host_id.clone(),
                host_name: host.name.clone(),
                result: result.clone(),
            };

            if event_tx.send(event).is_err() {
                // Receiver dropped, exit
                break;
            }

            sequence = sequence.wrapping_add(1);
        }
    }

    async fn resolve_hostname(hostname: &str) -> Result<IpAddr> {
        // Try parsing as IP first
        if let Ok(ip) = hostname.parse::<IpAddr>() {
            return Ok(ip);
        }

        // Resolve hostname
        let ips = lookup_host(hostname)
            .with_context(|| format!("DNS lookup failed for {}", hostname))?;
        
        ips.into_iter()
            .next()
            .with_context(|| format!("No IP addresses found for {}", hostname))
    }

    fn generate_host_id(address: &str) -> String {
        // Use a deterministic ID based on address for consistency
        format!("host_{}", uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, address.as_bytes()))
    }

    pub async fn get_stats(&self) -> HashMap<String, PingStats> {
        self.stats.read().await.clone()
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

    #[tokio::test]
    async fn test_ping_engine_creation() {
        let hosts = vec![
            Host {
                name: "localhost".to_string(),
                address: "127.0.0.1".to_string(),
                enabled: true,
                interval: None,
            }
        ];
        
        let ping_config = PingConfig {
            interval: 1.0,
            timeout: 5.0,
            history_size: 100,
        };
        
        let (tx, _rx) = mpsc::unbounded_channel();
        
        let result = PingEngine::new(hosts, ping_config, tx).await;
        assert!(result.is_ok(), "PingEngine creation should succeed");
    }

    #[tokio::test]
    async fn test_hostname_resolution() {
        let result = PingEngine::resolve_hostname("127.0.0.1").await;
        assert!(result.is_ok(), "Should resolve localhost IP");
        
        let result = PingEngine::resolve_hostname("localhost").await;
        assert!(result.is_ok(), "Should resolve localhost hostname");
    }

    #[test]
    fn test_host_id_generation() {
        let id1 = PingEngine::generate_host_id("127.0.0.1");
        let id2 = PingEngine::generate_host_id("127.0.0.1");
        let id3 = PingEngine::generate_host_id("8.8.8.8");
        
        assert_eq!(id1, id2, "Same address should generate same ID");
        assert_ne!(id1, id3, "Different addresses should generate different IDs");
    }
}