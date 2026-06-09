# PingPong Robustness Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make PingPong trustworthy on bad airplane wifi — survive any panic/Ctrl-C/tiny terminal, recover automatically from DNS/network failures, distinguish offline from captive portal, make every config knob real, show a live status in the terminal title, and get tests + CI green.

**Architecture:** Keep the existing async-per-host + mpsc-event + App-owns-stats skeleton. Add one derived state model (`HostState`, `ConnectivityState`) as the single source of truth for connectivity, computed in pure functions in a new `status.rs`. Extend the ping event to carry resolution status. Add a small captive-portal probe task (`probe.rs`) using plain HTTP over tokio `TcpStream` (no new deps). Thread new UI data through a single `RenderOpts` struct instead of growing the positional arg list.

**Tech Stack:** Rust 2021, tokio, ratatui 0.29, crossterm 0.28, surge-ping 0.8 (defaults to unprivileged DGRAM ICMP and auto-falls-back to RAW), anyhow, serde/toml, clap.

---

## Shared Types (defined where first implemented, referenced thereafter)

These types are created in the tasks noted and reused across the plan. Signatures are repeated at use sites for clarity.

- `HostUpdate` (Task 8, `ping.rs`): event payload — `Resolving`, `ResolveFailed(String)`, `Resolved(IpAddr)`, `Pinged(PingResult)`.
- `HostState` (Task 7, `status.rs`): derived per-host display state — `Resolving`, `Up { rtt_ms: f64 }`, `Degraded { loss_pct: f64 }`, `Down { reason: String }`.
- `ConnectivityState` (Task 7, `status.rs`): derived global state — `Online`, `Degraded`, `CaptivePortal { url: String }`, `Offline`.
- `ProbeResult` (Task 9, `probe.rs`): `Online`, `CaptivePortal { url: String }`, `Offline`.
- `Backoff` (Task 6, `ping.rs`): exponential backoff helper.
- `Theme` (Task 12, `tui.rs`): color palette.
- `RenderOpts` (Task 12, `tui.rs`): bundle of UI options threaded into the renderer.

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src/main.rs` | CLI parse, panic hook install, wiring | Modify |
| `src/config.rs` | Config load/save, theme value, validation, host list | Modify |
| `src/ping.rs` | Per-host loop: resolve/retry/backoff/state, packet payload, events | Modify |
| `src/probe.rs` | Captive-portal HTTP probe task | **Create** |
| `src/status.rs` | Pure derivation of `HostState`/`ConnectivityState`, aggregate, title string | **Create** |
| `src/stats.rs` | Stats math + graph data helper (+ tests) | Modify |
| `src/tui.rs` | Terminal guard, panic-safe teardown, theme, graph, banner, details, title, guards | Modify |
| `src/app.rs` | Event loop, single source of truth, Ctrl-C, title update | Modify |
| `.github/workflows/ci.yml` | CI actions | Modify |

---

# PHASE 1 — Safety Net

*Files: `tui.rs`, `app.rs`, `main.rs`. Verify clippy green; app survives a 1-row terminal and Ctrl-C.*

## Task 1: Make clippy green (fix `manual_is_multiple_of`)

**Files:**
- Modify: `src/tui.rs` (lines ~714, 716, 855, 894, 896 — 7 occurrences across those lines)

- [ ] **Step 1: Confirm the failures**

Run: `cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep -c is_multiple_of`
Expected: `7`

- [ ] **Step 2: Fix each occurrence**

Replace each `<expr> % <n> == 0` with `(<expr>).is_multiple_of(<n>)`. The 7 sites (exact current text — match precisely):

```rust
// tui.rs ~714
if intensity > 2.0 && (x + y + time_int) % 7 == 0 {
// becomes:
if intensity > 2.0 && (x + y + time_int).is_multiple_of(7) {
```
```rust
// tui.rs ~716
} else if intensity > 1.5 && (x * 2 + y + time_int / 2) % 11 == 0 {
// becomes:
} else if intensity > 1.5 && (x * 2 + y + time_int / 2).is_multiple_of(11) {
```
```rust
// tui.rs ~855
if elevation > 4 && (x + y + (time * 2.0) as usize) % 12 == 0 {
// becomes:
if elevation > 4 && (x + y + (time * 2.0) as usize).is_multiple_of(12) {
```
```rust
// tui.rs ~894
let char_to_use = if star_seed % 25 == 0 {
// becomes:
let char_to_use = if star_seed.is_multiple_of(25) {
```
```rust
// tui.rs ~896  (two `% .. == 0` on this line — fix both)
} else if star_seed % 47 == 0 && (time * 1.0) as usize % 15 < 3 {
// becomes:
} else if star_seed.is_multiple_of(47) && (time * 1.0) as usize % 15 < 3 {
```

Note: the `% 15 < 3` on line 896 is NOT a `== 0` check — leave it as `%`. Only the seven `% n == 0` forms change.

- [ ] **Step 3: Verify clippy passes**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: finishes with no errors (exit 0).

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "fix: resolve clippy manual_is_multiple_of errors"
```

## Task 2: Panic-safe terminal teardown + remove debug eprintln

**Files:**
- Modify: `src/tui.rs` (imports; `TuiState::with_animation` line ~74; `TuiApp::new` lines ~145-164; `Drop` impl lines ~1682-1692; add free functions)
- Modify: `src/main.rs` (install panic hook in `main`)

- [ ] **Step 1: Add terminal enter/leave free functions in `tui.rs`**

Add near the top of `tui.rs` (after imports). These are the ONE place that knows how to set up / tear down the terminal. `terminal_enter` pushes the title onto the xterm title stack (`CSI 22;2t`); `terminal_leave` pops it (`CSI 23;2t`) so the user's original title returns.

```rust
use std::io::Write as _;

/// Put the terminal into TUI mode: raw mode, alternate screen, save title.
pub fn terminal_enter() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Save current window title onto the xterm title stack, then enter alt screen.
    write!(stdout, "\x1b[22;2t")?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    stdout.flush()?;
    Ok(())
}

/// Restore the terminal: leave alt screen, disable raw mode, restore title, show cursor.
/// Safe to call multiple times; all errors are ignored so it can run from a panic hook.
pub fn terminal_leave() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);
    let _ = disable_raw_mode();
    // Pop the saved title off the xterm title stack.
    let _ = write!(stdout, "\x1b[23;2t");
    let _ = execute!(stdout, crossterm::cursor::Show);
    let _ = stdout.flush();
}
```

Add `crossterm::cursor` is already reachable via the `crossterm::` path; no new import needed beyond `std::io::Write`.

- [ ] **Step 2: Use the free functions in `TuiApp::new` and `Drop`**

In `TuiApp::new` (lines ~145-151), replace the inline setup:

```rust
pub async fn new(animation_type: Option<AnimationType>) -> anyhow::Result<Self> {
    terminal_enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    // ... rest unchanged (state selection, Ok(Self{...}))
```

Replace the entire `Drop` impl (lines ~1682-1692) with:

```rust
impl Drop for TuiApp {
    fn drop(&mut self) {
        terminal_leave();
    }
}
```

- [ ] **Step 3: Remove the debug eprintln in `with_animation`**

In `tui.rs` `TuiState::with_animation` (line ~74) delete:

```rust
        // Debug: Log which animation was selected
        eprintln!("🎨 Selected animation: {:?}", animation_type);
```

- [ ] **Step 4: Install the panic hook in `main.rs`**

In `src/main.rs`, at the very top of `main()` (before `Cli::parse()`), add a panic hook that restores the terminal before the default hook prints. This covers panics in spawned tasks, which `Drop` alone does not.

```rust
    // Restore the terminal on ANY panic before printing the message,
    // so a crash never leaves the user's shell in raw mode.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tui::terminal_leave();
        default_hook(info);
    }));
```

`tui::terminal_leave` is already public from Step 1. `tui` is already a module in `main.rs`.

- [ ] **Step 5: Verify it builds and runs**

Run: `cargo build`
Expected: builds clean.

Run: `cargo run` then press `q`. Expected: terminal returns to normal, original title restored, cursor visible.

- [ ] **Step 6: Verify panic safety manually**

Temporarily add `panic!("test")` as the first line of `App::run` (in `app.rs`), run `cargo run`, confirm the shell is left usable (not stuck in raw mode), then remove the `panic!`.

- [ ] **Step 7: Commit**

```bash
git add src/tui.rs src/main.rs
git commit -m "feat: panic-safe terminal teardown via shared enter/leave + panic hook"
```

## Task 3: Guard against tiny-terminal panics

**Files:**
- Modify: `src/tui.rs` (the 6 animation generators' `effective_width`/`effective_height` math; `step_by` divisors ~1417/1420; DNA `% effective_*` ~1307/1308; `render_main` `host_count * 8` ~307; `"─".repeat(...)` ~363/387)

- [ ] **Step 1: Add a clamp helper near the top of `tui.rs`**

```rust
/// Smallest safe step for `Iterator::step_by` (which panics on 0).
fn safe_step(n: usize) -> usize {
    n.max(1)
}
```

- [ ] **Step 2: Make every `step_by` divisor safe**

At `tui.rs` ~1417 and ~1420:

```rust
for y in (0..effective_height).step_by(safe_step(effective_height / 4)) {
    for x in (0..effective_width).step_by(safe_step(effective_width / 8)) {
```

- [ ] **Step 3: Guard the DNA modulo-by-size**

At `tui.rs` ~1307-1308, guard against zero size before the `%`:

```rust
let y = ((time * 3.0) as usize + rand::random::<usize>()) % effective_height.max(1);
let x = rand::random::<usize>() % effective_width.max(1);
```

- [ ] **Step 4: Make the `"─".repeat` widths underflow-proof**

At `tui.rs` ~363 and ~387, replace `35 - host_name.len().min(25)` with a saturating form keyed off display width:

```rust
"─".repeat(35usize.saturating_sub(host_name.chars().count().min(25)))
```

(Using `chars().count()` instead of `len()` so multibyte host names don't miscount.)

- [ ] **Step 5: Guard the host-percentage math in `render_main`**

At `tui.rs` ~307, the intermediate `host_count * 8` can overflow for absurd host counts; use saturating math:

```rust
let ping_percentage = std::cmp::min(80usize, 40usize.saturating_add(host_count.saturating_mul(8)));
```

- [ ] **Step 6: Verify build + clippy**

Run: `cargo build && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 7: Verify on a tiny terminal manually**

Resize your terminal very small (a few rows/cols) and run `cargo run` cycling animations with `v` (especially DNA and waveform). Expected: no panic; quit with `q`.

- [ ] **Step 8: Commit**

```bash
git add src/tui.rs
git commit -m "fix: prevent panics on tiny terminals (step_by/modulo/underflow guards)"
```

## Task 4: Robust input + surfaced loop errors (Ctrl-C, Esc)

**Files:**
- Modify: `src/tui.rs` (`handle_events` lines ~224-249; import `KeyModifiers`)
- Modify: `src/app.rs` (`run` loop lines ~62-95; imports)

- [ ] **Step 1: Add Esc + Ctrl-C handling in `handle_events`**

Update the `crossterm::event` import line (~7) to include `KeyModifiers`:

```rust
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
```

In `handle_events` (~227), add Ctrl-C and Esc as quit. Insert before the `KeyCode::Char('q')` arm:

```rust
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(true) // Ctrl-C quits (raw mode swallows SIGINT)
                    }
                    KeyCode::Esc => return Ok(true),
```

- [ ] **Step 2: Stop silently dropping loop errors in `app.rs`**

In `App::run` (~84-89), the current code does `if let Ok(should_quit) = self.tui.handle_events().await`, discarding `Err`. Replace with propagation so input errors surface instead of vanishing:

```rust
                _ = ui_update_interval.tick() => {
                    let stats = self.stats.read().await;
                    self.tui.draw(&stats).await?;
                    drop(stats);
                    if self.tui.handle_events().await? {
                        break;
                    }
                }
```

(Removing the `eprintln!("TUI error...")` paths — errors now propagate out of `run` and are handled by `main`'s `Result`, after the terminal is restored by `Drop`.)

- [ ] **Step 3: Add a Ctrl-C signal arm for the pre-/non-raw window**

Add `use tokio::signal;` near the top of `app.rs`. Add a third branch to the `tokio::select!` in `run`:

```rust
                _ = signal::ctrl_c() => {
                    break;
                }
```

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: clean.

- [ ] **Step 5: Verify manually**

Run `cargo run`; confirm `Esc` quits cleanly and `Ctrl-C` quits cleanly, both restoring the terminal.

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs src/app.rs
git commit -m "feat: quit on Esc/Ctrl-C and surface event-loop errors"
```

---

# PHASE 2 — Network Resilience

*Files: `ping.rs`, `probe.rs`, `status.rs`, `app.rs`, `stats.rs`. Verify state/backoff/status unit tests; manual offline/portal simulation.*

## Task 5: Unit-test the stats math (TDD on existing code)

**Files:**
- Modify: `src/stats.rs` (extend the `#[cfg(test)]` module at end of file)

- [ ] **Step 1: Add a test helper and the failing tests**

Append to the bottom of `src/stats.rs`:

```rust
#[cfg(test)]
mod stats_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn success(ms: u64) -> PingResult {
        PingResult::Success { rtt: Duration::from_millis(ms), sequence: 0, timestamp: Instant::now() }
    }
    fn timeout() -> PingResult {
        PingResult::Timeout { sequence: 0, timestamp: Instant::now() }
    }

    #[test]
    fn empty_stats_are_zero() {
        let s = PingStats::new(100);
        assert_eq!(s.total_pings(), 0);
        assert_eq!(s.packet_loss_percent(), 0.0);
        let r = s.rtt_stats();
        assert_eq!(r.avg, Duration::ZERO);
    }

    #[test]
    fn packet_loss_counts_timeouts_and_errors() {
        let mut s = PingStats::new(100);
        s.add_result(&success(10));
        s.add_result(&success(10));
        s.add_result(&timeout());
        s.add_result(&timeout());
        assert_eq!(s.total_pings(), 4);
        assert!((s.packet_loss_percent() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn recent_loss_uses_only_window() {
        let mut s = PingStats::new(100);
        for _ in 0..10 { s.add_result(&success(10)); }
        for _ in 0..2 { s.add_result(&timeout()); }
        // window of 2 = the two most recent, both timeouts = 100%
        assert!((s.packet_loss_percent_recent(2) - 100.0).abs() < 1e-9);
        // window of 12 = 2/12 lost
        assert!((s.packet_loss_percent_recent(12) - (2.0/12.0*100.0)).abs() < 1e-9);
    }

    #[test]
    fn rtt_min_max_avg_median_odd() {
        let mut s = PingStats::new(100);
        for ms in [10u64, 20, 30] { s.add_result(&success(ms)); }
        let r = s.rtt_stats();
        assert_eq!(r.min, Duration::from_millis(10));
        assert_eq!(r.max, Duration::from_millis(30));
        assert_eq!(r.avg, Duration::from_millis(20));
        assert_eq!(r.median, Duration::from_millis(20));
    }

    #[test]
    fn rtt_median_even() {
        let mut s = PingStats::new(100);
        for ms in [10u64, 20, 30, 40] { s.add_result(&success(ms)); }
        // even count -> mean of the two middle (20,30) = 25
        assert_eq!(s.rtt_stats().median, Duration::from_millis(25));
    }

    #[test]
    fn jitter_zero_for_constant_rtt() {
        let mut s = PingStats::new(100);
        for _ in 0..5 { s.add_result(&success(42)); }
        assert!(s.rtt_stats().jitter < Duration::from_micros(50));
    }

    #[test]
    fn quality_thresholds() {
        let mut good = PingStats::new(100);
        for _ in 0..20 { good.add_result(&success(10)); }
        assert_eq!(good.connection_quality(), ConnectionQuality::Good);

        let mut poor = PingStats::new(100);
        for _ in 0..20 { poor.add_result(&timeout()); }
        assert_eq!(poor.connection_quality(), ConnectionQuality::Poor);
    }

    #[test]
    fn history_is_bounded() {
        let mut s = PingStats::new(3);
        for ms in [1u64, 2, 3, 4, 5] { s.add_result(&success(ms)); }
        // Buffer caps at 3, so the oldest (1,2) are dropped: min over {3,4,5} is 3.
        assert_eq!(s.rtt_stats().min, Duration::from_millis(3));
        assert_eq!(s.total_pings(), 5); // cumulative counter is unaffected by the cap
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test stats_tests`
Expected: all pass. If any FAIL, that is a real bug in `stats.rs` — fix the implementation (not the test) and re-run. (Watch `quality_thresholds`: `connection_quality` mixes recent loss with all-time avg RTT; the test above only asserts the clear Good/Poor extremes, which hold regardless.)

- [ ] **Step 3: Leave dead-code markers alone for now**

These tests only call public methods that already exist (`add_result`, `total_pings`, `rtt_stats`, `packet_loss_percent`, `packet_loss_percent_recent`, `connection_quality`). Leave every `#[allow(dead_code)]` marker as-is — the graph wires up `rtt_history_for_graph` in Task 12.

- [ ] **Step 4: Commit**

```bash
git add src/stats.rs
git commit -m "test: cover stats math (loss, rtt, median, jitter, quality, bounding)"
```

## Task 6: Exponential backoff helper

**Files:**
- Modify: `src/ping.rs` (add `Backoff` struct + tests in the existing `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/ping.rs`:

```rust
    #[test]
    fn backoff_doubles_and_caps_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
        assert_eq!(b.next(), Duration::from_secs(1));
        assert_eq!(b.next(), Duration::from_secs(2));
        assert_eq!(b.next(), Duration::from_secs(4));
        assert_eq!(b.next(), Duration::from_secs(8));
        assert_eq!(b.next(), Duration::from_secs(16));
        assert_eq!(b.next(), Duration::from_secs(30)); // capped (would be 32)
        assert_eq!(b.next(), Duration::from_secs(30)); // stays capped
        b.reset();
        assert_eq!(b.next(), Duration::from_secs(1));
    }
```

- [ ] **Step 2: Run it (fails to compile — `Backoff` undefined)**

Run: `cargo test backoff_doubles_and_caps_and_resets`
Expected: compile error `cannot find type Backoff`.

- [ ] **Step 3: Implement `Backoff` in `ping.rs`**

Add near the top of `ping.rs` (after the imports/`PingEvent`):

```rust
/// Exponential backoff with a cap. `next()` returns the current delay then doubles it.
#[derive(Debug)]
pub struct Backoff {
    current: Duration,
    base: Duration,
    max: Duration,
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self { current: base, base, max }
    }
    pub fn next(&mut self) -> Duration {
        let delay = self.current.min(self.max);
        self.current = (self.current * 2).min(self.max);
        delay
    }
    pub fn reset(&mut self) {
        self.current = self.base;
    }
}
```

- [ ] **Step 4: Run it (passes)**

Run: `cargo test backoff_doubles_and_caps_and_resets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ping.rs
git commit -m "feat: add exponential backoff helper with tests"
```

## Task 7: `status.rs` — pure connectivity derivation + title formatting

**Files:**
- Create: `src/status.rs`
- Modify: `src/main.rs` (add `mod status;`)

- [ ] **Step 1: Create `src/status.rs` with the failing tests first**

```rust
// ABOUTME: Pure derivation of per-host and global connectivity state and the
// ABOUTME: terminal-title summary string. No I/O — fully unit-testable.

use crate::probe::ProbeResult;
use crate::stats::PingStats;

/// Per-host display state, derived from stats + last resolution status.
#[derive(Debug, Clone, PartialEq)]
pub enum HostState {
    Resolving,
    Up { rtt_ms: f64 },
    Degraded { loss_pct: f64 },
    Down { reason: String },
}

/// Global connectivity, derived from all host states + the portal probe.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectivityState {
    Online,
    Degraded,
    CaptivePortal { url: String },
    Offline,
}

/// Aggregate numbers for the title/banner.
#[derive(Debug, Clone, PartialEq)]
pub struct Aggregate {
    pub hosts_up: usize,
    pub hosts_total: usize,
    pub avg_rtt_ms: f64,
    pub worst_loss_pct: f64,
}

/// Derive a host's state from its stats and whether it currently has an IP.
/// `resolved` is false while DNS is failing/backing off.
pub fn host_state(stats: Option<&PingStats>, resolved: bool, resolve_error: Option<&str>) -> HostState {
    if let Some(err) = resolve_error {
        return HostState::Down { reason: format!("dns: {err}") };
    }
    if !resolved {
        return HostState::Resolving;
    }
    match stats {
        None => HostState::Resolving,
        Some(s) if s.total_pings() == 0 => HostState::Resolving,
        Some(s) => {
            let loss = s.packet_loss_percent_recent(20);
            if loss >= 100.0 {
                HostState::Down { reason: "no replies".to_string() }
            } else if loss > 2.0 {
                HostState::Degraded { loss_pct: loss }
            } else {
                HostState::Up { rtt_ms: s.rtt_stats().avg.as_secs_f64() * 1000.0 }
            }
        }
    }
}

/// Derive global connectivity from host states and the latest probe result.
pub fn connectivity(states: &[HostState], probe: &ProbeResult) -> ConnectivityState {
    if let ProbeResult::CaptivePortal { url } = probe {
        return ConnectivityState::CaptivePortal { url: url.clone() };
    }
    let up = states.iter().filter(|s| matches!(s, HostState::Up { .. })).count();
    let any_traffic = states.iter().any(|s| matches!(s, HostState::Up { .. } | HostState::Degraded { .. }));
    if up == states.len() && !states.is_empty() {
        ConnectivityState::Online
    } else if any_traffic {
        ConnectivityState::Degraded
    } else {
        // No host is passing traffic; the probe (Offline here, since CaptivePortal
        // was handled above) confirms we are dark.
        ConnectivityState::Offline
    }
}

pub fn aggregate(states: &[HostState]) -> Aggregate {
    let hosts_total = states.len();
    let hosts_up = states.iter().filter(|s| matches!(s, HostState::Up { .. })).count();
    let up_rtts: Vec<f64> = states.iter().filter_map(|s| match s {
        HostState::Up { rtt_ms } => Some(*rtt_ms),
        _ => None,
    }).collect();
    let avg_rtt_ms = if up_rtts.is_empty() { 0.0 } else { up_rtts.iter().sum::<f64>() / up_rtts.len() as f64 };
    let worst_loss_pct = states.iter().filter_map(|s| match s {
        HostState::Degraded { loss_pct } => Some(*loss_pct),
        HostState::Down { .. } => Some(100.0),
        _ => None,
    }).fold(0.0_f64, f64::max);
    Aggregate { hosts_up, hosts_total, avg_rtt_ms, worst_loss_pct }
}

/// Build the terminal-title string: symbol + ratio + most-relevant metric.
pub fn title(conn: &ConnectivityState, agg: &Aggregate) -> String {
    match conn {
        ConnectivityState::Online => {
            format!("\u{25cf}  pingpong  {}/{} up \u{b7} {:.0}ms", agg.hosts_up, agg.hosts_total, agg.avg_rtt_ms)
        }
        ConnectivityState::Degraded => {
            format!("\u{25d0}  pingpong  {}/{} up \u{b7} {:.0}% loss", agg.hosts_up, agg.hosts_total, agg.worst_loss_pct)
        }
        ConnectivityState::CaptivePortal { .. } => {
            "\u{26a0}  pingpong  captive portal \u{2014} log in".to_string()
        }
        ConnectivityState::Offline => "\u{2717}  pingpong  offline".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::PingResult;
    use std::time::{Duration, Instant};

    fn stats_with(successes: usize, timeouts: usize, ms: u64) -> PingStats {
        let mut s = PingStats::new(100);
        for _ in 0..successes {
            s.add_result(&PingResult::Success { rtt: Duration::from_millis(ms), sequence: 0, timestamp: Instant::now() });
        }
        for _ in 0..timeouts {
            s.add_result(&PingResult::Timeout { sequence: 0, timestamp: Instant::now() });
        }
        s
    }

    #[test]
    fn resolving_when_no_pings_yet() {
        assert_eq!(host_state(None, true, None), HostState::Resolving);
    }

    #[test]
    fn down_when_dns_failed() {
        assert_eq!(
            host_state(None, false, Some("no address")),
            HostState::Down { reason: "dns: no address".to_string() }
        );
    }

    #[test]
    fn up_when_healthy() {
        let s = stats_with(20, 0, 30);
        assert_eq!(host_state(Some(&s), true, None), HostState::Up { rtt_ms: 30.0 });
    }

    #[test]
    fn degraded_with_some_loss() {
        let s = stats_with(18, 2, 30); // 10% recent loss
        assert!(matches!(host_state(Some(&s), true, None), HostState::Degraded { .. }));
    }

    #[test]
    fn portal_probe_wins() {
        let states = vec![HostState::Up { rtt_ms: 10.0 }];
        let conn = connectivity(&states, &ProbeResult::CaptivePortal { url: "http://x".into() });
        assert_eq!(conn, ConnectivityState::CaptivePortal { url: "http://x".into() });
    }

    #[test]
    fn online_when_all_up() {
        let states = vec![HostState::Up { rtt_ms: 10.0 }, HostState::Up { rtt_ms: 20.0 }];
        assert_eq!(connectivity(&states, &ProbeResult::Online), ConnectivityState::Online);
    }

    #[test]
    fn offline_when_all_down_and_probe_offline() {
        let states = vec![HostState::Down { reason: "x".into() }];
        assert_eq!(connectivity(&states, &ProbeResult::Offline), ConnectivityState::Offline);
    }

    #[test]
    fn title_strings_match_states() {
        let agg = Aggregate { hosts_up: 3, hosts_total: 3, avg_rtt_ms: 42.0, worst_loss_pct: 0.0 };
        assert!(title(&ConnectivityState::Online, &agg).contains("3/3 up"));
        assert!(title(&ConnectivityState::Online, &agg).contains("42ms"));
        assert!(title(&ConnectivityState::CaptivePortal { url: "x".into() }, &agg).contains("captive portal"));
        assert!(title(&ConnectivityState::Offline, &agg).contains("offline"));
    }
}
```

- [ ] **Step 2: Register the module**

This task depends on `probe::ProbeResult` (Task 9). To compile `status.rs` now, also add `mod status;` AND `mod probe;` to `src/main.rs` and create a minimal `probe.rs` stub first (full impl in Task 9):

```rust
// src/probe.rs (stub — full implementation in Task 9)
// ABOUTME: Captive-portal connectivity probe over plain HTTP.
// ABOUTME: Classifies network as Online, CaptivePortal, or Offline.

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Online,
    CaptivePortal { url: String },
    Offline,
}
```

Add to `src/main.rs` module list:

```rust
mod probe;
mod status;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test status::`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/status.rs src/probe.rs src/main.rs
git commit -m "feat: pure connectivity-state derivation and title formatting with tests"
```

## Task 8: Ping loop rework — lazy resolve, backoff, state events, packet payload

**Files:**
- Modify: `src/ping.rs` (`PingEvent`, `PingEngine::new`, `ping_host_loop`, remove dead stats map)
- Modify: `src/app.rs` (track per-host resolution status; populate stats from events)

- [ ] **Step 1: Extend the event payload**

Replace the `PingEvent` struct (~17-23) and add `HostUpdate`:

```rust
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub enum HostUpdate {
    Resolving,
    ResolveFailed(String),
    Resolved(IpAddr),
    Pinged(PingResult),
}

#[derive(Debug, Clone)]
pub struct PingEvent {
    pub host_id: String,
    pub host_name: String,
    pub update: HostUpdate,
}
```

- [ ] **Step 2: Make `PingEngine::new` non-fatal (lazy resolve) and drop the dead stats map**

Replace `PingEngine::new` (~34-70). It no longer resolves DNS (so no host can kill startup) and no longer keeps a stats map (App owns stats — single source of truth). It also no longer creates clients up front (the loop owns its client):

```rust
pub struct PingEngine {
    hosts: Vec<Host>,
    event_tx: mpsc::Sender<PingEvent>,
    ping_config: crate::config::PingConfig,
}

impl PingEngine {
    pub fn new(
        hosts: Vec<Host>,
        ping_config: crate::config::PingConfig,
        event_tx: mpsc::Sender<PingEvent>,
    ) -> Self {
        Self { hosts, event_tx, ping_config }
    }
```

Remove `use surge_ping::{...}` items no longer needed only if unused; `Client`, `Config as SurgePingConfig`, `PingIdentifier`, `PingSequence` are still used in the loop. Remove `use std::collections::HashMap;`, `use std::sync::Arc;`, `use tokio::sync::RwLock;` if they become unused (the dead stats map used them). Keep what the loop needs.

- [ ] **Step 3: Rework `ping_host_loop` for resolve/retry/backoff/payload/state**

Replace `ping_host_loop` (~104-178) with a version that: emits `Resolving`, resolves with backoff (re-resolving on repeated ping failure), sends a `packet_size` payload, and emits `HostUpdate` events instead of writing a stats map:

```rust
async fn ping_host_loop(
    host: Host,
    event_tx: mpsc::Sender<PingEvent>,
    ping_config: crate::config::PingConfig,
) {
    let host_id = Self::generate_host_id(&host.address);
    let interval = Duration::from_secs_f64(host.interval.unwrap_or(ping_config.interval));
    let timeout = Duration::from_secs_f64(ping_config.timeout);
    let payload = vec![0u8; ping_config.packet_size as usize];

    let send = |update: HostUpdate| {
        let _ = event_tx.try_send(PingEvent {
            host_id: host_id.clone(),
            host_name: host.name.clone(),
            update,
        });
    };

    let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
    let mut sequence = 0u16;

    loop {
        // (Re)resolve with backoff until success.
        send(HostUpdate::Resolving);
        let ip_addr = loop {
            match Self::resolve_hostname(&host.address).await {
                Ok(ip) => { backoff.reset(); break ip; }
                Err(e) => {
                    send(HostUpdate::ResolveFailed(e.to_string()));
                    tokio::time::sleep(backoff.next()).await;
                }
            }
        };
        send(HostUpdate::Resolved(ip_addr));

        // Build a client; if sockets are denied even after surge-ping's
        // DGRAM->RAW fallback, report it and back off (don't spin).
        let client = match Client::new(&SurgePingConfig::default()) {
            Ok(c) => c,
            Err(e) => {
                send(HostUpdate::ResolveFailed(format!(
                    "icmp socket denied ({e}); on Linux set net.ipv4.ping_group_range or run elevated"
                )));
                tokio::time::sleep(backoff.next()).await;
                continue;
            }
        };

        // Ping at the configured interval. After several consecutive failures,
        // break out to re-resolve (handles IP changes / reconnects).
        // Create the pinger ONCE and reuse it — this is surge-ping's intended use
        // and avoids a redundant double-timeout. It enforces `pinger.timeout` itself
        // and returns Err(SurgeError::Timeout) when a reply does not arrive in time.
        let mut pinger = client.pinger(ip_addr, PingIdentifier(0)).await;
        pinger.timeout(timeout);
        let mut interval_timer = tokio::time::interval(interval);
        let mut consecutive_failures = 0u32;
        loop {
            interval_timer.tick().await;
            let start_time = Instant::now();

            let result = match pinger.ping(PingSequence(sequence), &payload).await {
                Ok((_, rtt)) => {
                    consecutive_failures = 0;
                    PingResult::Success { rtt, sequence, timestamp: start_time }
                }
                Err(surge_ping::SurgeError::Timeout { .. }) => {
                    consecutive_failures += 1;
                    PingResult::Timeout { sequence, timestamp: start_time }
                }
                Err(e) => {
                    consecutive_failures += 1;
                    PingResult::Error { error: e.to_string(), sequence, timestamp: start_time }
                }
            };

            if event_tx.try_send(PingEvent {
                host_id: host_id.clone(),
                host_name: host.name.clone(),
                update: HostUpdate::Pinged(result),
            }).is_err() {
                // Channel full is fine (UI drops a frame); channel closed = exit.
                if event_tx.is_closed() { return; }
            }

            sequence = sequence.wrapping_add(1);
            if consecutive_failures >= 5 { break; } // re-resolve
        }
    }
}
```

- [ ] **Step 4: Update `start` to match the new loop signature**

In `PingEngine::start` (~72-102), remove the `clients`/`stats` clones; spawn the loop with just `(host, event_tx, ping_config)`:

```rust
pub async fn start(&self) -> Result<()> {
    let mut handles = Vec::new();
    for host in &self.hosts {
        if !host.enabled { continue; }
        let host_clone = host.clone();
        let event_tx = self.event_tx.clone();
        let ping_config = self.ping_config.clone();
        handles.push(tokio::spawn(async move {
            Self::ping_host_loop(host_clone, event_tx, ping_config).await
        }));
    }
    for handle in handles {
        let _ = handle.await; // task panics already restore the terminal via panic hook
    }
    Ok(())
}
```

Delete `get_stats` (dead) and keep `get_host_info` and `generate_host_id`. Update the `test_ping_engine_creation` test: `PingEngine::new` is now synchronous and returns `Self` (not `Result`):

```rust
    #[test]
    fn test_ping_engine_creation() {
        let hosts = vec![Host { name: "localhost".into(), address: "127.0.0.1".into(), enabled: true, interval: None }];
        let ping_config = PingConfig { interval: 1.0, timeout: 5.0, history_size: 100, packet_size: 64 };
        let (tx, _rx) = mpsc::channel(64);
        let _engine = PingEngine::new(hosts, ping_config, tx);
    }
```

- [ ] **Step 5: Gate the live-DNS test**

Mark the network-dependent assertion `#[ignore]` (opt-in; fails on a plane otherwise). Keep the deterministic IP path as a normal test:

```rust
    #[tokio::test]
    async fn test_ip_parse_fast_path() {
        assert!(PingEngine::resolve_hostname("127.0.0.1").await.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires live DNS; run with --ignored"]
    async fn test_hostname_resolution_live() {
        assert!(PingEngine::resolve_hostname("localhost").await.is_ok());
    }
```

- [ ] **Step 6: Update `app.rs` to own state from events**

In `app.rs`, change the channel to bounded and track per-host resolution status alongside stats. Replace `App`'s fields and `new`/`handle_ping_event`:

```rust
use std::collections::HashMap;
use crate::ping::{HostUpdate, PingEngine, PingEvent};
use crate::probe::ProbeResult;

pub struct App {
    config: Config,
    tui: TuiApp,
    stats: HashMap<String, PingStats>,
    resolved: HashMap<String, bool>,
    resolve_err: HashMap<String, Option<String>>,
    portal: ProbeResult,
    event_rx: mpsc::Receiver<PingEvent>,
    host_info: Vec<(String, String)>,
}
```

In `App::new`, use `mpsc::channel(1024)` and `PingEngine::new(...)` (now sync). Spawn the engine as before. Initialize the new maps empty and `portal: ProbeResult::Offline`. (`stats` no longer needs `Arc<RwLock>` because only the App task touches it now — single owner.)

Replace `handle_ping_event`:

```rust
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
            let entry = self.stats.entry(event.host_id.clone())
                .or_insert_with(|| PingStats::new(self.config.ping.history_size));
            entry.add_result(&result);
        }
    }
}
```

In `run`, the `draw` call now takes `&self.stats` directly (a plain `&HashMap`), no `.read().await`. Adjust the select arm:

```rust
                _ = ui_update_interval.tick() => {
                    self.tui.draw(&self.stats).await?;
                    if self.tui.handle_events().await? { break; }
                }
```

(The richer snapshot — resolution status + portal — is wired into the renderer in Phase 3 Task 14; for now `draw` keeps its current `&HashMap<String, PingStats>` signature so this task compiles and runs.)

- [ ] **Step 7: Build, test, run**

Run: `cargo build && cargo test`
Expected: builds; all non-ignored tests pass.

Run: `cargo run` — confirm pings still work and quitting is clean. Then test recovery: start with wifi off (all hosts show failures, no crash, no spam), turn wifi on, confirm hosts recover on their own.

- [ ] **Step 8: Commit**

```bash
git add src/ping.rs src/app.rs
git commit -m "feat: resilient ping loop (lazy resolve, backoff, recovery, payload, state events)"
```

## Task 9: Captive-portal probe (`probe.rs`)

**Files:**
- Modify: `src/probe.rs` (replace the Task 7 stub with the full probe)
- Modify: `src/config.rs` (add `portal_check_url` to `PingConfig` with a default)
- Modify: `src/app.rs` (spawn the probe, store latest `ProbeResult`)

- [ ] **Step 1: Add a config field for the probe URL**

In `src/config.rs`, add to `PingConfig` (with serde default so old config files still load):

```rust
    /// URL used to detect captive portals (plain HTTP; default Apple's endpoint).
    #[serde(default = "default_portal_url")]
    pub portal_check_url: String,
```

Add the default fn and include it in `Config::default`'s `PingConfig`:

```rust
fn default_portal_url() -> String { "http://captive.apple.com".to_string() }
```

(In `Config::default`, set `portal_check_url: default_portal_url()`.)

- [ ] **Step 2: Replace the `probe.rs` stub with the real probe + a parser test**

```rust
// ABOUTME: Captive-portal connectivity probe over plain HTTP.
// ABOUTME: Classifies the network as Online, CaptivePortal, or Offline.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Online,
    CaptivePortal { url: String },
    Offline,
}

/// Classify an HTTP status line + body length from a known captive-check endpoint.
/// Apple's `captive.apple.com` returns 200 with the exact body "Success" when open;
/// a portal returns a redirect (3xx) or a different 200 body.
fn classify(status: u16, body_is_success: bool, probe_url: &str) -> ProbeResult {
    match status {
        200 if body_is_success => ProbeResult::Online,
        200 => ProbeResult::CaptivePortal { url: probe_url.to_string() },
        300..=399 => ProbeResult::CaptivePortal { url: probe_url.to_string() },
        _ => ProbeResult::Offline,
    }
}

/// Parse `host` and `path` from a plain `http://host[/path]` URL. Returns None for https/other.
fn parse_http_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    };
    Some((host, path))
}

/// Perform one probe. Any connect/read failure or DNS failure => Offline.
pub async fn probe_once(url: &str) -> ProbeResult {
    let Some((host, path)) = parse_http_url(url) else { return ProbeResult::Offline };
    let addr = format!("{host}:80");
    let fut = async {
        let mut stream = TcpStream::connect(&addr).await.ok()?;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: pingpong\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.ok()?;
        let mut buf = Vec::new();
        // Cap the read so a portal that streams a big login page can't hang us.
        let mut limited = stream.take(8192);
        limited.read_to_end(&mut buf).await.ok()?;
        let text = String::from_utf8_lossy(&buf);
        let status = text.lines().next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse::<u16>().ok())?;
        let body_is_success = text.contains("Success");
        Some(classify(status, body_is_success, url))
    };
    match tokio::time::timeout(Duration::from_secs(5), fut).await {
        Ok(Some(r)) => r,
        _ => ProbeResult::Offline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_online_on_success_body() {
        assert_eq!(classify(200, true, "u"), ProbeResult::Online);
    }
    #[test]
    fn classify_portal_on_redirect() {
        assert_eq!(classify(302, false, "u"), ProbeResult::CaptivePortal { url: "u".into() });
    }
    #[test]
    fn classify_portal_on_unexpected_200() {
        assert_eq!(classify(200, false, "u"), ProbeResult::CaptivePortal { url: "u".into() });
    }
    #[test]
    fn parse_url_splits_host_and_path() {
        assert_eq!(parse_http_url("http://captive.apple.com"), Some(("captive.apple.com".into(), "/".into())));
        assert_eq!(parse_http_url("http://h/x"), Some(("h".into(), "/x".into())));
        assert_eq!(parse_http_url("https://h"), None);
    }
}
```

- [ ] **Step 3: Spawn the probe loop in `app.rs` and store results**

Add a `ProbeStatus` event channel OR reuse a simpler approach: spawn a task that updates a shared value. To stay consistent with the single-owner model, send probe results through a small dedicated channel and select on it. In `App::new`:

```rust
    let (probe_tx, probe_rx) = mpsc::channel::<ProbeResult>(8);
    let portal_url = config.ping.portal_check_url.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            let r = crate::probe::probe_once(&portal_url).await;
            if probe_tx.send(r).await.is_err() { break; }
        }
    });
```

Store `probe_rx` on `App`, and add a select arm in `run`:

```rust
                Some(p) = self.probe_rx.recv() => { self.portal = p; }
```

- [ ] **Step 4: Build + test**

Run: `cargo build && cargo test probe::`
Expected: builds; probe tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/probe.rs src/config.rs src/app.rs
git commit -m "feat: captive-portal probe over plain HTTP with classification tests"
```

---

# PHASE 3 — Honest Config + Features

*Files: `config.rs`, `main.rs`, `tui.rs`. Verify feature tests + manual.*

## Task 10: Config tests, IPv6-safe `add_host`, validation, CLI interval Option

**Files:**
- Modify: `src/config.rs` (`add_host`, add `validate`, tests)
- Modify: `src/main.rs` (`interval: Option<f64>`)

- [ ] **Step 1: Write failing config tests**

Append to `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_hosts() {
        let c = Config::default();
        assert!(!c.hosts.is_empty());
        assert_eq!(c.ping.portal_check_url, "http://captive.apple.com");
    }

    #[test]
    fn add_host_names_ipv4() {
        let mut c = Config { ping: Config::default().ping, hosts: vec![], ui: Config::default().ui };
        c.add_host("8.8.8.8".to_string());
        assert_eq!(c.hosts[0].name, "IP 8.8.8.8");
    }

    #[test]
    fn add_host_keeps_hostname_and_ipv6() {
        let mut c = Config { ping: Config::default().ping, hosts: vec![], ui: Config::default().ui };
        c.add_host("example.com".to_string());
        c.add_host("2606:4700:4700::1111".to_string());
        assert_eq!(c.hosts[0].name, "example.com");
        // IPv6 must NOT be misclassified/renamed oddly; name == address is fine.
        assert_eq!(c.hosts[1].name, "2606:4700:4700::1111");
    }

    #[test]
    fn validate_clamps_absurd_values() {
        let mut c = Config::default();
        c.ping.interval = 0.0;
        c.ping.timeout = 0.0;
        c.ping.history_size = 0;
        c.validate();
        assert!(c.ping.interval >= 0.1);
        assert!(c.ping.timeout >= 0.1);
        assert!(c.ping.history_size >= 1);
    }
}
```

- [ ] **Step 2: Fix `add_host` to use `IpAddr` parsing (handles IPv6) and add `validate`**

Replace `add_host` (~114-127). Only bare IPv4 literals get the `"IP "` label; hostnames and IPv6 addresses keep their address as the name (an IPv6 literal with an `"IP "` prefix reads badly, and the test pins this):

```rust
pub fn add_host(&mut self, address: String) {
    use std::net::IpAddr;
    let name = match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => format!("IP {address}"),
        _ => address.clone(), // hostname or IPv6 -> use as-is
    };
    self.hosts.push(Host { name, address, enabled: true, interval: None });
}

/// Clamp nonsensical values so a hand-edited config can't wedge the app.
pub fn validate(&mut self) {
    if !(self.ping.interval >= 0.1) { self.ping.interval = 1.0; }
    if !(self.ping.timeout >= 0.1) { self.ping.timeout = 3.0; }
    if self.ping.history_size == 0 { self.ping.history_size = 300; }
    if self.ping.packet_size == 0 { self.ping.packet_size = 32; }
    if self.ui.refresh_rate == 0 { self.ui.refresh_rate = 100; }
}
```

- [ ] **Step 3: Call `validate` after load/merge in `main.rs` and make interval an Option**

In `src/main.rs`, change the CLI field:

```rust
    /// Ping interval in seconds (overrides config when set)
    #[arg(short, long)]
    interval: Option<f64>,
```

And the override logic:

```rust
    if let Some(interval) = cli.interval {
        config.set_interval(interval);
    }
    config.validate();
```

- [ ] **Step 4: Run tests**

Run: `cargo test config::tests`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: config validation, IPv6-safe add_host, optional CLI interval"
```

## Task 11: Theme + `RenderOpts` plumbing

**Files:**
- Modify: `src/tui.rs` (add `Theme`, `RenderOpts`; thread through `draw`/`render_main`; `t` key)
- Modify: `src/config.rs` (`theme` already exists; add a parser `Theme::from_config`)

- [ ] **Step 1: Add `Theme` and `RenderOpts` to `tui.rs`**

```rust
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub dim: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self { fg: Color::Green, accent: Color::Cyan, good: Color::Green, warn: Color::Yellow, bad: Color::Red, dim: Color::DarkGray }
    }
    pub fn light() -> Self {
        Self { fg: Color::Black, accent: Color::Blue, good: Color::Green, warn: Color::Rgb(180,120,0), bad: Color::Red, dim: Color::Gray }
    }
    /// "dark" | "light" | "auto" (auto uses COLORFGBG when present, else dark).
    pub fn from_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "dark" => Self::dark(),
            _ => {
                // auto: COLORFGBG like "15;0" => light bg (last field high) -> light theme
                match std::env::var("COLORFGBG").ok().and_then(|v| v.rsplit(';').next().map(|s| s.to_string())) {
                    Some(bg) if bg.trim().parse::<u8>().map(|n| n >= 7).unwrap_or(false) => Self::light(),
                    _ => Self::dark(),
                }
            }
        }
    }
    pub fn cycle_name(name: &str) -> &'static str {
        match name { "dark" => "light", "light" => "auto", _ => "dark" }
    }
}

