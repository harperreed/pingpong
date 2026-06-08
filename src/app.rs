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
use crate::status::{self, ConnectivityState, HostState};
use crate::tui::{AnimationType, TuiApp};

pub struct App {
    config: Config,
    tui: TuiApp,
    stats: HashMap<String, PingStats>,
    // Per-host DNS resolution state, keyed by host id; read by the connectivity status renderer.
    resolved: HashMap<String, bool>,
    // Per-host last resolution error (None once resolved), keyed by host id; read by the error banner.
    resolve_err: HashMap<String, Option<String>>,
    // Latest captive-portal/connectivity classification from the probe; read by the connectivity banner.
    portal: ProbeResult,
    event_rx: mpsc::Receiver<PingEvent>,
    probe_rx: mpsc::Receiver<ProbeResult>,
    // (host_id, display name) pairs identifying each monitored host; used to label host rows.
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
        tui.set_ui_config(
            config.ui.theme.clone(),
            config.ui.show_details,
            config.ui.graph_height,
        );

        // Start ping engine in background
        tokio::spawn(async move {
            let _ = ping_engine.start().await;
        });

        // Start captive-portal probe loop in background
        let (probe_tx, probe_rx) = mpsc::channel::<ProbeResult>(8);
        let portal_url = config.ping.portal_check_url.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                let r = crate::probe::probe_once(&portal_url).await;
                if probe_tx.send(r).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            config,
            tui,
            stats: HashMap::new(),
            resolved: HashMap::new(),
            resolve_err: HashMap::new(),
            portal: ProbeResult::Offline,
            event_rx,
            probe_rx,
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
                        self.handle_ping_event(ping_event);
                    }
                }

                // Store the latest captive-portal probe result
                Some(p) = self.probe_rx.recv() => { self.portal = p; }

                // Update UI
                // Errors propagate out of run; App's Drop restores the terminal before main prints them.
                _ = ui_update_interval.tick() => {
                    let host_states: Vec<(String, HostState)> = self
                        .host_info
                        .iter()
                        .map(|(id, _)| {
                            let resolved = *self.resolved.get(id).unwrap_or(&false);
                            let err = self.resolve_err.get(id).and_then(|o| o.as_deref());
                            (id.clone(), status::host_state(self.stats.get(id), resolved, err))
                        })
                        .collect();
                    let states: Vec<HostState> = host_states.iter().map(|(_, s)| s.clone()).collect();
                    let conn = status::connectivity(&states, &self.portal);
                    let agg = status::aggregate(&states);
                    self.tui.set_title(&status::title(&conn, &agg));
                    let banner = match &conn {
                        ConnectivityState::CaptivePortal { url } => {
                            Some(format!("\u{26a0}  Captive portal detected \u{2014} open {url}"))
                        }
                        ConnectivityState::Offline => {
                            Some("\u{2717}  Offline \u{2014} no connectivity".to_string())
                        }
                        _ => None,
                    };
                    let opts = crate::tui::RenderOpts {
                        theme: crate::tui::Theme::from_name(self.tui.theme_name()),
                        show_details: self.tui.show_details(),
                        graph_height: self.config.ui.graph_height,
                        banner,
                        host_states,
                    };
                    self.tui.draw(&self.stats, &opts).await?;
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

    fn handle_ping_event(&mut self, event: PingEvent) {
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
