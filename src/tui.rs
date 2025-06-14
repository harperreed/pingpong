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
    Matrix,
    Dna,
    Waveform,
}

impl AnimationType {
    pub fn random() -> Self {
        use rand::Rng;
        
        let mut rng = rand::thread_rng();
        match rng.gen_range(0..6) {
            0 => AnimationType::Plasma,
            1 => AnimationType::Globe,
            2 => AnimationType::BouncingLogo,
            3 => AnimationType::Matrix,
            4 => AnimationType::Dna,
            _ => AnimationType::Waveform,
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
    pub start_time: Instant,
    pub animation_type: AnimationType,
    pub bounce_x: f64,
    pub bounce_y: f64,
    pub bounce_dx: f64,
    pub bounce_dy: f64,
}

impl TuiState {
    pub fn with_animation(animation_type: AnimationType) -> Self {
        // Debug: Log which animation was selected
        eprintln!("🎨 Selected animation: {:?}", animation_type);
        
        let (bounce_dx, bounce_dy) = match animation_type {
            AnimationType::BouncingLogo => (1.5, 1.2), // Initial velocity
            _ => (0.0, 0.0),
        };
        
        let now = Instant::now();
        
        Self {
            selected_tab: 0,
            selected_host: 0,
            show_help: false,
            paused: false,
            animation_frame: 0,
            last_frame_time: now,
            start_time: now,
            animation_type,
            bounce_x: 20.0, // Starting position
            bounce_y: 8.0,
            bounce_dx,
            bounce_dy,
        }
    }
}

impl Default for TuiState {
    fn default() -> Self {
        // Initialize with random animation
        let animation_type = AnimationType::random();
        Self::with_animation(animation_type)
    }
}

pub struct TuiApp {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    state: TuiState,
    host_info: Vec<(String, String)>, // (id, name)
}

impl TuiApp {
    pub async fn new(animation_type: Option<AnimationType>) -> anyhow::Result<Self> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let state = if let Some(anim_type) = animation_type {
            TuiState::with_animation(anim_type)
        } else {
            TuiState::default()
        };

        Ok(Self {
            terminal,
            state,
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
            self.state.animation_frame = self.state.animation_frame.wrapping_add(1);
            self.state.last_frame_time = now;
            
            // Update bouncing logo position if that's the current animation
            if self.state.animation_type == AnimationType::BouncingLogo {
                self.update_bounce_position();
            }
        }
        
        let animation_frame = self.state.animation_frame;
        let animation_time = self.state.start_time.elapsed().as_secs_f64();
        let animation_type = self.state.animation_type;
        let bounce_pos = (self.state.bounce_x, self.state.bounce_y);
        
        self.terminal.draw(move |f| {
            if show_help {
                render_help(f);
            } else {
                render_main(f, stats, &host_info, animation_frame, animation_time, avg_rtt, animation_type, bounce_pos);
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

fn render_main(f: &mut Frame, stats: &HashMap<String, PingStats>, host_info: &[(String, String)], animation_frame: usize, animation_time: f64, avg_rtt: f64, animation_type: AnimationType, bounce_pos: (f64, f64)) {
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
    render_animation_window(f, main_chunks[1], animation_frame, animation_time, avg_rtt, animation_type, bounce_pos);
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
        AnimationType::Matrix => vec![
            "💚 Matrix Digital Rain",
            "",
            "Wake up, Neo... Your network",
            "flows with cascading code that",
            "reveals the true nature of",
            "digital reality.",
            "",
            "Green characters fall like rain,",
            "each symbol a packet traversing",
            "the matrix of interconnected",
            "systems that bind our world.",
            "",
            "The faster your connection,",
            "the faster the code flows...",
            "Red pill or blue pill?",
        ],
        AnimationType::Dna => vec![
            "🧬 Network DNA Helix",
            "",
            "Your network connection has",
            "its own genetic code - a double",
            "helix of data packets and",
            "acknowledgments spiraling",
            "through digital space.",
            "",
            "Perfect connections show stable,",
            "graceful helical motion.",
            "Network issues manifest as",
            "mutations in the data stream.",
            "",
            "The backbone of digital life",
            "twists through fiber and air...",
        ],
        AnimationType::Waveform => vec![
            "📊 Network Oscilloscope",
            "",
            "Your connection pulses like a",
            "heartbeat on an oscilloscope,",
            "showing the vital signs of",
            "data flowing through cables",
            "and wireless frequencies.",
            "",
            "Strong signals create bold,",
            "clear waveforms. Weak signals",
            "show irregular patterns and",
            "interference noise.",
            "",
            "Listen to the rhythm of your",
            "network's electronic pulse...",
        ],
    };

    let paragraph = Paragraph::new(lore_text.join("\n"))
        .block(Block::default().borders(Borders::ALL).title(" Network Lore "))
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

fn render_animation_window(f: &mut Frame, area: Rect, _frame: usize, animation_time: f64, avg_rtt: f64, animation_type: AnimationType, bounce_pos: (f64, f64)) {
    // Check for connection failure or 0ms ping (suspicious)
    let has_connection_failure = avg_rtt <= 0.0 || avg_rtt.is_nan() || avg_rtt.is_infinite();
    
    let (mut animation_art, title) = match animation_type {
        AnimationType::Plasma => {
            let art = generate_plasma_animation(animation_time, area.width as usize, area.height as usize);
            (art, format!(" Plasma Field - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::Globe => {
            let art = generate_globe_animation(animation_time, area.width as usize, area.height as usize);
            (art, format!(" Digital Earth - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::BouncingLogo => {
            let art = generate_bouncing_rtt_animation(bounce_pos, area.width as usize, area.height as usize, avg_rtt);
            (art, format!(" Bouncing RTT - {:.1}ms ", avg_rtt))
        },
        AnimationType::Matrix => {
            let art = generate_matrix_animation(animation_time, area.width as usize, area.height as usize, avg_rtt);
            (art, format!(" Matrix Code - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::Dna => {
            let art = generate_dna_animation(animation_time, area.width as usize, area.height as usize, avg_rtt);
            (art, format!(" DNA Helix - RTT: {:.1}ms ", avg_rtt))
        },
        AnimationType::Waveform => {
            let art = generate_waveform_animation(animation_time, area.width as usize, area.height as usize, avg_rtt);
            (art, format!(" Network Pulse - RTT: {:.1}ms ", avg_rtt))
        },
    };
    
    // Overlay flashing red X for connection failures
    if has_connection_failure {
        // Flash every 0.5 seconds
        let flash_on = ((animation_time * 2.0) as usize % 2) == 0;
        if flash_on {
            animation_art = generate_connection_failure_overlay(animation_art, area.width as usize, area.height as usize);
        }
    }
    
    let color = if has_connection_failure {
        Color::Red
    } else if avg_rtt < 50.0 {
        Color::Green
    } else if avg_rtt < 150.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let final_title = if has_connection_failure {
        " CONNECTION FAILED! ".to_string()
    } else {
        title
    };

    let paragraph = Paragraph::new(animation_art)
        .block(Block::default().borders(Borders::ALL).title(final_title))
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
    // Much faster frame rates for smoother animations
    // Fast networks (< 50ms) spin very fast (50ms per frame)
    // Medium networks (50-150ms) spin fast (100ms per frame)  
    // Slow networks (> 150ms) spin medium (200ms per frame)
    if avg_rtt < 50.0 {
        50  // 20 FPS
    } else if avg_rtt < 150.0 {
        100 // 10 FPS
    } else {
        200 // 5 FPS
    }
}

fn generate_plasma_animation(time: f64, width: usize, height: usize) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Advanced plasma characters with multiple layers
    let plasma_layers = [
        [' ', '░', '▒', '▓', '█', '▓', '▒', '░', ' '],      // Base layer
        ['·', '•', '○', '●', '◉', '⚫', '◯', '○', '·'],      // Energy layer  
        [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'],      // Vertical bars
        [' ', '▖', '▗', '▘', '▝', '▞', '▟', '▙', '█'],      // Block patterns
        ['˙', '∘', '○', '◌', '◯', '●', '◉', '⬢', '⬡'],      // Geometric
    ];
    
    let fx_chars = ['✦', '✧', '✩', '✪', '✫', '✬', '✭', '✮', '✯', '✰', '✱', '✲', '⚡', '⟡', '⟢', '⟣'];
    
    // Create advanced plasma field
    let time_int = (time * 10.0) as usize;
    for y in 0..effective_height {
        let mut line = String::new();
        for x in 0..effective_width {
            
            // Multiple interference waves with different frequencies
            let wave1 = (x as f64 * 0.3 + time * 1.5).sin();
            let wave2 = (y as f64 * 0.25 + time * 1.8).sin();
            let wave3 = ((x + y) as f64 * 0.15 + time * 0.9).sin();
            let wave4 = ((x as f64 * 0.1).cos() + (y as f64 * 0.12).cos() + time * 2.1).sin();
            let wave5 = (((x * x + y * y) as f64).sqrt() * 0.2 - time * 1.2).sin(); // Radial wave
            
            // Turbulence function for more organic feel
            let turbulence = ((x as f64 * 0.05).sin() * (y as f64 * 0.07).cos() + time * 0.5).sin() * 0.3;
            
            // Combine all waves with different weights
            let intensity = (wave1 * 2.0 + wave2 * 1.8 + wave3 * 1.5 + wave4 * 1.2 + wave5 * 0.8 + turbulence) * 0.8;
            
            // Map to character index (0-8)
            let char_index = ((intensity + 3.0) * 1.33).max(0.0).min(8.0) as usize;
            
            // Choose layer based on position and time for variation
            let layer_choice = (x + y + time_int / 3) % plasma_layers.len();
            let base_char = plasma_layers[layer_choice][char_index];
            
            // Add special effects for high intensity areas
            let char_to_use = if intensity > 2.0 && (x + y + time_int) % 7 == 0 {
                fx_chars[time_int % fx_chars.len()]
            } else if intensity > 1.5 && (x * 2 + y + time_int / 2) % 11 == 0 {
                fx_chars[(time_int / 2) % fx_chars.len()]
            } else {
                base_char
            };
            
            line.push(char_to_use);
        }
        result.push(line);
    }
    
    // Add dynamic energy nodes that move around
    let num_nodes = 3 + ((time * 0.5) as usize) % 3; // 3-5 nodes
    for node in 0..num_nodes {
        let node_time = time + node as f64 * 2.0;
        let node_x = ((node_time * 0.7).sin() * (effective_width as f64 - 10.0) / 2.0 + effective_width as f64 / 2.0) as usize;
        let node_y = ((node_time * 0.9 + node as f64).cos() * (effective_height as f64 - 6.0) / 2.0 + effective_height as f64 / 2.0) as usize;
        
        if node_x < effective_width && node_y < effective_height && node_y < result.len() {
            let mut chars: Vec<char> = result[node_y].chars().collect();
            if node_x < chars.len() {
                let node_char = match ((time * 3.0) as usize + node * 3) % 6 {
                    0 => '◉',
                    1 => '⚡',
                    2 => '✦',
                    3 => '●',
                    4 => '⟡',
                    _ => '◯',
                };
                chars[node_x] = node_char;
            }
            result[node_y] = chars.into_iter().collect();
        }
    }
    
    // Add energy field borders with flowing effect
    if effective_height > 4 && effective_width > 8 {
        let flow_chars = ['─', '━', '═', '▬', '▭'];
        let flow_char = flow_chars[((time * 5.0) as usize / 2) % flow_chars.len()];
        
        // Top and bottom borders
        let time_int_border = (time * 10.0) as usize;
        for x in 0..effective_width {
            if x < result[0].len() && (x + time_int_border) % 4 < 2 {
                let mut chars: Vec<char> = result[0].chars().collect();
                chars[x] = flow_char;
                result[0] = chars.into_iter().collect();
            }
            
            let last_idx = result.len() - 1;
            if x < result[last_idx].len() && (x + time_int_border + 2) % 4 < 2 {
                let mut chars: Vec<char> = result[last_idx].chars().collect();
                chars[x] = flow_char;
                result[last_idx] = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
}

fn generate_globe_animation(time: f64, width: usize, height: usize) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Enhanced Earth surface with realistic continent patterns
    let continent_layers = [
        ['▓', '█', '▆', '▅', '▄', '▃', '▂', '▁'],          // Mountain ranges
        ['▰', '▱', '▮', '▯', '◪', '◫', '◨', '◧'],          // Plains and forests
        ['⬛', '⬜', '◼', '◻', '▪', '▫', '■', '□'],          // Urban areas
    ];
    
    let ocean_layers = [
        ['~', '≈', '∼', '◦', '∘', '○', '◯', '●'],          // Ocean waves
        ['░', '▒', '▓', '█', '▆', '▅', '▄', '▃'],          // Ocean depths
        ['⋅', '∙', '•', '◘', '◙', '○', '◯', '●'],          // Currents
    ];
    
    let atmosphere_chars = ['⋅', '∘', '○', '◯', '●', '◉', '⬡', '⬢'];
    let cloud_patterns = ['☁', '⛅', '⛈', '🌤', '⋅', '∘', ' ', ' '];
    let star_chars = ['✦', '✧', '✩', '✪', '✫', '✬', '✭', '✮', '*', '·'];
    
    let center_x = effective_width / 2;
    let center_y = effective_height / 2;
    let radius = std::cmp::min(center_x, center_y).saturating_sub(2);
    
    for y in 0..effective_height {
        let mut line = String::new();
        for x in 0..effective_width {
            let dx = x as f64 - center_x as f64;
            let dy = y as f64 - center_y as f64;
            let distance = (dx * dx + dy * dy).sqrt();
            
            if distance <= radius as f64 {
                // Inside the globe - realistic Earth with rotation
                let rotation = time * 0.2; // Smooth continuous rotation
                let longitude = (dx / radius as f64).atan2(-dy / radius as f64) + rotation;
                let latitude = (dy / radius as f64).asin();
                
                // Create realistic continent patterns using multiple noise functions
                let continent_noise1 = (longitude * 2.0).sin() * (latitude * 3.0).cos();
                let continent_noise2 = (longitude * 3.0 + 1.5).cos() * (latitude * 2.0).sin();
                let continent_noise3 = (longitude * 1.5 - 0.7).sin() * (latitude * 4.0).cos();
                
                let land_probability = (continent_noise1 + continent_noise2 * 0.7 + continent_noise3 * 0.5) * 0.6;
                
                // Day/night cycle with terminator line
                let sun_angle = time * 0.15; // Smooth sun movement
                let day_night = (longitude - sun_angle).cos();
                let is_day = day_night > 0.0;
                let terminator_blend = (day_night * 3.0).max(-1.0).min(1.0);
                
                // Weather patterns and clouds
                let cloud_noise = (longitude * 4.0 + time * 0.3).sin() * (latitude * 3.0).cos();
                let has_clouds = cloud_noise > 0.6 && (x + y + (time * 3.0) as usize) % 8 < 3;
                
                // Ocean currents and movement
                let ocean_current = (longitude * 2.0 + time * 0.5).sin() * 0.5;
                
                let char_to_use = if has_clouds {
                    let cloud_intensity = ((cloud_noise + 1.0) * 4.0) as usize % cloud_patterns.len();
                    cloud_patterns[cloud_intensity]
                } else if land_probability > 0.1 {
                    // Land features based on latitude and terrain type
                    let terrain_type = ((latitude.abs() * 2.0 + longitude * 1.5) as usize) % continent_layers.len();
                    let elevation = ((land_probability + 1.0) * 4.0) as usize % continent_layers[terrain_type].len();
                    
                    // Adjust for day/night (darker at night)
                    if is_day || terminator_blend > -0.5 {
                        continent_layers[terrain_type][elevation]
                    } else {
                        // Nighttime - show city lights occasionally
                        if elevation > 4 && (x + y + (time * 2.0) as usize) % 12 == 0 {
                            '●' // City lights
                        } else {
                            '▓' // Darker land
                        }
                    }
                } else {
                    // Ocean with current effects
                    let ocean_type = (ocean_current + 1.0) as usize % ocean_layers.len();
                    let wave_intensity = ((distance / radius as f64 + time) * 4.0) as usize % ocean_layers[ocean_type].len();
                    ocean_layers[ocean_type][wave_intensity]
                };
                
                line.push(char_to_use);
            } else if distance <= (radius + 2) as f64 {
                // Atmospheric layers with aurora effects
                let atmo_distance = distance - radius as f64;
                let rotation = time * 0.2;
                let longitude = (dx / radius as f64).atan2(-dy / radius as f64) + rotation;
                let latitude = (dy / radius as f64).asin();
                let aurora_effect = (longitude * 4.0 + time).sin() * (latitude * 2.0).cos();
                
                let char_to_use = if atmo_distance < 1.0 && aurora_effect > 0.8 && latitude.abs() > 0.6 {
                    // Aurora at poles
                    let aurora_chars = ['◉', '⚡', '✦', '◯', '●'];
                    aurora_chars[(time * 5.0) as usize % aurora_chars.len()]
                } else {
                    // Normal atmosphere
                    let atmo_intensity = (atmo_distance * 4.0) as usize % atmosphere_chars.len();
                    atmosphere_chars[atmo_intensity]
                };
                
                line.push(char_to_use);
            } else {
                // Deep space with twinkling stars and satellites
                let star_seed = x * 17 + y * 23 + (time * 1.25) as usize;
                let char_to_use = if star_seed % 25 == 0 {
                    star_chars[star_seed % star_chars.len()]
                } else if star_seed % 47 == 0 && (time * 1.0) as usize % 15 < 3 {
                    '🛰' // Occasional satellite
                } else {
                    ' '
                };
                
                line.push(char_to_use);
            }
        }
        result.push(line);
    }
    
    // Add dynamic orbital indicators
    if effective_height > 6 && effective_width > 20 {
        // ISS orbital path
        let iss_angle = time;
        let iss_x = (center_x as f64 + (radius as f64 + 3.0) * iss_angle.cos()) as usize;
        let iss_y = (center_y as f64 + (radius as f64 + 3.0) * iss_angle.sin() * 0.5) as usize;
        
        if iss_x < effective_width && iss_y < effective_height && iss_y < result.len() {
            let mut chars: Vec<char> = result[iss_y].chars().collect();
            if iss_x < chars.len() {
                chars[iss_x] = '🚀';
            }
            result[iss_y] = chars.into_iter().collect();
        }
    }
    
    // Add status information with global network pulse
    if effective_height > 3 {
        let status_y = effective_height - 1;
        if status_y < result.len() {
            let pulse_chars = ['◐', '◓', '◑', '◒', '◉', '●', '○', '◯'];
            let pulse_char = pulse_chars[(time * 3.0) as usize % pulse_chars.len()];
            let time_indicator = match ((time * 0.1) as usize) % 24 {
                0..=5 => "🌙 Night",
                6..=11 => "🌅 Dawn", 
                12..=17 => "☀️ Day",
                _ => "🌆 Dusk",
            };
            let status_text = format!("Global Network {} {}", pulse_char, time_indicator);
            
            if effective_width > status_text.len() {
                let start_x = (effective_width - status_text.len()) / 2;
                let mut chars: Vec<char> = result[status_y].chars().collect();
                for (i, c) in status_text.chars().enumerate() {
                    if start_x + i < chars.len() {
                        chars[start_x + i] = c;
                    }
                }
                result[status_y] = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
}

fn generate_bouncing_rtt_animation(bounce_pos: (f64, f64), width: usize, height: usize, avg_rtt: f64) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Create empty field
    for _ in 0..effective_height {
        result.push(" ".repeat(effective_width));
    }
    
    // RTT text to bounce
    let rtt_text = format!("{:.1}ms", avg_rtt);
    let text_width = rtt_text.len();
    let text_height = 1;
    
    // Position the RTT text
    let x_pos = (bounce_pos.0 as usize).min(effective_width.saturating_sub(text_width));
    let y_pos = (bounce_pos.1 as usize).min(effective_height.saturating_sub(text_height));
    
    // Draw the RTT text
    if y_pos < result.len() {
        let mut chars: Vec<char> = result[y_pos].chars().collect();
        for (i, c) in rtt_text.chars().enumerate() {
            let target_x = x_pos + i;
            if target_x < chars.len() {
                chars[target_x] = c;
            }
        }
        result[y_pos] = chars.into_iter().collect();
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
    
    // Add trail effect - show previous positions with dots
    if effective_height > 2 && x_pos > 2 && y_pos > 0 {
        // Add a subtle trail behind the bouncing text
        for trail_offset in 1..=3 {
            let trail_x = x_pos.saturating_sub(trail_offset);
            let trail_char = match trail_offset {
                1 => '·',
                2 => '.',
                _ => ' ',
            };
            
            if trail_x < effective_width && y_pos < result.len() {
                let mut chars: Vec<char> = result[y_pos].chars().collect();
                if trail_x < chars.len() && chars[trail_x] == ' ' {
                    chars[trail_x] = trail_char;
                }
                result[y_pos] = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
}

fn generate_matrix_animation(time: f64, width: usize, height: usize, avg_rtt: f64) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Authentic Matrix characters - katakana, numbers, and symbols from the movie
    let matrix_chars = [
        'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ',
        'サ', 'シ', 'ス', 'セ', 'ソ', 'タ', 'チ', 'ツ', 'テ', 'ト',
        'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', 'ホ',
        'マ', 'ミ', 'ム', 'メ', 'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', 'リ',
        'ル', 'レ', 'ロ', 'ワ', 'ヲ', 'ン', '0', '1', '2', '3',
        '4', '5', '6', '7', '8', '9', ':', '·', '"', '=',
        '*', '+', '<', '>', '¦', '|', 'Z', '_'
    ];
    
    // Initialize black background
    for _ in 0..effective_height {
        result.push(" ".repeat(effective_width));
    }
    
    // Each column has its own falling stream
    let num_columns = effective_width;
    let speed_multiplier = if avg_rtt < 50.0 { 2.0 } else if avg_rtt < 150.0 { 1.5 } else { 1.0 };
    
    for x in 0..num_columns {
        // Stable column seed that doesn't change too rapidly
        let column_base_seed = x * 17; // Fixed seed per column
        let time_factor = (time * speed_multiplier * 0.3) as usize; // Slower time progression
        let column_phase = (x as f64 * 0.618) % 1.0; // Golden ratio for nice distribution
        
        // Determine if this column should have a stream (stable decision)
        let has_stream = (column_base_seed % 13) < 4; // About 30% of columns active
        
        if has_stream {
            // Stream parameters - more stable
            let stream_speed = 1.0 + ((column_base_seed / 7) % 3) as f64 * 0.3; // Slower varying speeds
            let stream_length = 8 + (column_base_seed % 8); // Lengths 8-15 (shorter range)
            let stream_y_offset = (time * speed_multiplier * stream_speed + column_phase * effective_height as f64) % (effective_height as f64 + stream_length as f64 * 2.0);
            
            for i in 0..stream_length {
                let y = (stream_y_offset - i as f64) as isize;
                
                if y >= 0 && y < effective_height as isize {
                    let y_pos = y as usize;
                    
                    // More stable character selection - changes slower
                    let char_seed = (column_base_seed + i * 11 + time_factor / 3) % matrix_chars.len();
                    let matrix_char = matrix_chars[char_seed];
                    
                    // Brightness based on position in stream
                    let brightness_char = if i == 0 {
                        // Bright white head (leading character)
                        matrix_char
                    } else if i == 1 {
                        // Bright green second character
                        matrix_char
                    } else if i < stream_length / 2 {
                        // Medium brightness characters
                        matrix_char
                    } else {
                        // Fading tail characters
                        matrix_char
                    };
                    
                    if y_pos < result.len() {
                        let mut chars: Vec<char> = result[y_pos].chars().collect();
                        // Ensure line is long enough
                        while chars.len() <= x {
                            chars.push(' ');
                        }
                        if x < chars.len() {
                            chars[x] = brightness_char;
                        }
                        result[y_pos] = chars.into_iter().collect();
                    }
                }
            }
        }
        
        // Add occasional static characters (residual code) - much more stable
        if !has_stream && (column_base_seed % 23) < 3 {
            let static_y = (column_base_seed / 5) % effective_height;
            let static_char = matrix_chars[(column_base_seed * 7 + time_factor / 10) % matrix_chars.len()];
            
            if static_y < result.len() {
                let mut chars: Vec<char> = result[static_y].chars().collect();
                while chars.len() <= x {
                    chars.push(' ');
                }
                if x < chars.len() {
                    chars[x] = static_char;
                }
                result[static_y] = chars.into_iter().collect();
            }
        }
    }
    
    // Add network-related glitch effects for poor connections - less flashy
    if avg_rtt > 150.0 {
        let glitch_intensity = ((avg_rtt - 150.0) / 100.0).min(0.3);
        let num_glitches = (effective_height as f64 * effective_width as f64 * glitch_intensity * 0.02) as usize;
        
        for _ in 0..num_glitches {
            // More stable glitch positions - less random jumping
            let glitch_x = ((time * 3.0) as usize * 7 + effective_width / 3) % effective_width;
            let glitch_y = ((time * 2.0) as usize * 11 + effective_height / 4) % effective_height;
            
            if glitch_y < result.len() {
                let mut chars: Vec<char> = result[glitch_y].chars().collect();
                while chars.len() <= glitch_x {
                    chars.push(' ');
                }
                if glitch_x < chars.len() {
                    // Less harsh glitch characters
                    let glitch_chars = ['▒', '░', '·', '?', '‾'];
                    let glitch_seed = ((time * 1.0) as usize + glitch_x + glitch_y) % glitch_chars.len();
                    chars[glitch_x] = glitch_chars[glitch_seed];
                }
                result[glitch_y] = chars.into_iter().collect();
            }
        }
    }
    
    // Neo's wake-up message for excellent connections
    if avg_rtt < 15.0 && ((time * 0.3) as usize % 25) < 4 {
        let messages = [
            "Wake up, Neo...",
            "The Matrix has you...",
            "Follow the white rabbit",
            "There is no spoon"
        ];
        let message = messages[((time * 0.1) as usize) % messages.len()];
        let center_y = effective_height / 2;
        
        if center_y < result.len() && effective_width > message.len() {
            let start_x = (effective_width - message.len()) / 2;
            let mut chars: Vec<char> = result[center_y].chars().collect();
            
            // Clear the area around the message
            for clear_x in start_x.saturating_sub(1)..=(start_x + message.len()).min(effective_width - 1) {
                while chars.len() <= clear_x {
                    chars.push(' ');
                }
                if clear_x < chars.len() {
                    chars[clear_x] = ' ';
                }
            }
            
            // Write the message
            for (i, c) in message.chars().enumerate() {
                let msg_x = start_x + i;
                while chars.len() <= msg_x {
                    chars.push(' ');
                }
                if msg_x < chars.len() {
                    chars[msg_x] = c;
                }
            }
            result[center_y] = chars.into_iter().collect();
        }
    }
    
    result.join("\n")
}

fn generate_dna_animation(time: f64, width: usize, height: usize, avg_rtt: f64) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // DNA base pairs: A-T, G-C
    let _bases = ['A', 'T', 'G', 'C'];
    let backbone_chars = ['│', '║', '|'];
    let bond_chars = ['─', '═', '~', '≈'];
    
    // Initialize the field
    for _ in 0..effective_height {
        result.push(" ".repeat(effective_width));
    }
    
    let center_x = effective_width / 2;
    let helix_width = (effective_width / 4).max(3).min(8);
    
    // Generate rotating double helix
    for y in 0..effective_height {
        let t = time + y as f64 * 0.3; // Offset each row for helix twist
        let rotation_speed = if avg_rtt < 50.0 { 2.0 } else { 1.0 }; // Faster rotation for better performance
        
        // Left strand position (sine wave)
        let left_offset = (t * rotation_speed).sin() * helix_width as f64;
        let left_x = (center_x as f64 + left_offset) as usize;
        
        // Right strand position (cosine wave, 180 degrees out of phase)
        let right_offset = (t * rotation_speed + std::f64::consts::PI).sin() * helix_width as f64;
        let right_x = (center_x as f64 + right_offset) as usize;
        
        if y < result.len() {
            let mut chars: Vec<char> = result[y].chars().collect();
            
            // Draw left backbone
            if left_x < chars.len() {
                chars[left_x] = backbone_chars[(y + (time * 2.0) as usize) % backbone_chars.len()];
            }
            
            // Draw right backbone  
            if right_x < chars.len() {
                chars[right_x] = backbone_chars[(y + (time * 2.0) as usize) % backbone_chars.len()];
            }
            
            // Draw base pairs when strands are close enough
            let distance = (left_x as isize - right_x as isize).abs();
            if distance <= helix_width as isize && distance > 1 {
                let min_x = left_x.min(right_x);
                let max_x = left_x.max(right_x);
                
                // Draw bonds between base pairs
                for bond_x in (min_x + 1)..max_x {
                    if bond_x < chars.len() {
                        let bond_char = bond_chars[(y + bond_x) % bond_chars.len()];
                        chars[bond_x] = bond_char;
                    }
                }
                
                // Add base letters at strand positions
                let base_pair_index = (y + (time * 0.5) as usize) % 4;
                let (left_base, right_base) = match base_pair_index {
                    0 => ('A', 'T'),
                    1 => ('T', 'A'), 
                    2 => ('G', 'C'),
                    _ => ('C', 'G'),
                };
                
                if left_x > 0 && left_x - 1 < chars.len() {
                    chars[left_x - 1] = left_base;
                }
                if right_x + 1 < chars.len() {
                    chars[right_x + 1] = right_base;
                }
            }
            
            result[y] = chars.into_iter().collect();
        }
    }
    
    // Add network quality indicator as DNA mutations/stability
    if avg_rtt > 100.0 {
        // Show "mutations" for poor network performance
        let mutation_rate = ((avg_rtt - 100.0) / 100.0).min(0.5);
        let num_mutations = (effective_height as f64 * mutation_rate) as usize;
        
        for _ in 0..num_mutations {
            let y = ((time * 3.0) as usize + rand::random::<usize>()) % effective_height;
            let x = rand::random::<usize>() % effective_width;
            
            if y < result.len() {
                let mut chars: Vec<char> = result[y].chars().collect();
                if x < chars.len() {
                    chars[x] = '×'; // Mutation marker
                }
                result[y] = chars.into_iter().collect();
            }
        }
    }
    
    // Add status information 
    if effective_height > 2 {
        let status_y = effective_height - 1;
        let quality = if avg_rtt < 50.0 { "STABLE" } else if avg_rtt < 150.0 { "DEGRADED" } else { "MUTATING" };
        let status_text = format!("DNA:{} RTT:{:.1}ms", quality, avg_rtt);
        
        if status_y < result.len() && effective_width > status_text.len() {
            let start_x = (effective_width - status_text.len()) / 2;
            let mut chars: Vec<char> = result[status_y].chars().collect();
            for (i, c) in status_text.chars().enumerate() {
                if start_x + i < chars.len() {
                    chars[start_x + i] = c;
                }
            }
            result[status_y] = chars.into_iter().collect();
        }
    }
    
    result.join("\n")
}

fn generate_waveform_animation(time: f64, width: usize, height: usize, avg_rtt: f64) -> String {
    let mut result = Vec::new();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Initialize the field
    for _ in 0..effective_height {
        result.push(" ".repeat(effective_width));
    }
    
    let center_y = effective_height / 2;
    let amplitude = (effective_height / 3).max(2);
    
    // Generate oscilloscope-style waveforms
    for x in 0..effective_width {
        // Primary network pulse wave - frequency based on RTT performance
        let frequency = if avg_rtt < 50.0 { 0.3 } else if avg_rtt < 150.0 { 0.2 } else { 0.1 };
        let wave_phase = time * 2.0 + x as f64 * frequency;
        let primary_wave = (wave_phase.sin() * amplitude as f64) as isize;
        
        // Secondary harmonic for interference patterns
        let harmonic_wave = (wave_phase * 2.0 + time).sin() * (amplitude as f64 * 0.3);
        let combined_wave = primary_wave + harmonic_wave as isize;
        
        let y_pos = (center_y as isize + combined_wave).max(0).min(effective_height as isize - 1) as usize;
        
        // Draw main waveform
        if y_pos < result.len() {
            let mut chars: Vec<char> = result[y_pos].chars().collect();
            if x < chars.len() {
                let intensity = (combined_wave.abs() as f64 / amplitude as f64).min(1.0);
                let wave_char = if intensity > 0.8 {
                    '█'
                } else if intensity > 0.6 {
                    '▓'
                } else if intensity > 0.3 {
                    '▒'
                } else {
                    '░'
                };
                chars[x] = wave_char;
            }
            result[y_pos] = chars.into_iter().collect();
        }
        
        // Add packet burst visualization
        if ((time * 5.0 + x as f64 * 0.1) as usize % 20) < 3 {
            // Packet data as vertical bars
            let packet_height = 2 + (x % 3);
            for py in 0..packet_height {
                let packet_y = (center_y + py).min(effective_height - 1);
                if packet_y < result.len() {
                    let mut chars: Vec<char> = result[packet_y].chars().collect();
                    if x < chars.len() && chars[x] == ' ' {
                        chars[x] = '|';
                    }
                    result[packet_y] = chars.into_iter().collect();
                }
            }
        }
    }
    
    // Add network quality indicators as scope grid
    for y in (0..effective_height).step_by(effective_height / 4) {
        if y < result.len() {
            let mut chars: Vec<char> = result[y].chars().collect();
            for x in (0..effective_width).step_by(effective_width / 8) {
                if x < chars.len() && chars[x] == ' ' {
                    chars[x] = '·';
                }
            }
            result[y] = chars.into_iter().collect();
        }
    }
    
    // Add center line for zero reference
    if center_y < result.len() {
        let mut chars: Vec<char> = result[center_y].chars().collect();
        for x in (0..effective_width).step_by(4) {
            if x < chars.len() && chars[x] == ' ' {
                chars[x] = '─';
            }
        }
        result[center_y] = chars.into_iter().collect();
    }
    
    // Add signal quality and RTT display
    if effective_height > 3 {
        let signal_strength = if avg_rtt < 50.0 { "STRONG" } else if avg_rtt < 150.0 { "MEDIUM" } else { "WEAK" };
        let freq_display = format!("{}Hz", (1000.0 / avg_rtt.max(1.0)) as usize);
        
        // Top status line
        let top_status = format!("SIG:{} {}kHz", signal_strength, ((time * 10.0) as usize % 100));
        if effective_width > top_status.len() {
            let start_x = (effective_width - top_status.len()) / 2;
            let mut chars: Vec<char> = result[0].chars().collect();
            for (i, c) in top_status.chars().enumerate() {
                if start_x + i < chars.len() {
                    chars[start_x + i] = c;
                }
            }
            result[0] = chars.into_iter().collect();
        }
        
        // Bottom status line
        let bottom_status = format!("RTT:{:.1}ms {}", avg_rtt, freq_display);
        let status_y = effective_height - 1;
        if status_y < result.len() && effective_width > bottom_status.len() {
            let start_x = (effective_width - bottom_status.len()) / 2;
            let mut chars: Vec<char> = result[status_y].chars().collect();
            for (i, c) in bottom_status.chars().enumerate() {
                if start_x + i < chars.len() {
                    chars[start_x + i] = c;
                }
            }
            result[status_y] = chars.into_iter().collect();
        }
    }
    
    result.join("\n")
}

fn generate_connection_failure_overlay(base_animation: String, width: usize, height: usize) -> String {
    let mut lines: Vec<String> = base_animation.lines().map(|s| s.to_string()).collect();
    let effective_width = if width > 4 { width - 4 } else { 20 };
    let effective_height = if height > 6 { height - 6 } else { 12 };
    
    // Ensure we have enough lines
    while lines.len() < effective_height {
        lines.push(" ".repeat(effective_width));
    }
    
    // Draw a big red X across the entire animation area
    let center_x = effective_width / 2;
    let center_y = effective_height / 2;
    let size = (effective_width.min(effective_height) / 2).max(3);
    
    // Draw both diagonals of the X
    for i in 0..size {
        // Top-left to bottom-right diagonal
        let x1 = center_x - size / 2 + i;
        let y1 = center_y - size / 2 + i;
        
        // Top-right to bottom-left diagonal  
        let x2 = center_x + size / 2 - i;
        let y2 = center_y - size / 2 + i;
        
        // Draw first diagonal
        if y1 < lines.len() && x1 < effective_width {
            let mut chars: Vec<char> = lines[y1].chars().collect();
            // Ensure the line is long enough
            while chars.len() < effective_width {
                chars.push(' ');
            }
            if x1 < chars.len() {
                chars[x1] = '█';
            }
            lines[y1] = chars.into_iter().collect();
        }
        
        // Draw second diagonal
        if y2 < lines.len() && x2 < effective_width {
            let mut chars: Vec<char> = lines[y2].chars().collect();
            // Ensure the line is long enough
            while chars.len() < effective_width {
                chars.push(' ');
            }
            if x2 < chars.len() {
                chars[x2] = '█';
            }
            lines[y2] = chars.into_iter().collect();
        }
    }
    
    // Add failure message at the bottom
    if effective_height > 3 {
        let failure_messages = [
            "CONNECTION LOST",
            "NETWORK FAILURE", 
            "PING TIMEOUT",
            "NO RESPONSE"
        ];
        
        let message_index = (rand::random::<usize>()) % failure_messages.len();
        let failure_text = failure_messages[message_index];
        let bottom_y = effective_height - 2;
        
        if bottom_y < lines.len() && effective_width > failure_text.len() {
            let start_x = (effective_width - failure_text.len()) / 2;
            let mut chars: Vec<char> = lines[bottom_y].chars().collect();
            
            // Ensure the line is long enough
            while chars.len() < effective_width {
                chars.push(' ');
            }
            
            for (i, c) in failure_text.chars().enumerate() {
                if start_x + i < chars.len() {
                    chars[start_x + i] = c;
                }
            }
            lines[bottom_y] = chars.into_iter().collect();
        }
    }
    
    // Add warning symbols around the X
    if effective_height > 1 && effective_width > 6 {
        let warning_chars = ['⚠', '!', '×', '✗'];
        
        // Top warning
        if center_y > 0 {
            let mut chars: Vec<char> = lines[center_y - 1].chars().collect();
            while chars.len() < effective_width {
                chars.push(' ');
            }
            if center_x < chars.len() {
                chars[center_x] = warning_chars[0];
            }
            lines[center_y - 1] = chars.into_iter().collect();
        }
        
        // Side warnings
        if center_y < lines.len() {
            let mut chars: Vec<char> = lines[center_y].chars().collect();
            while chars.len() < effective_width {
                chars.push(' ');
            }
            if center_x > 2 && center_x - 3 < chars.len() {
                chars[center_x - 3] = warning_chars[1];
            }
            if center_x + 3 < chars.len() {
                chars[center_x + 3] = warning_chars[2];
            }
            lines[center_y] = chars.into_iter().collect();
        }
        
        // Bottom warning
        if center_y + 1 < lines.len() {
            let mut chars: Vec<char> = lines[center_y + 1].chars().collect();
            while chars.len() < effective_width {
                chars.push(' ');
            }
            if center_x < chars.len() {
                chars[center_x] = warning_chars[3];
            }
            lines[center_y + 1] = chars.into_iter().collect();
        }
    }
    
    lines.join("\n")
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