pub struct RenderOpts {
    pub theme: Theme,
    pub show_details: bool,
    pub graph_height: u16,
    pub banner: Option<String>, // connectivity banner text (portal/offline)
    pub host_states: Vec<(String, crate::status::HostState)>, // (host_id, state)
}
```

- [ ] **Step 2: Store theme name + show_details + graph_height + opts source in `TuiState`**

Add to `TuiState` fields:

```rust
    pub theme_name: String,
    pub show_details: bool,
    pub graph_height: u16,
```

Add a setter on `TuiApp` so `app.rs` can pass config in:

```rust
pub fn set_ui_config(&mut self, theme_name: String, show_details: bool, graph_height: u16) {
    self.state.theme_name = theme_name;
    self.state.show_details = show_details;
    self.state.graph_height = graph_height;
}
```

Initialize the three new fields in `with_animation` (e.g. `theme_name: "auto".into()`, `show_details: true`, `graph_height: 10`).

- [ ] **Step 3: Add the `t` (theme) and `d` (details) keys to `handle_events`**

```rust
                    KeyCode::Char('t') => {
                        let next = Theme::cycle_name(&self.state.theme_name).to_string();
                        self.state.theme_name = next;
                    }
                    KeyCode::Char('d') => {
                        self.state.show_details = !self.state.show_details;
                    }
