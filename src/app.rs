// ABOUTME: Main application orchestrator that coordinates ping engine and TUI
// ABOUTME: Manages the event loop between ping results and UI updates

use anyhow::Result;
use std::collections::HashMap;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time;

use crate::config::Config;
use crate::ping::{HostUpdate, PingEngine, PingEvent};
use crate::probe::ProbeResult;
use crate::stats::PingStats;
use crate::tui::{AnimationType, TuiApp};

pub struct App {
    config: Config,
    tui: TuiApp,
    stats: HashMap<String, PingStats>,
    // Per-host resolution flag; read by the connectivity status renderer once wired in.
    #[allow(dead_code)]
    resolved: HashMap<String, bool>,
    // Per-host last resolution error; read by the renderer once the error banner is wired in.
    #[allow(dead_code)]
    resolve_err: HashMap<String, Option<String>>,
    // Network connectivity classification; read by the renderer once the banner is wired in.
    #[allow(dead_code)]
    portal: ProbeResult,
    event_rx: mpsc::Receiver<PingEvent>,
    // Host name/id pairs exposed to the TUI title row; read by the renderer.
    #[allow(dead_code)]
    host_info: Vec<(String, String)>,
}

impl App {
    pub async fn new(config: Config, animation_type: Option<AnimationType>) -> Result<Self> {
        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(1024);

        // Get enabled hosts
        let hosts: Vec<_> = config.enabled_hosts().cloned().collect();

        // Initialize ping engine (synchronous, no DNS at construction time)
        let ping_engine = PingEngine::new(hosts, config.ping.clone(), event_tx);

        // Get host info before moving ping_engine
        let host_info = ping_engine.get_host_info();

        // Initialize TUI
        let mut tui = TuiApp::new(animation_type).await?;
        tui.set_host_info(host_info.clone());

        // Start ping engine in background
        tokio::spawn(async move {
            let _ = ping_engine.start().await;
        });

        Ok(Self {
            config,
            tui,
            stats: HashMap::new(),
            resolved: HashMap::new(),
            resolve_err: HashMap::new(),
            portal: ProbeResult::Offline,
            event_rx,
            host_info,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        // Main event loop
        let mut ui_update_interval =
            time::interval(Duration::from_millis(self.config.ui.refresh_rate));

        loop {
            tokio::select! {
                // Handle ping events
                event = self.event_rx.recv() => {
                    if let Some(ping_event) = event {
                        self.handle_ping_event(ping_event).await;
                    }
                }

                // Update UI
                // Errors propagate out of run; App's Drop restores the terminal before main prints them.
                _ = ui_update_interval.tick() => {
                    self.tui.draw(&self.stats).await?;
                    if self.tui.handle_events().await? { break; }
                }

                // Ctrl-C signal path for pre-/non-raw-mode window
                _ = signal::ctrl_c() => {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_ping_event(&mut self, event: PingEvent) {
        match event.update {
            HostUpdate::Resolving => {
                self.resolved.insert(event.host_id.clone(), false);
            }
            HostUpdate::ResolveFailed(e) => {
                self.resolved.insert(event.host_id.clone(), false);
                self.resolve_err.insert(event.host_id.clone(), Some(e));
            }
            HostUpdate::Resolved(_) => {
                self.resolved.insert(event.host_id.clone(), true);
                self.resolve_err.insert(event.host_id.clone(), None);
            }
            HostUpdate::Pinged(result) => {
                let entry = self
                    .stats
                    .entry(event.host_id.clone())
                    .or_insert_with(|| PingStats::new(self.config.ping.history_size));
                entry.add_result(&result);
            }
        }
    }
}
