// ABOUTME: Main entry point for pingpong TUI ping utility
// ABOUTME: Orchestrates the async runtime, configuration loading, and TUI initialization

use anyhow::Result;
use clap::{Parser, ValueEnum};

mod app;
mod config;
mod ping;
mod probe;
mod stats;
mod status;
mod tui;

use app::App;
use config::Config;
use tui::AnimationType;

#[derive(Debug, Clone, ValueEnum)]
enum AnimationChoice {
    Plasma,
    Globe,
    Bounce,
    Matrix,
    Dna,
    Waveform,
}

impl From<AnimationChoice> for AnimationType {
    fn from(choice: AnimationChoice) -> Self {
        match choice {
            AnimationChoice::Plasma => AnimationType::Plasma,
            AnimationChoice::Globe => AnimationType::Globe,
            AnimationChoice::Bounce => AnimationType::BouncingLogo,
            AnimationChoice::Matrix => AnimationType::Matrix,
            AnimationChoice::Dna => AnimationType::Dna,
            AnimationChoice::Waveform => AnimationType::Waveform,
        }
    }
}

#[derive(Parser)]
#[command(name = "pingpong")]
#[command(about = "A beautiful TUI ping utility for monitoring network connectivity")]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "pingpong.toml")]
    config: String,

    /// Ping interval in seconds (overrides config when set)
    #[arg(short, long)]
    interval: Option<f64>,

    /// Additional hosts to ping (can be used multiple times)
    #[arg(long)]
    host: Vec<String>,

    /// Animation type: plasma, globe, bounce, matrix, dna, or waveform
    #[arg(short, long, value_enum)]
    animation: Option<AnimationChoice>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tui::terminal_leave();
        default_hook(info);
    }));

    let cli = Cli::parse();

    // Load configuration
    let mut config = Config::load(&cli.config).unwrap_or_else(|_| Config::default());

    // Add CLI hosts to config
    for host in cli.host {
        config.add_host(host);
    }

    // Override interval if specified
    if let Some(interval) = cli.interval {
        config.set_interval(interval);
    }
    config.validate();

    // Convert animation choice if provided
    let animation_type = cli.animation.map(|choice| choice.into());

    // Initialize and run the app
    let app = App::new(config, animation_type).await?;
    app.run().await
}
