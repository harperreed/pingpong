// ABOUTME: Main application orchestrator that coordinates ping engine and TUI
// ABOUTME: Manages the event loop between ping results and UI updates

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time;

use crate::config::Config;
use crate::ping::{PingEngine, PingEvent};
use crate::stats::PingStats;
use crate::tui::{TuiApp, AnimationType};

pub struct App {
    config: Config,
    tui: TuiApp,
    stats: Arc<RwLock<HashMap<String, PingStats>>>,
    event_rx: mpsc::UnboundedReceiver<PingEvent>,
    host_info: Vec<(String, String)>,
}

impl App {
    pub async fn new(config: Config, animation_type: Option<AnimationType>) -> Result<Self> {
        // Create event channel
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Get enabled hosts
        let hosts: Vec<_> = config.enabled_hosts().cloned().collect();

        // Initialize ping engine
        let ping_engine = PingEngine::new(hosts, config.ping.clone(), event_tx).await?;

        // Get host info before moving ping_engine
        let host_info = ping_engine.get_host_info();

        // Initialize TUI
        let mut tui = TuiApp::new(animation_type).await?;
        tui.set_host_info(host_info.clone());

        // Initialize stats
        let stats = Arc::new(RwLock::new(HashMap::new()));

        // Start ping engine in background
        tokio::spawn(async move {
            if let Err(e) = ping_engine.start().await {
                eprintln!("Ping engine error: {}", e);
            }
        });

        Ok(Self {
            config,
            tui,
            stats,
            event_rx,
            host_info,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        // Main event loop
        let mut ui_update_interval = time::interval(Duration::from_millis(self.config.ui.refresh_rate));
        
        loop {
            tokio::select! {
                // Handle ping events
                event = self.event_rx.recv() => {
                    if let Some(ping_event) = event {
                        self.handle_ping_event(ping_event).await;
                    }
                }
                
                // Update UI
                _ = ui_update_interval.tick() => {
                    let stats = self.stats.read().await;
                    if let Err(e) = self.tui.draw(&*stats).await {
                        eprintln!("TUI error: {}", e);
                        break;
                    }
                    
                    // Handle user input
                    if let Ok(should_quit) = self.tui.handle_events().await {
                        if should_quit {
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    async fn handle_ping_event(&mut self, event: PingEvent) {
        // Update stats
        let mut stats = self.stats.write().await;
        let host_stats = stats
            .entry(event.host_id.clone())
            .or_insert_with(|| PingStats::new(self.config.ping.history_size));
        
        host_stats.add_result(&event.result);
    }
}