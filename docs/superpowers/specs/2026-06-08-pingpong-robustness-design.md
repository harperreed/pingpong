# PingPong Robustness Hardening — Design

- **Date:** 2026-06-08
- **Status:** Approved (design); pending implementation plan
- **Branch:** `robustness-hardening`

## Context

PingPong is a Rust/ratatui TUI that pings multiple hosts and shows live latency,
loss, and connection quality. It works, but it is fragile in specific ways and a
chunk of its advertised configuration is wired to nothing.

The primary real-world use case is **monitoring connectivity on bad airplane
wifi**: intermittent DNS, captive portals that block everything until you log in,
hosts flapping up and down, and frequent reconnects. The current code hides all
failures behind the alternate screen, resolves DNS exactly once, and dies
silently when anything goes wrong — the worst possible behavior for that use case.

## Goals

Make PingPong robust enough to trust on a plane, **hardening in place** (no
re-architecture):

1. **Never break the terminal** — survive any panic, `Ctrl-C`, or tiny terminal
   without leaving the user's shell in raw mode.
2. **Survive network chaos** — DNS retry/backoff, automatic recovery of dead
   hosts, no host can kill startup, all failures visible in the UI, and
   distinguish "offline" from "captive portal".
3. **Honest config + features** — every config knob does what it says, including
   a real latency graph, a `show_details` toggle, theme switching, and a working
   `packet_size`.
4. **Tests + CI green** — cover the untested stats math, remove network-dependent
   test flakiness, fix current clippy failures, modernize CI.
5. **Terminal title summary** — a live, glanceable status in the title/tab bar.

## Non-Goals

- No re-architecture of the engine↔UI contract (event-driven, App-owns-stats
  stays). The only structural change is **deleting** the dead engine-side stats
  map — that is dead-code removal, not redesign.
- No rewrite of the existing animations (plasma/globe/bounce/matrix/dna/waveform)
  beyond fixing the clippy errors and the small-terminal panic guards.
- No new heavy dependencies. Captive-portal detection uses plain HTTP over the
  existing tokio `TcpStream` (portals intercept plaintext HTTP by design, so no
  TLS/`reqwest` is needed).

## Locked Decisions

| Question | Decision |
| --- | --- |
| Scope | Harden in place |
| Robustness priorities | Terminal safety, network resilience, honest config, tests/CI, **plus** terminal-title summary |
| Dead config | Make **all** real: latency graph, `show_details`, theme, and `packet_size` |
| Connectivity smarts | **Full HTTP captive-portal probe** (`/generate_204`-style) |
| Terminal title format | Status symbol + host ratio + most-relevant metric, with portal/offline states |

## Architecture

Keep the existing shape:

```
PingEngine ──spawns──> N host loops ─┐
           ──spawns──> 1 portal probe ┼─ mpsc events ─> App (single source of truth)
                                      ┘                    │
                                                 owns stats+state map
                                                 derives ConnectivityState
                                                 renders TUI + sets title
```

- **One source of truth:** the stats/state map owned by `App`. Per-host state and
  global connectivity are *derived* from it in one place each, never stored
  redundantly. The current dead second stats map inside `PingEngine` is removed.
- The mpsc channel becomes **bounded** (generous cap) so a stalled UI cannot grow
  memory without bound; senders drop/replace rather than block forever.

### Per-host state (single source of truth, derived)

```
HostState:
  Resolving                 // DNS in progress / backing off
  Up { last_rtt }           // recent pings succeeding
  Degraded { loss_pct }     // some loss in the recent window
  Down { reason }           // resolution or pings failing
```

Derived from the stats history + last result + resolver status. Drives the
per-host row symbol and the aggregate.

### Global connectivity (single source of truth, derived)

```
ConnectivityState:
  Online                    // pings flowing
  Degraded                  // some hosts up, some down/lossy
  CaptivePortal { url }     // portal probe says we are intercepted
  Offline                   // DNS + pings + probe all failing
```

Derived once per refresh from host states + the latest portal-probe result.
Drives **both** the in-UI banner and the terminal title — one value, two readers.

## Components (by module)

- **`main.rs`** — CLI wiring. Replace the fragile `interval != 1.0` "was it set?"
  sentinel with `Option<f64>`. Install the panic hook early.
- **`config.rs`** — config structs + new `Theme` handling; light validation
  (clamp absurd intervals/sizes); fix `add_host` name heuristic (currently
  misclassifies IPv6). Unit tests.
- **`ping.rs`** — host loop rework: lazy resolve (nothing resolves in `new()`),
  periodic re-resolution, exponential backoff (1s→2s→…→30s cap, reset on
  success), per-host state, `packet_size` payload, unprivileged ICMP datagram
  socket with clear fallback message. Remove the dead engine-side stats map and
  all `eprintln!`. Keep the deterministic tests; gate live-DNS test behind
  `#[ignore]`.
- **`probe.rs` (new, small)** — captive-portal probe task: periodic minimal
  HTTP/1.1 GET to a configurable endpoint (default `http://captive.apple.com`)
  over `TcpStream`; classify `Online` / `CaptivePortal{url}` / `Offline`.