```

- [ ] **Step 4: Add a `Theme::from_name` unit test**

```rust
#[cfg(test)]
mod theme_tests {
    use super::*;
    #[test]
    fn cycle_wraps() {
        assert_eq!(Theme::cycle_name("dark"), "light");
        assert_eq!(Theme::cycle_name("light"), "auto");
        assert_eq!(Theme::cycle_name("auto"), "dark");
    }
}
```

- [ ] **Step 5: Build + test**

Run: `cargo build && cargo test theme_tests`
Expected: pass. (Renderer still uses the old hardcoded `Color::Green` until Task 12 — that is fine; this task only adds the machinery and keys.)

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs
git commit -m "feat: theme palettes + render-options plumbing + t/d keybindings"
```

## Task 12: Latency graph + connectivity banner + details toggle in the pings window

**Files:**
- Modify: `src/tui.rs` (`draw` signature, `render_main`, `render_pings_window`)
- Modify: `src/app.rs` (build `RenderOpts` from state and pass it into `draw`)
- Modify: `src/stats.rs` (repurpose the dead `rtt_history_for_graph` to return `Vec<Option<u64>>` for the sparkline)

- [ ] **Step 1: Repurpose the existing `rtt_history_for_graph` to feed the sparkline**

The spec calls for wiring the currently-dead `rtt_history_for_graph` (it carries `#[allow(dead_code)]` at `stats.rs:209`). It already walks the circular buffer — **keep that buffer iteration** and change only the per-element mapping and the return type so it yields the most-recent `points` RTTs as **milliseconds**, oldest→newest, with `None` for timeouts/errors. `ratatui::widgets::Sparkline` accepts `Vec<Option<u64>>` (each `Option<u64>: Into<SparklineBar>`, and `None` renders as a gap). Target shape:

