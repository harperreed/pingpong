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
        use rand::Rng;
        
        let mut rng = rand::thread_rng();
        match rng.gen_range(0..3) {
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

impl TuiState {
    pub fn with_animation(animation_type: AnimationType) -> Self {
        // Debug: Log which animation was selected
        eprintln!("🎨 Selected animation: {:?}", animation_type);
        
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
            let art = generate_bouncing_rtt_animation(bounce_pos, area.width as usize, area.height as usize, avg_rtt);
            (art, format!(" Bouncing RTT - {:.1}ms ", avg_rtt))
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
    for y in 0..effective_height {
        let mut line = String::new();
        for x in 0..effective_width {
            let time = frame as f64 * 0.2;
            
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
            let layer_choice = (x + y + frame / 3) % plasma_layers.len();
            let base_char = plasma_layers[layer_choice][char_index];
            
            // Add special effects for high intensity areas
            let char_to_use = if intensity > 2.0 && (x + y + frame) % 7 == 0 {
                fx_chars[frame % fx_chars.len()]
            } else if intensity > 1.5 && (x * 2 + y + frame / 2) % 11 == 0 {
                fx_chars[(frame / 2) % fx_chars.len()]
            } else {
                base_char
            };
            
            line.push(char_to_use);
        }
        result.push(line);
    }
    
    // Add dynamic energy nodes that move around
    let num_nodes = 3 + (frame / 20) % 3; // 3-5 nodes
    for node in 0..num_nodes {
        let node_time = frame as f64 * 0.2 + node as f64 * 2.0;
        let node_x = ((node_time * 0.7).sin() * (effective_width as f64 - 10.0) / 2.0 + effective_width as f64 / 2.0) as usize;
        let node_y = ((node_time * 0.9 + node as f64).cos() * (effective_height as f64 - 6.0) / 2.0 + effective_height as f64 / 2.0) as usize;
        
        if node_x < effective_width && node_y < effective_height && node_y < result.len() {
            let mut chars: Vec<char> = result[node_y].chars().collect();
            if node_x < chars.len() {
                let node_char = match (frame + node * 3) % 6 {
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
        let flow_char = flow_chars[(frame / 2) % flow_chars.len()];
        
        // Top and bottom borders
        for x in 0..effective_width {
            if x < result[0].len() && (x + frame) % 4 < 2 {
                let mut chars: Vec<char> = result[0].chars().collect();
                chars[x] = flow_char;
                result[0] = chars.into_iter().collect();
            }
            
            let last_idx = result.len() - 1;
            if x < result[last_idx].len() && (x + frame + 2) % 4 < 2 {
                let mut chars: Vec<char> = result[last_idx].chars().collect();
                chars[x] = flow_char;
                result[last_idx] = chars.into_iter().collect();
            }
        }
    }
    
    result.join("\n")
}

fn generate_globe_animation(frame: usize, width: usize, height: usize) -> String {
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
                let rotation = frame as f64 * 0.08; // Slower, more realistic rotation
                let longitude = (dx / radius as f64).atan2(-dy / radius as f64) + rotation;
                let latitude = (dy / radius as f64).asin();
                
                // Create realistic continent patterns using multiple noise functions
                let continent_noise1 = (longitude * 2.0).sin() * (latitude * 3.0).cos();
                let continent_noise2 = (longitude * 3.0 + 1.5).cos() * (latitude * 2.0).sin();
                let continent_noise3 = (longitude * 1.5 - 0.7).sin() * (latitude * 4.0).cos();
                
                let land_probability = (continent_noise1 + continent_noise2 * 0.7 + continent_noise3 * 0.5) * 0.6;
                
                // Day/night cycle with terminator line
                let sun_angle = frame as f64 * 0.05; // Sun position
                let day_night = (longitude - sun_angle).cos();
                let is_day = day_night > 0.0;
                let terminator_blend = (day_night * 3.0).max(-1.0).min(1.0);
                
                // Weather patterns and clouds
                let cloud_noise = (longitude * 4.0 + frame as f64 * 0.1).sin() * (latitude * 3.0).cos();
                let has_clouds = cloud_noise > 0.6 && (x + y + frame / 3) % 8 < 3;
                
                // Ocean currents and movement
                let ocean_current = (longitude * 2.0 + frame as f64 * 0.15).sin() * 0.5;
                
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
                        if elevation > 4 && (x + y + frame / 5) % 12 == 0 {
                            '●' // City lights
                        } else {
                            '▓' // Darker land
                        }
                    }
                } else {
                    // Ocean with current effects
                    let ocean_type = (ocean_current + 1.0) as usize % ocean_layers.len();
                    let wave_intensity = ((distance / radius as f64 + frame as f64 * 0.1) * 4.0) as usize % ocean_layers[ocean_type].len();
                    ocean_layers[ocean_type][wave_intensity]
                };
                
                line.push(char_to_use);
            } else if distance <= (radius + 2) as f64 {
                // Atmospheric layers with aurora effects
                let atmo_distance = distance - radius as f64;
                let rotation = frame as f64 * 0.08;
                let longitude = (dx / radius as f64).atan2(-dy / radius as f64) + rotation;
                let latitude = (dy / radius as f64).asin();
                let aurora_effect = (longitude * 4.0 + frame as f64 * 0.3).sin() * (latitude * 2.0).cos();
                
                let char_to_use = if atmo_distance < 1.0 && aurora_effect > 0.8 && latitude.abs() > 0.6 {
                    // Aurora at poles
                    let aurora_chars = ['◉', '⚡', '✦', '◯', '●'];
                    aurora_chars[frame % aurora_chars.len()]
                } else {
                    // Normal atmosphere
                    let atmo_intensity = (atmo_distance * 4.0) as usize % atmosphere_chars.len();
                    atmosphere_chars[atmo_intensity]
                };
                
                line.push(char_to_use);
            } else {
                // Deep space with twinkling stars and satellites
                let star_seed = x * 17 + y * 23 + frame / 8;
                let char_to_use = if star_seed % 25 == 0 {
                    star_chars[star_seed % star_chars.len()]
                } else if star_seed % 47 == 0 && frame % 15 < 3 {
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
        let iss_angle = frame as f64 * 0.4;
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
            let pulse_char = pulse_chars[frame % pulse_chars.len()];
            let time_indicator = match (frame / 10) % 24 {
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