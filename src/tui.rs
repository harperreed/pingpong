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
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use crate::stats::PingStats;

pub struct TuiState {
    pub selected_tab: usize,
    pub selected_host: usize,
    pub show_help: bool,
    pub paused: bool,
    pub dna_animation_frame: usize,
    pub last_frame_time: Instant,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            selected_tab: 0,
            selected_host: 0,
            show_help: false,
            paused: false,
            dna_animation_frame: 0,
            last_frame_time: Instant::now(),
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
        
        // Update DNA animation frame based on ping performance
        let avg_rtt = calculate_average_rtt(stats);
        let animation_speed = calculate_animation_speed(avg_rtt);
        
        let now = Instant::now();
        if now.duration_since(self.state.last_frame_time).as_millis() > animation_speed as u128 {
            self.state.dna_animation_frame = (self.state.dna_animation_frame + 1) % 8;
            self.state.last_frame_time = now;
        }
        
        let dna_frame = self.state.dna_animation_frame;
        
        self.terminal.draw(move |f| {
            if show_help {
                render_help(f);
            } else {
                render_main(f, stats, &host_info, dna_frame, avg_rtt);
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

fn render_main(f: &mut Frame, stats: &HashMap<String, PingStats>, host_info: &[(String, String)], dna_frame: usize, avg_rtt: f64) {
    let size = f.area();

    // Create 4-window layout: left side split top/bottom, right side single window
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(size);

    // Calculate dynamic split based on number of hosts (more hosts = more space for pings)
    let host_count = host_info.len();
    let ping_percentage = std::cmp::min(80, 40 + (host_count * 8)); // 40% base + 8% per host, max 80%
    let lore_percentage = 100 - ping_percentage;
    
    // Split left side into top (pings) and bottom (lore)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(ping_percentage as u16), 
            Constraint::Percentage(lore_percentage as u16)
        ])
        .split(main_chunks[0]);

    // Render pings window (top left)
    render_pings_window(f, left_chunks[0], stats, host_info);
    
    // Render lore window (bottom left)
    render_lore_window(f, left_chunks[1]);
    
    // Render plasma animation (right side)
    render_plasma_window(f, main_chunks[1], dna_frame, avg_rtt);
}

fn render_pings_window(f: &mut Frame, area: Rect, stats: &HashMap<String, PingStats>, host_info: &[(String, String)]) {
    let mut text = String::new();
    text.push_str("🏓 Network Monitor\n");
    text.push_str("═══════════════════════════════════════════════\n\n");
    
    for (i, (host_id, host_name)) in host_info.iter().enumerate() {
        if let Some(stat) = stats.get(host_id) {
            let quality = stat.connection_quality();
            let rtt_stats = stat.rtt_stats();
            let loss = stat.packet_loss_percent();
            
            text.push_str(&format!(
                "{} {} {}\n",
                quality.symbol(),
                host_name,
                "─".repeat(35 - host_name.len().min(25))
            ));
            text.push_str(&format!(
                "   RTT: {:.1}ms (avg) | Loss: {:.1}% | Pings: {}\n",
                rtt_stats.avg.as_secs_f64() * 1000.0,
                loss,
                stat.total_pings()
            ));
            
            // Add status indicator bar
            let status_bar = if loss < 1.0 && rtt_stats.avg.as_millis() < 100 {
                "   Status: ████████████ EXCELLENT"
            } else if loss < 5.0 && rtt_stats.avg.as_millis() < 200 {
                "   Status: ████████▓▓▓▓ GOOD"
            } else if loss < 10.0 && rtt_stats.avg.as_millis() < 500 {
                "   Status: ██████▓▓▓▓▓▓ FAIR"
            } else {
                "   Status: ████▓▓▓▓▓▓▓▓ POOR"
            };
            text.push_str(&format!("{}\n", status_bar));
            
        } else {
            text.push_str(&format!(
                "● {} {}\n",
                host_name,
                "─".repeat(35 - host_name.len().min(25))
            ));
            text.push_str("   Status: ░░░░░░░░░░░░ WAITING\n");
        }
        
        // Add separator line between hosts (except last one)
        if i < host_info.len() - 1 {
            text.push_str("───────────────────────────────────────────────\n");
        }
        text.push_str("\n");
    }
    
    text.push_str("Controls: 'q' quit | 'h' help | 'space' pause");

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" Network Status "))
        .style(Style::default().fg(Color::Green))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

fn render_lore_window(f: &mut Frame, area: Rect) {
    let lore_text = vec![
        "⚡ Plasma Field Energy",
        "",
        "Digital energy flows through the",
        "network like plasma through space.",
        "Each packet creates ripples in the",
        "electromagnetic field of data.",
        "",
        "Fast connections create intense,",
        "rapidly shifting plasma patterns.",
        "Slow connections show gentle,",
        "slowly undulating energy waves.",
        "",
        "The plasma field reveals the true",
        "nature of your network's soul...",
    ];

    let paragraph = Paragraph::new(lore_text.join("\n"))
        .block(Block::default().borders(Borders::ALL).title(" Network Lore "))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

fn render_plasma_window(f: &mut Frame, area: Rect, frame: usize, avg_rtt: f64) {
    let plasma_art = generate_plasma_animation(frame, area.width as usize, area.height as usize);
    
    let title = format!(" Plasma Field - RTT: {:.1}ms ", avg_rtt);
    let color = if avg_rtt < 50.0 {
        Color::Green
    } else if avg_rtt < 150.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let paragraph = Paragraph::new(plasma_art)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(color))
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

fn calculate_average_rtt(stats: &HashMap<String, PingStats>) -> f64 {
    if stats.is_empty() {
        return 100.0; // Default moderate RTT
    }
    
    let mut total_rtt = 0.0;
    let mut count = 0;
    
    for stat in stats.values() {
        if stat.total_pings() > 0 {
            total_rtt += stat.rtt_stats().avg.as_secs_f64() * 1000.0;
            count += 1;
        }
    }
    
    if count > 0 {
        total_rtt / count as f64
    } else {
        100.0
    }
}

fn calculate_animation_speed(avg_rtt: f64) -> u64 {
    // Fast networks (< 50ms) spin fast (100ms per frame)
    // Medium networks (50-150ms) spin medium (300ms per frame)  
    // Slow networks (> 150ms) spin slow (800ms per frame)
    if avg_rtt < 50.0 {
        100
    } else if avg_rtt < 150.0 {
        300
    } else {
        800
    }
}

fn generate_plasma_animation(frame: usize, width: usize, height: usize) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 }; // Account for borders
    let effective_height = if height > 6 { height - 6 } else { 12 }; // Account for borders and title
    