```rust
/// Recent RTTs in milliseconds for the sparkline, oldest→newest, at most `points`.
/// `None` marks a gap (timeout/error). Called by the per-host graph in render.
pub fn rtt_history_for_graph(&self, points: usize) -> Vec<Option<u64>> {
    // Preserve the existing buffer iteration; map each result and reverse to
    // oldest→newest. `<buffer>` is whatever field the current method already reads.
    let mut v: Vec<Option<u64>> = self
        .<buffer>
        .iter()
        .rev()
        .take(points)
        .map(|r| match r {
            PingResult::Success { rtt, .. } => Some((rtt.as_secs_f64() * 1000.0) as u64),
            _ => None,
        })
        .collect();
    v.reverse();
    v
}
```

Replace `<buffer>` with the actual circular-buffer field name used by the current `rtt_history_for_graph` body (read it first). Remove the method's `#[allow(dead_code)]` — render now calls it.

- [ ] **Step 2: Thread `RenderOpts` through `draw` and `render_main`**

Change `TuiApp::draw` signature to accept opts:

```rust
pub async fn draw(&mut self, stats: &HashMap<String, PingStats>, opts: &RenderOpts) -> anyhow::Result<()> {
```

Pass `opts` into the `render_main(...)` call (add it as the final argument). Update `render_main`'s signature to take `opts: &RenderOpts` and pass it to `render_pings_window`. Replace the body's hardcoded base style usages with `opts.theme`.

