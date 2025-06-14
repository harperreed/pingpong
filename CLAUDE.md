# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Pingpong is a Rust-based TUI (Terminal User Interface) ping utility for monitoring network connectivity to multiple hosts simultaneously. It provides real-time network latency visualization, statistics, and connection quality monitoring.

## Key Architecture

### Core Components
- **main.rs**: Entry point handling CLI args and configuration loading
- **app.rs**: Main application orchestrator coordinating the ping engine and TUI event loop
- **ping.rs**: Asynchronous ping engine with per-host ICMP operations and DNS resolution
- **tui.rs**: Terminal UI rendering with ratatui for graphs, tables, and real-time updates  
- **stats.rs**: Statistics collection with circular buffers and real-time metrics calculation
- **config.rs**: TOML configuration management for hosts, ping settings, and UI preferences

### Architecture Pattern
The application uses an async event-driven architecture:
1. PingEngine spawns async tasks per enabled host
2. Each task sends PingEvent messages through mpsc channels
3. App coordinates between ping events and UI updates via tokio::select!
4. Stats are maintained in Arc<RwLock<HashMap>> for thread-safe access
5. TUI renders stats at configurable refresh intervals

### Host Management
- Hosts are identified by deterministic UUIDs (uuid::Uuid::new_v5)
- DNS resolution happens per ping loop for dynamic IP handling
- Individual hosts can override global ping intervals
- Host state is managed through enabled/disabled flags

## Development Commands

### Build and Run
```bash
cargo build --release
cargo run -- --config pingpong.toml
cargo run -- --host 8.8.8.8 --host google.com --interval 0.5
```

### Testing
```bash
cargo test
cargo test -- --nocapture  # For test output
```

### Development Tools
```bash
cargo check          # Fast syntax/type checking
cargo clippy         # Linting
cargo fmt            # Code formatting
```

## Configuration

The application uses TOML configuration files (default: `pingpong.toml`):
- **ping**: interval, timeout, history_size, packet_size
- **ui**: refresh_rate, theme, show_details, graph_height  
- **hosts**: array of {name, address, enabled, interval?}

CLI arguments override config file settings.

## Dependencies

Key dependencies and their purposes:
- **ratatui + crossterm**: TUI framework and terminal handling
- **tokio**: Async runtime for concurrent ping operations
- **surge-ping**: ICMP ping implementation 
- **dns-lookup**: Hostname resolution
- **serde + toml**: Configuration serialization
- **clap**: CLI argument parsing