    // Plasma intensity characters from low to high
    let intensity_chars = [' ', '░', '▒', '▓', '█', '▓', '▒', '░'];
    let energy_chars = ['·', '•', '○', '●', '◉', '⚫', '⬟', '⬢'];
    
    // Create plasma field using sine waves
    for y in 0..effective_height {
        let mut line = String::new();
        for x in 0..effective_width {
            // Create interference pattern with multiple sine waves
            let time = frame as f64 * 0.3;
            let wave1 = ((x as f64 * 0.2 + time).sin() * 2.0) as i32;
            let wave2 = ((y as f64 * 0.3 + time * 1.2).sin() * 2.0) as i32;
            let wave3 = (((x + y) as f64 * 0.15 + time * 0.8).sin() * 1.5) as i32;
            
            // Combine waves to get intensity
            let intensity = (wave1 + wave2 + wave3 + 6) / 2; // Normalize to 0-6 range
            let intensity = intensity.max(0).min(7) as usize;
            
            // Use different character sets for variation
            let use_energy_chars = (x + y + frame) % 3 == 0;
            let char_to_use = if use_energy_chars {
                energy_chars[intensity]
            } else {
                intensity_chars[intensity]
            };
            
            line.push(char_to_use);
        }
        result.push(line);
    }
    
    // Add center decoration
    if effective_height > 8 {
        let center_y = effective_height / 2;
        if center_y < result.len() {
            let center_line = &mut result[center_y];
            if effective_width > 12 {
                let center_x = effective_width / 2;
                let decoration = match frame % 8 {
                    0 => "⚡",
                    1 => "⚡",
                    2 => "✦",
                    3 => "✧",
                    4 => "✦",
                    5 => "✧",
                    6 => "⚡",
                    7 => "⚡",
                    _ => "⚡",
                };
                // Replace center character with decoration
                let mut chars: Vec<char> = center_line.chars().collect();
                if center_x < chars.len() {
                    chars[center_x] = decoration.chars().next().unwrap_or('⚡');
                }
                *center_line = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
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