- [ ] **Step 3: Render the banner**

At the top of `render_main`, when `opts.banner` is `Some(text)`, carve a 1-row strip above everything and render it. Replace the outer layout:

```rust
    let size = f.area();
    let (banner_area, body_area) = if let Some(_) = &opts.banner {
        let chunks = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)]).split(size);
        (Some(chunks[0]), chunks[1])
    } else { (None, size) };

    if let (Some(area), Some(text)) = (banner_area, opts.banner.as_ref()) {
        let p = Paragraph::new(text.clone())
            .style(Style::default().fg(opts.theme.warn).add_modifier(ratatui::style::Modifier::BOLD));
        f.render_widget(p, area);
    }
```

Then build the existing `outer_chunks` from `body_area` instead of `size`. (Add `use ratatui::style::Modifier;` or use the fully-qualified path as above.)

- [ ] **Step 4: Rewrite `render_pings_window` to use per-host rows with a sparkline**

Replace the single-`Paragraph` body with a vertical layout: one chunk per host; each chunk shows a header line (state-colored) plus, when `show_details`, a `Sparkline` of height `graph_height`. Import the widget: add `Sparkline` to the `ratatui::widgets` use (`widgets::{Block, Borders, Paragraph, Sparkline}`).

```rust
fn render_pings_window(
    f: &mut Frame,
    area: Rect,
    stats: &HashMap<String, PingStats>,
    host_info: &[(String, String)],
    opts: &RenderOpts,
) {
    let outer = Block::default().borders(Borders::ALL).title(" Network Status ");
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if host_info.is_empty() { return; }

    // Rows: header line + (optional) graph_height for the sparkline.
    let graph_h = if opts.show_details { opts.graph_height.max(1) } else { 0 };
    let per_host = 2 + graph_h; // 1 header + 1 detail line + graph
    let constraints: Vec<Constraint> = host_info.iter()
        .map(|_| Constraint::Length(per_host)).collect();
    let rows = Layout::default().direction(Direction::Vertical)
        .constraints(constraints).split(inner);

    for (row, (host_id, host_name)) in rows.iter().zip(host_info.iter()) {
        let state = opts.host_states.iter()
            .find(|(id, _)| id == host_id).map(|(_, s)| s.clone());
        let (symbol, color, detail) = match &state {
            Some(crate::status::HostState::Up { rtt_ms }) =>
                ("\u{25cf}", opts.theme.good, format!("{rtt_ms:.0}ms")),
            Some(crate::status::HostState::Degraded { loss_pct }) =>
                ("\u{25d0}", opts.theme.warn, format!("{loss_pct:.0}% loss")),
            Some(crate::status::HostState::Down { reason }) =>
                ("\u{2717}", opts.theme.bad, format!("down: {reason}")),
            _ => ("\u{25cb}", opts.theme.dim, "resolving\u{2026}".to_string()),
        };

        let sub = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)]).split(*row);

        let header = Paragraph::new(format!("{symbol} {host_name}\n   {detail}"))
            .style(Style::default().fg(color));
        f.render_widget(header, sub[0]);

        if opts.show_details {
            if let Some(stat) = stats.get(host_id) {
                // Option<u64> preserves gaps (None) for timeouts/errors.
                let spark = Sparkline::default()
                    .data(stat.rtt_history_for_graph(sub[1].width as usize))
                    .style(Style::default().fg(opts.theme.accent));
                f.render_widget(spark, sub[1]);
            }
        }
    }
}
```

