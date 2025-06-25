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

impl Default for Config {
    fn default() -> Self {
        Self {
            ping: PingConfig {
                interval: 1.0,
                timeout: 3.0,
                history_size: 300, // 5 minutes at 1s intervals
                packet_size: 32,
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
        let name = if address.chars().all(|c| c.is_ascii_digit() || c == '.') {
            format!("IP {}", address)
        } else {
            address.clone()
        };

        self.hosts.push(Host {
            name,
            address,
            enabled: true,
            interval: None,
        });
    }

    pub fn set_interval(&mut self, interval: f64) {
        self.ping.interval = interval;
    }

    pub fn enabled_hosts(&self) -> impl Iterator<Item = &Host> {
        self.hosts.iter().filter(|h| h.enabled)
    }
}
