// ABOUTME: Configuration management for pingpong utility
// ABOUTME: Handles loading/saving TOML config files and managing host lists

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub ping: PingConfig,
    pub hosts: Vec<Host>,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingConfig {
    /// Ping interval in seconds
    pub interval: f64,
    /// Timeout for each ping in seconds
    pub timeout: f64,
    /// Number of ping history entries to keep
    pub history_size: usize,
    /// Packet size in bytes
    pub packet_size: u16,
    /// URL used to detect captive portals (plain HTTP; default Apple's endpoint).
    #[serde(default = "default_portal_url")]
    pub portal_check_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    /// Display name for the host
    pub name: String,
    /// Hostname or IP address
    pub address: String,
    /// Whether this host is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Custom ping interval for this host (overrides global)
    pub interval: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Refresh rate for the UI in milliseconds
    pub refresh_rate: u64,
    /// Color theme (dark, light, auto)
    pub theme: String,
    /// Show detailed stats by default
    #[serde(default = "default_true")]
    pub show_details: bool,
    /// Graph height in terminal rows
    pub graph_height: u16,
}

fn default_true() -> bool {
    true
}

fn default_portal_url() -> String {
    "http://captive.apple.com".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ping: PingConfig {
                interval: 1.0,
                timeout: 3.0,
                history_size: 300, // 5 minutes at 1s intervals
                packet_size: 32,
                portal_check_url: default_portal_url(),
            },
            hosts: vec![
                Host {
                    name: "Google DNS".to_string(),
                    address: "8.8.8.8".to_string(),
                    enabled: true,
                    interval: None,
                },
                Host {
                    name: "Cloudflare DNS".to_string(),
                    address: "1.1.1.1".to_string(),
                    enabled: true,
                    interval: None,
                },
                Host {
                    name: "Google".to_string(),
                    address: "google.com".to_string(),
                    enabled: true,
                    interval: None,
                },
            ],
            ui: UiConfig {
                refresh_rate: 100, // 10 FPS
                theme: "auto".to_string(),
                show_details: true,
                graph_height: 10,
            },
        }
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.as_ref().display()))
    }

    #[allow(dead_code)]
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {}", path.as_ref().display()))
    }

    pub fn add_host(&mut self, address: String) {
        use std::net::IpAddr;
        let name = match address.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) => format!("IP {address}"),
            _ => address.clone(), // hostname or IPv6 -> use as-is
        };
        self.hosts.push(Host {
            name,
            address,
            enabled: true,
            interval: None,
        });
    }

    /// Clamp nonsensical values so a hand-edited config can't wedge the app.
    pub fn validate(&mut self) {
        if !self.ping.interval.is_finite() || self.ping.interval < 0.1 {
            self.ping.interval = 1.0;
        }
        if !self.ping.timeout.is_finite() || self.ping.timeout < 0.1 {
            self.ping.timeout = 3.0;
        }
        if self.ping.history_size == 0 {
            self.ping.history_size = 300;
        }
        if self.ping.packet_size == 0 {
            self.ping.packet_size = 32;
        }
        if self.ui.refresh_rate == 0 {
            self.ui.refresh_rate = 100;
        }
        // Per-host interval overrides feed Duration::from_secs_f64 in the ping loop;
        // drop any that are non-finite or too small so they fall back to the global interval.
        for host in &mut self.hosts {
            if host.interval.is_some_and(|i| !i.is_finite() || i < 0.1) {
                host.interval = None;
            }
        }
        // Keep the graph height within a sane range of terminal rows.
        if self.ui.graph_height == 0 || self.ui.graph_height > 50 {
            self.ui.graph_height = 10;
        }
    }

    pub fn set_interval(&mut self, interval: f64) {
        self.ping.interval = interval;
    }

    pub fn enabled_hosts(&self) -> impl Iterator<Item = &Host> {
        self.hosts.iter().filter(|h| h.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_hosts() {
        let c = Config::default();
        assert!(!c.hosts.is_empty());
        assert_eq!(c.ping.portal_check_url, "http://captive.apple.com");
    }

    #[test]
    fn add_host_names_ipv4() {
        let mut c = Config {
            ping: Config::default().ping,
            hosts: vec![],
            ui: Config::default().ui,
        };
        c.add_host("8.8.8.8".to_string());
        assert_eq!(c.hosts[0].name, "IP 8.8.8.8");
    }

    #[test]
    fn add_host_keeps_hostname_and_ipv6() {
        let mut c = Config {
            ping: Config::default().ping,
            hosts: vec![],
            ui: Config::default().ui,
        };
        c.add_host("example.com".to_string());
        c.add_host("2606:4700:4700::1111".to_string());
        assert_eq!(c.hosts[0].name, "example.com");
        // IPv6 must NOT be misclassified/renamed oddly; name == address is fine.
        assert_eq!(c.hosts[1].name, "2606:4700:4700::1111");
    }

    #[test]
    fn validate_clamps_absurd_values() {
        let mut c = Config::default();
        c.ping.interval = 0.0;
        c.ping.timeout = 0.0;
        c.ping.history_size = 0;
        c.ping.packet_size = 0;
        c.ui.refresh_rate = 0;
        c.validate();
        assert!(c.ping.interval >= 0.1);
        assert!(c.ping.timeout >= 0.1);
        assert!(c.ping.history_size >= 1);
        assert!(c.ping.packet_size >= 1);
        assert!(c.ui.refresh_rate >= 1);
    }

    #[test]
    fn validate_rejects_non_finite_timeouts() {
        // NaN and infinity would panic Duration::from_secs_f64 in the ping loop;
        // validate must replace them with finite defaults.
        let mut c = Config::default();
        c.ping.interval = f64::INFINITY;
        c.ping.timeout = f64::NAN;
        c.validate();
        assert!(c.ping.interval.is_finite() && c.ping.interval >= 0.1);
        assert!(c.ping.timeout.is_finite() && c.ping.timeout >= 0.1);
    }

    #[test]
    fn validate_drops_bad_per_host_intervals() {
        let mut c = Config {
            hosts: vec![
                Host {
                    name: "nan".into(),
                    address: "1.1.1.1".into(),
                    enabled: true,
                    interval: Some(f64::NAN),
                },
                Host {
                    name: "inf".into(),
                    address: "8.8.8.8".into(),
                    enabled: true,
                    interval: Some(f64::INFINITY),
                },
                Host {
                    name: "tiny".into(),
                    address: "9.9.9.9".into(),
                    enabled: true,
                    interval: Some(0.0),
                },
                Host {
                    name: "ok".into(),
                    address: "google.com".into(),
                    enabled: true,
                    interval: Some(2.0),
                },
                Host {
                    name: "none".into(),
                    address: "1.0.0.1".into(),
                    enabled: true,
                    interval: None,
                },
            ],
            ..Config::default()
        };
        c.validate();
        assert_eq!(
            c.hosts[0].interval, None,
            "NaN per-host interval must be dropped"
        );
        assert_eq!(
            c.hosts[1].interval, None,
            "infinite per-host interval must be dropped"
        );
        assert_eq!(
            c.hosts[2].interval, None,
            "too-small per-host interval must be dropped"
        );
        assert_eq!(
            c.hosts[3].interval,
            Some(2.0),
            "valid per-host interval must be kept"
        );
        assert_eq!(c.hosts[4].interval, None, "absent interval stays absent");
    }

    #[test]
    fn validate_clamps_graph_height() {
        let mut c = Config::default();
        c.ui.graph_height = 0;
        c.validate();
        assert_eq!(c.ui.graph_height, 10, "zero graph height clamps to default");

        c.ui.graph_height = 9999;
        c.validate();
        assert_eq!(
            c.ui.graph_height, 10,
            "absurd graph height clamps to default"
        );

        c.ui.graph_height = 20;
        c.validate();
        assert_eq!(c.ui.graph_height, 20, "in-range graph height is preserved");
    }
}