- [ ] **Step 5: Build `RenderOpts` in `app.rs` and pass it to `draw`**

In `App::run`, before drawing, derive host states + connectivity + banner using `status.rs`, then pass `RenderOpts`:

```rust
use crate::status::{self, ConnectivityState, HostState};
// ...
                _ = ui_update_interval.tick() => {
                    let host_states: Vec<(String, HostState)> = self.host_info.iter().map(|(id, _)| {
                        let resolved = *self.resolved.get(id).unwrap_or(&false);
                        let err = self.resolve_err.get(id).and_then(|o| o.as_deref());
                        (id.clone(), status::host_state(self.stats.get(id), resolved, err))
                    }).collect();
                    let states: Vec<HostState> = host_states.iter().map(|(_, s)| s.clone()).collect();
                    let conn = status::connectivity(&states, &self.portal);
                    let banner = match &conn {
                        ConnectivityState::CaptivePortal { url } => Some(format!("\u{26a0}  Captive portal detected \u{2014} open {url}")),
                        ConnectivityState::Offline => Some("\u{2717}  Offline \u{2014} no connectivity".to_string()),
                        _ => None,
                    };
                    let opts = crate::tui::RenderOpts {
                        theme: crate::tui::Theme::from_name(&self.tui.theme_name()),
                        show_details: self.tui.show_details(),
                        graph_height: self.config.ui.graph_height,
                        banner,
                        host_states,
                    };
                    self.tui.draw(&self.stats, &opts).await?;
                    if self.tui.handle_events().await? { break; }
                }
```

