// ABOUTME: Terminal User Interface rendering and layout management  
// ABOUTME: Handles all visual components including graphs, tables, and real-time updates

use anyhow;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

use crate::stats::{ConnectionQuality, PingStats};

pub struct TuiState {
    pub selected_tab: usize,
    pub selected_host: usize,
    pub show_help: bool,
    pub paused: bool,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            selected_tab: 0,
            selected_host: 0,
            show_help: false,
            paused: false,
        }
    }
}

pub struct TuiApp {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    state: TuiState,
    host_info: Vec<(String, String)>, // (id, name)
}

impl TuiApp {
    pub async fn new() -> anyhow::Result<Self> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self {
            terminal,
            state: TuiState::default(),
            host_info: Vec::new(),
        })
    }

    pub fn set_host_info(&mut self, host_info: Vec<(String, String)>) {
        self.host_info = host_info;
    }

    pub async fn draw(
        &mut self,
        stats: &HashMap<String, PingStats>,
    ) -> anyhow::Result<()> {
        let host_info = self.host_info.clone();
        let show_help = self.state.show_help;
        
        self.terminal.draw(move |f| {
            if show_help {
                render_help(f);
            } else {
                render_main(f, stats, &host_info);
            }
        })?;
        Ok(())
    }

    pub async fn handle_events(&mut self) -> anyhow::Result<bool> {
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => return Ok(true), // Quit
                    KeyCode::Char('h') | KeyCode::F(1) => {
                        self.state.show_help = !self.state.show_help;
                    }
                    KeyCode::Char(' ') => {
                        self.state.paused = !self.state.paused;
                    }
                    _ => {}
                }
            }
        }
        Ok(false)
    }
}

fn render_main(f: &mut Frame, stats: &HashMap<String, PingStats>, host_info: &[(String, String)]) {
    let size = f.area();

    // Create a simple text display
    let mut text = String::new();
    text.push_str("🏓 Pingpong - Network Monitor\n\n");
    
    for (host_id, host_name) in host_info {
        if let Some(stat) = stats.get(host_id) {
            let quality = stat.connection_quality();
            let rtt_stats = stat.rtt_stats();
            let loss = stat.packet_loss_percent();
            
            text.push_str(&format!(
                "{} {} - RTT: {:.1}ms, Loss: {:.1}%, Pings: {}\n",
                quality.symbol(),
                host_name,
                rtt_stats.avg.as_secs_f64() * 1000.0,
                loss,
                stat.total_pings()
            ));
        } else {
            text.push_str(&format!("● {} - No data yet\n", host_name));
        }
    }
    
    text.push_str("\nPress 'q' to quit, 'h' for help, 'space' to pause");

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, size);
}

fn render_help(f: &mut Frame) {
    let area = f.area();
    
    let help_text = vec![
        "🏓 Pingpong Help",
        "",
        "CONTROLS:",
        "  Space       - Pause/resume pings",
        "  q           - Quit application",
        "  h / F1      - Toggle this help",
        "",
        "INDICATORS:",
        "  ●           - Good connection (< 2% loss, < 100ms)",
        "  ◐           - Fair connection (< 10% loss, < 500ms)", 
        "  ○           - Poor connection (> 10% loss or > 500ms)",
        "",
        "Press 'h' or F1 to close this help",
    ];

    let help_paragraph = Paragraph::new(help_text.join("\n"))
        .block(Block::default().borders(Borders::ALL).title(" Help "))
        .alignment(Alignment::Left);

    f.render_widget(help_paragraph, area);
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}