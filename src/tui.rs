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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationType {
    Plasma,
    Globe,
    BouncingLogo,
}

impl AnimationType {
    pub fn random() -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;
        
        let mut hasher = DefaultHasher::new();
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos().hash(&mut hasher);
        
        match hasher.finish() % 3 {
            0 => AnimationType::Plasma,
            1 => AnimationType::Globe,
            _ => AnimationType::BouncingLogo,
        }
    }
}

pub struct TuiState {
    pub selected_tab: usize,
    pub selected_host: usize,
    pub show_help: bool,
    pub paused: bool,
    pub animation_frame: usize,
    pub last_frame_time: Instant,
    pub animation_type: AnimationType,
    pub bounce_x: f64,
    pub bounce_y: f64,
    pub bounce_dx: f64,
    pub bounce_dy: f64,
}

impl Default for TuiState {
    fn default() -> Self {
        // Initialize bouncing logo position and velocity
        let animation_type = AnimationType::random();
        let (bounce_dx, bounce_dy) = match animation_type {
            AnimationType::BouncingLogo => (1.5, 1.2), // Initial velocity
            _ => (0.0, 0.0),
        };
        
        Self {
            selected_tab: 0,
            selected_host: 0,
            show_help: false,
            paused: false,
            animation_frame: 0,
            last_frame_time: Instant::now(),
            animation_type,
            bounce_x: 20.0, // Starting position
            bounce_y: 8.0,
            bounce_dx,
            bounce_dy,
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
        
        // Update animation frame based on ping performance
        let avg_rtt = calculate_average_rtt(stats);
        let animation_speed = calculate_animation_speed(avg_rtt);
        
        let now = Instant::now();
        if now.duration_since(self.state.last_frame_time).as_millis() > animation_speed as u128 {
            self.state.animation_frame = (self.state.animation_frame + 1) % 8;
            self.state.last_frame_time = now;
            
            // Update bouncing logo position if that's the current animation
            if self.state.animation_type == AnimationType::BouncingLogo {
                self.update_bounce_position();
            }
        }
        
        let animation_frame = self.state.animation_frame;
        let animation_type = self.state.animation_type;
        let bounce_pos = (self.state.bounce_x, self.state.bounce_y);
        
        self.terminal.draw(move |f| {
            if show_help {
                render_help(f);
            } else {
                render_main(f, stats, &host_info, animation_frame, avg_rtt, animation_type, bounce_pos);
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
    
    fn update_bounce_position(&mut self) {
        // Assume a typical terminal window size for bounds
        let width = 80.0;
        let height = 24.0;
        
        // Update position
        self.state.bounce_x += self.state.bounce_dx;
        self.state.bounce_y += self.state.bounce_dy;
        
        // Bounce off walls
        if self.state.bounce_x <= 0.0 || self.state.bounce_x >= width - 10.0 {
            self.state.bounce_dx = -self.state.bounce_dx;
        }
        if self.state.bounce_y <= 0.0 || self.state.bounce_y >= height - 5.0 {
            self.state.bounce_dy = -self.state.bounce_dy;
        }
        
        // Keep within bounds
        self.state.bounce_x = self.state.bounce_x.clamp(0.0, width - 10.0);
        self.state.bounce_y = self.state.bounce_y.clamp(0.0, height - 5.0);
    }
}

fn render_main(f: &mut Frame, stats: &HashMap<String, PingStats>, host_info: &[(String, String)], animation_frame: usize, avg_rtt: f64, animation_type: AnimationType, bounce_pos: (f64, f64)) {
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
    render_lore_window(f, left_chunks[1], animation_type);
    
    // Render animation (right side)
    render_animation_window(f, main_chunks[1], animation_frame, avg_rtt, animation_type, bounce_pos);
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

fn render_lore_window(f: &mut Frame, area: Rect, animation_type: AnimationType) {
    let lore_text = match animation_type {
        AnimationType::Plasma => vec![
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
        ],
        AnimationType::Globe => vec![
            "🌍 Digital Earth Network",
            "",
            "Your data travels the globe,",
            "spinning through fiber optic cables",
            "and satellite beams that connect",
            "every corner of our planet.",
            "",
            "Each ping is a digital heartbeat,",
            "pulsing across continents and",
            "through the ocean depths where",
            "undersea cables carry the world's",
            "conversations.",
            "",
            "The Earth spins, and so does",
            "your connection to the world...",
        ],
        AnimationType::BouncingLogo => vec![
            "📺 Retro Network Vibes",
            "",
            "Like a screensaver from the past,",
            "your network data bounces through",
            "the digital void, hitting walls",
            "and boundaries of protocols.",
            "",
            "Each bounce represents a hop",
            "through routers and switches,",
            "ricocheting across the internet's",
            "infrastructure like a digital",
            "pinball machine.",
            "",
            "Nostalgic packets, forever in",
            "motion through cyberspace...",
        ],
    };

    let paragraph = Paragraph::new(lore_text.join("\n"))
        .block(Block::default().borders(Borders::ALL).title(" Network Lore "))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

fn render_animation_window(f: &mut Frame, area: Rect, frame: usize, avg_rtt: f64, animation_type: AnimationType, bounce_pos: (f64, f64)) {
    let (animation_art, title) = match animation_type {
        AnimationType::Plasma => {
            let art = generate_plasma_animation(frame, area.width as usize, area.height as usize);
            (art, format!(" Plasma Field - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::Globe => {
            let art = generate_globe_animation(frame, area.width as usize, area.height as usize);
            (art, format!(" Digital Earth - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::BouncingLogo => {
            let art = generate_bouncing_logo_animation(bounce_pos, area.width as usize, area.height as usize);
            (art, format!(" Retro Bounce - RTT: {:.1}ms ", avg_rtt))
        },
    };
    
    let color = if avg_rtt < 50.0 {
        Color::Green
    } else if avg_rtt < 150.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let paragraph = Paragraph::new(animation_art)
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

fn generate_globe_animation(frame: usize, width: usize, height: usize) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Globe symbols for different parts of the Earth
    let land_chars = ['▓', '█', '▆', '▅'];
    let ocean_chars = ['~', '≈', '∼', '◦'];
    let cloud_chars = ['☁', '☁', '⋅', ' '];
    
    let center_x = effective_width / 2;
    let center_y = effective_height / 2;
    let radius = std::cmp::min(center_x, center_y).saturating_sub(2);
    
    for y in 0..effective_height {
        let mut line = String::new();
        for x in 0..effective_width {
            let dx = x as i32 - center_x as i32;
            let dy = y as i32 - center_y as i32;
            let distance = ((dx * dx + dy * dy) as f64).sqrt();
            
            if distance <= radius as f64 {
                // Inside the globe - show rotating Earth
                let angle = ((x as f64 - center_x as f64) / radius as f64 + frame as f64 * 0.1).sin();
                let latitude = (y as f64 - center_y as f64) / radius as f64;
                
                // Simulate continents and oceans
                let is_land = angle > 0.3 || (angle > -0.2 && latitude.abs() < 0.5);
                let has_clouds = ((x + y + frame) % 7) == 0;
                
                let char_to_use = if has_clouds {
                    cloud_chars[frame % cloud_chars.len()]
                } else if is_land {
                    land_chars[(x + y + frame) % land_chars.len()]
                } else {
                    ocean_chars[(x + y + frame / 2) % ocean_chars.len()]
                };
                
                line.push(char_to_use);
            } else if distance <= (radius + 1) as f64 {
                // Atmosphere
                line.push('·');
            } else {
                // Space
                if ((x + y + frame) % 15) == 0 {
                    line.push('*'); // Stars
                } else {
                    line.push(' ');
                }
            }
        }
        result.push(line);
    }
    
    // Add spinning indicator
    if effective_height > 4 {
        let indicator_y = effective_height - 2;
        if indicator_y < result.len() {
            let spin_chars = ['◐', '◓', '◑', '◒'];
            let spin_char = spin_chars[frame % spin_chars.len()];
            let indicator_text = format!("🌍 Earth spins {}", spin_char);
            
            if effective_width > indicator_text.len() {
                let start_x = (effective_width - indicator_text.len()) / 2;
                let mut chars: Vec<char> = result[indicator_y].chars().collect();
                for (i, c) in indicator_text.chars().enumerate() {
                    if start_x + i < chars.len() {
                        chars[start_x + i] = c;
                    }
                }
                result[indicator_y] = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
}

fn generate_bouncing_logo_animation(bounce_pos: (f64, f64), width: usize, height: usize) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Create empty field
    for _ in 0..effective_height {
        result.push(" ".repeat(effective_width));
    }
    
    // DVD-style logo
    let logo = vec![
        "██████",
        "██  ██", 
        "██████",
        "██",
        "██",
    ];
    
    let logo_width = 6;
    let logo_height = logo.len();
    
    // Position the logo
    let x_pos = (bounce_pos.0 as usize).min(effective_width.saturating_sub(logo_width));
    let y_pos = (bounce_pos.1 as usize).min(effective_height.saturating_sub(logo_height));
    
    // Draw the logo
    for (logo_y, logo_line) in logo.iter().enumerate() {
        let target_y = y_pos + logo_y;
        if target_y < result.len() {
            let mut chars: Vec<char> = result[target_y].chars().collect();
            for (logo_x, logo_char) in logo_line.chars().enumerate() {
                let target_x = x_pos + logo_x;
                if target_x < chars.len() {
                    chars[target_x] = logo_char;
                }
            }
            result[target_y] = chars.into_iter().collect();
        }
    }
    
    // Add corner decorations to show bounds
    if effective_height > 0 && effective_width > 0 {
        // Top corners
        if let Some(first_line) = result.get_mut(0) {
            let mut chars: Vec<char> = first_line.chars().collect();
            if chars.len() > 0 {
                chars[0] = '┌';
            }
            if chars.len() > 1 {
                let last_idx = chars.len() - 1;
                chars[last_idx] = '┐';
            }
            *first_line = chars.into_iter().collect();
        }
        
        // Bottom corners
        if let Some(last_line) = result.last_mut() {
            let mut chars: Vec<char> = last_line.chars().collect();
            if chars.len() > 0 {
                chars[0] = '└';
            }
            if chars.len() > 1 {
                let last_idx = chars.len() - 1;
                chars[last_idx] = '┘';
            }
            *last_line = chars.into_iter().collect();
        }
    }
    
    // Add retro text
    if effective_height > 2 {
        let retro_text = "RETRO SCREENSAVER MODE";
        let text_y = effective_height - 2;
        if text_y < result.len() && effective_width > retro_text.len() {
            let start_x = (effective_width - retro_text.len()) / 2;
            let mut chars: Vec<char> = result[text_y].chars().collect();
            for (i, c) in retro_text.chars().enumerate() {
                if start_x + i < chars.len() {
                    chars[start_x + i] = c;
                }
            }
            result[text_y] = chars.into_iter().collect();
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