Add small getters on `TuiApp`: `pub fn theme_name(&self) -> String { self.state.theme_name.clone() }`, `pub fn show_details(&self) -> bool { self.state.show_details }`.

Wire `set_ui_config` once in `App::new` after creating the TUI:

```rust
    tui.set_ui_config(config.ui.theme.clone(), config.ui.show_details, config.ui.graph_height);
```

- [ ] **Step 6: Confirm `rtt_history_for_graph` is now live code**

`render_pings_window` (Step 4) calls `rtt_history_for_graph`, so its `#[allow(dead_code)]` (removed in Step 1) is no longer needed. Run `cargo build` and confirm there is no dead-code warning for it.

- [ ] **Step 7: Update the controls help text**

In whichever renderer prints the controls line, update it to include the new keys: `'d' details | 't' theme`. (Old line lived in `render_pings_window`; move it to the status bar or help screen since the pings window is now row-based. Add to `render_status_bar` or `render_help` text: `q quit · d details · t theme · v viz · h help`.)

- [ ] **Step 8: Build, test, run**

Run: `cargo build && cargo test`
Expected: builds; all pass.

Run: `cargo run`. Verify: per-host rows show colored state + a live sparkline; `d` toggles the graphs; `t` cycles themes; pulling wifi shows the offline banner; a captive portal (or pointing `portal_check_url` at a portal) shows the portal banner.