- **`status.rs` (new, small, pure)** — derive `ConnectivityState` + aggregate
  (hosts up/total, avg latency, worst loss) and format the terminal-title string.
  Pure functions → easy to unit test across all four states.
- **`stats.rs`** — keep the math; add unit tests (loss total + recent window,
  jitter/stddev, median even/odd, avg/min/max, quality thresholds, empty edges);
  add small helpers for `HostState` derivation; wire up the currently-dead
  `rtt_history_for_graph` for the graph.
- **`tui.rs`** — `TerminalGuard` (RAII enter/exit incl. title save/restore);
  panic-safe teardown; `Theme` struct (dark/light; `auto`→`COLORFGBG` or dark)
  with runtime cycle; latency `Sparkline` sized by `graph_height`; `show_details`
  toggle; connectivity banner; clamp every animation divisor to ≥1 and switch
  `area.width - N` to `saturating_sub`; fix the 7 `manual_is_multiple_of` clippy
  errors; emit terminal title each refresh.
- **`app.rs`** — event loop gains a `tokio::signal::ctrl_c()` arm; handles new
  state/probe events; owns the single source of truth; computes + sets title.

## Terminal Title

Emit OSC (`ESC ] 0 ; <text> BEL`) each refresh, derived from `ConnectivityState`
+ aggregate:

```
online     ●  pingpong  3/3 up · 42ms
degraded   ◐  pingpong  2/3 up · 11% loss
portal     ⚠  pingpong  captive portal — log in
offline    ✗  pingpong  offline
```

The original title is saved on enter and restored by `TerminalGuard` on exit.

## Honest Config — details

- **`packet_size`** — send an N-byte payload (filled) instead of the current
  empty `&[]`.
- **`show_details`** — `d` collapses/expands the per-host detail block; initial
  value from config.
- **`theme`** — centralized `Theme` struct so colors are not threaded as
  literals; `dark`/`light` palettes; `auto` uses `COLORFGBG` if present else
  dark; `t` cycles at runtime.
- **`graph_height`** — drives a per-host ratatui `Sparkline` of recent RTT,
  fed by `rtt_history_for_graph`. The point of the plane use case: watch spikes
  and drops live.

## Error Handling

- No `eprintln!` anywhere in the running app — every failure becomes host/global
  state surfaced in the UI.
- Panic hook restores the terminal before printing the panic (covers spawned
  tasks, which `Drop` alone does not). `Drop` remains as backup.
- Config load failure falls back to default (as today) but surfaces a one-line,
  non-fatal note.
- Raw-socket permission failure → clear, actionable in-UI message rather than a
  cryptic error, after attempting the unprivileged datagram socket.

## Testing Strategy

- **Unit:** `stats.rs` math; `config.rs` parse/merge/`add_host`/validation;
  `status.rs` connectivity derivation + title formatting across all four states;
  backoff schedule.
- **Integration:** engine creation; event flow against a loopback host.
- **Network-dependent:** the live-DNS lookup test becomes `#[ignore]`'d
  (opt-in) — a test that fails on a plane is wrong for this app. The
  deterministic IP-parse path stays as a normal test. (No mocks, per project
  rules — we test the deterministic path and gate the real-network path.)
- TDD: write the failing test first for all pure logic (stats, status, backoff,
  config).

## CI Changes

- Fix the 7 `manual_is_multiple_of` clippy errors so `clippy -D warnings` is
  green on current stable.
- `actions/upload-artifact@v3 → v4`; replace the hand-rolled cargo cache with
  `Swatinem/rust-cache@v2`.
- Keep fmt + clippy + test + cross-platform build matrix.

## Phasing (≤5 files per phase, verify green after each)

1. **Safety net** — `TerminalGuard`, panic hook, `Ctrl-C`/`Esc`/`q`, divisor +
   `saturating_sub` guards, fix clippy. *Files:* `tui.rs`, `app.rs`, `main.rs`.
   *Verify:* clippy green; survives a 1-row terminal and `Ctrl-C`.
2. **Network resilience** — lazy resolve, backoff, per-host state, `packet_size`,
   unprivileged socket, remove dead stats map; `probe.rs`; `status.rs`. *Files:*
   `ping.rs`, `probe.rs`, `status.rs`, `app.rs`, `stats.rs`. *Verify:* state +
   backoff + status unit tests; manual offline/portal simulation.
3. **Honest config + features** — graph, `show_details` toggle, theme. *Files:*
   `config.rs`, `tui.rs`. *Verify:* feature tests + manual.
4. **Title + CI + full sweep** — terminal title, CI bumps, full test pass.
   *Files:* `app.rs`/`tui.rs`, `.github/workflows/ci.yml`, tests. *Verify:* full
   clippy + test + build green; title correct across all four states.

## Risks / Notes

- Unprivileged ICMP varies by OS (fine on macOS; Linux needs
  `net.ipv4.ping_group_range`). Fallback message must name the fix.
- Captive-portal endpoints differ by vendor; default chosen for macOS, made
  configurable.
- Terminal title support varies by terminal/multiplexer; OSC is widely supported
  but tmux may need `set -g set-titles on`. Documented, not blocking.