- [ ] **Step 9: Commit**

```bash
git add src/tui.rs src/app.rs src/stats.rs
git commit -m "feat: per-host latency sparkline, connectivity banner, details/theme wired to renderer"
```

---

# PHASE 4 — Terminal Title, CI, Final Sweep

*Files: `app.rs`/`tui.rs`, `.github/workflows/ci.yml`, docs. Verify full clippy + test + build green; title correct across all four states.*

## Task 13: Live terminal title

**Files:**
- Modify: `src/tui.rs` (add `set_title`; import `SetTitle`)
- Modify: `src/app.rs` (compute + set title each tick)

- [ ] **Step 1: Add a `set_title` method to `TuiApp`**

Add `SetTitle` to the crossterm terminal import:

```rust
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle},
```

```rust
pub fn set_title(&mut self, title: &str) {
    let _ = execute!(io::stdout(), SetTitle(title));
}
```

- [ ] **Step 2: Set the title each tick in `app.rs`**

In the UI-tick arm (Task 12 Step 5), after computing `conn` and the aggregate, set the title before drawing:

```rust
                    let agg = status::aggregate(&states);
                    self.tui.set_title(&status::title(&conn, &agg));
```

- [ ] **Step 3: Build + run**

Run: `cargo build && cargo run`
Expected: terminal tab/title shows `● pingpong 3/3 up · 42ms`; pull wifi → `✗ pingpong offline`; portal → `⚠ pingpong captive portal — log in`. On quit, the original title returns (xterm title stack pop from Task 2).

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs src/app.rs
git commit -m "feat: live terminal-title connectivity summary"
```

## Task 14: Modernize CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Replace the deprecated actions**

- Change both `actions/cache@v3` blocks to use `Swatinem/rust-cache@v2` (remove the manual `path`/`key`; it caches `~/.cargo` + `target` automatically and keys on the lockfile + target).
- Change `actions/upload-artifact@v3` to `actions/upload-artifact@v4`.

Replace the `test` job's cache step:

```yaml
      - name: Cache cargo dependencies
        uses: Swatinem/rust-cache@v2
```

Replace the `build-cross-platform` cache step the same way (add `with: { key: ${{ matrix.target }} }` so per-target caches don't collide):

```yaml
      - name: Cache cargo dependencies
        uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
```

And the artifact upload:

```yaml
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: pingpong-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/${{ matrix.binary-name }}
```

- [ ] **Step 2: Validate YAML locally**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('ok')"`
Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: modernize cache (Swatinem/rust-cache) and upload-artifact v4"
```

## Task 15: Final verification sweep + docs reconciliation

**Files:**
- Modify: `pingpong.toml` (document `portal_check_url`), `CLAUDE.md`/`README` claims if present
- Create: `gotchas.md` (lessons learned)

- [ ] **Step 1: Run the full local gate (mirrors CI + pre-commit)**

Run:
```bash
cargo fmt --all -- --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test && \
cargo build --release
```
Expected: all four succeed. Fix anything that fails (follow the pre-commit failure protocol — never `--no-verify`).

- [ ] **Step 2: Reconcile docs with reality**

Update `pingpong.toml` to mention `portal_check_url` under `[ping]`. In `CLAUDE.md`, confirm the "graph" and "per ping loop DNS" claims now match behavior (they do after this work). Update the controls list anywhere it is stale.

- [ ] **Step 3: Manual scenario pass (the plane simulation)**

- Normal: all hosts up; title shows ratio + ms; sparklines move.
- Wifi off: hosts go Down without spam/crash; banner + title show offline; turning wifi back on recovers automatically.
- Captive portal: point `portal_check_url` at a host that 302-redirects (or test on a real portal); banner + title show portal.
- Tiny terminal: shrink to a few rows; cycle animations with `v`; no panic.
- Ctrl-C and Esc: both quit cleanly; original title restored.

- [ ] **Step 4: Write `gotchas.md`**

Capture the lessons (e.g., "surge-ping already defaults to DGRAM and auto-falls-back to RAW", "crossterm has no get-title; use xterm title stack 22;2t/23;2t", "fish `$status` after a pipe reflects the last command — verify exit codes directly"). One bullet per lesson.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs: reconcile config/docs with behavior; add gotchas"
```

## Task 16: Open a PR

- [ ] **Step 1: Push and open the PR**

```bash
git push -u origin robustness-hardening
gh pr create --fill --title "Robustness hardening for plane wifi" \
  --body "Implements docs/superpowers/specs/2026-06-08-pingpong-robustness-design.md: panic-safe terminal, network resilience (lazy resolve + backoff + recovery), captive-portal detection, honest config (graph/theme/details/packet_size), live terminal title, stats/config tests, CI modernization."
```

- [ ] **Step 2: Confirm CI is green on the PR**

Run: `gh pr checks --watch`
Expected: all checks pass.

---

## Self-Review (completed during planning)

- **Spec coverage:** Terminal safety → Tasks 2,3,4. Network resilience → Tasks 6,7,8,9. Honest config (graph/details/theme/packet_size) → Tasks 8 (packet_size),10,11,12. Tests/CI → Tasks 1,5,6,7,9,10,11,14,15. Terminal title → Tasks 7,13. Single-source-of-truth (remove dead engine stats map) → Task 8. Bounded channel → Task 8. Unprivileged-socket message → Task 8. All spec sections map to tasks.
- **Placeholder scan:** No TBD/TODO; every code step has concrete code; the `add_host` step explicitly resolves its own intermediate inconsistency to the IPv4-only-prefix version.
- **Type consistency:** `HostUpdate`/`HostState`/`ConnectivityState`/`ProbeResult`/`Backoff`/`Theme`/`RenderOpts`/`Aggregate` names and fields are used identically across `ping.rs`, `status.rs`, `probe.rs`, `app.rs`, `tui.rs`. `draw(stats, opts)` signature change is propagated to its only caller (Task 12). `PingEngine::new` becoming synchronous is reflected in its test (Task 8) and caller (`app.rs`).

> Note vs. spec: the spec said "restore original title via OSC"; implemented concretely as the xterm title-stack escapes (`CSI 22;2t`/`23;2t`) since crossterm cannot query the current title. Same intent, verified mechanism.
