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
pub fn host_state(
    stats: Option<&PingStats>,
    resolved: bool,
    resolve_error: Option<&str>,
) -> HostState {
    if let Some(err) = resolve_error {
        return HostState::Down {
            reason: format!("dns: {err}"),
        };
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
                HostState::Down {
                    reason: "no replies".to_string(),
                }
            } else if loss > 2.0 {
                HostState::Degraded { loss_pct: loss }
            } else {
                HostState::Up {
                    rtt_ms: s.rtt_stats().avg.as_secs_f64() * 1000.0,
                }
            }
        }
    }
}

/// Derive global connectivity from host states and the latest probe result.
pub fn connectivity(states: &[HostState], probe: &ProbeResult) -> ConnectivityState {
    if let ProbeResult::CaptivePortal { url } = probe {
        return ConnectivityState::CaptivePortal { url: url.clone() };
    }
    let up = states
        .iter()
        .filter(|s| matches!(s, HostState::Up { .. }))
        .count();
    let any_traffic = states
        .iter()
        .any(|s| matches!(s, HostState::Up { .. } | HostState::Degraded { .. }));
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

/// Compute hosts-up count, mean RTT across Up hosts, and worst loss across Degraded/Down hosts.
pub fn aggregate(states: &[HostState]) -> Aggregate {
    let hosts_total = states.len();
    let hosts_up = states
        .iter()
        .filter(|s| matches!(s, HostState::Up { .. }))
        .count();
    let up_rtts: Vec<f64> = states
        .iter()
        .filter_map(|s| match s {
            HostState::Up { rtt_ms } => Some(*rtt_ms),
            _ => None,
        })
        .collect();
    let avg_rtt_ms = if up_rtts.is_empty() {
        0.0
    } else {
        up_rtts.iter().sum::<f64>() / up_rtts.len() as f64
    };
    let worst_loss_pct = states
        .iter()
        .filter_map(|s| match s {
            HostState::Degraded { loss_pct } => Some(*loss_pct),
            HostState::Down { .. } => Some(100.0),
            _ => None,
        })
        .fold(0.0_f64, f64::max);
    Aggregate {
        hosts_up,
        hosts_total,
        avg_rtt_ms,
        worst_loss_pct,
    }
}

/// Build the terminal-title string: symbol + ratio + most-relevant metric.
pub fn title(conn: &ConnectivityState, agg: &Aggregate) -> String {
    match conn {
        ConnectivityState::Online => {
            format!(
                "\u{25cf}  pingpong  {}/{} up \u{b7} {:.0}ms",
                agg.hosts_up, agg.hosts_total, agg.avg_rtt_ms
            )
        }
        ConnectivityState::Degraded => {
            format!(
                "\u{25d0}  pingpong  {}/{} up \u{b7} {:.0}% loss",
                agg.hosts_up, agg.hosts_total, agg.worst_loss_pct
            )
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
            s.add_result(&PingResult::Success {
                rtt: Duration::from_millis(ms),
                sequence: 0,
                timestamp: Instant::now(),
            });
        }
        for _ in 0..timeouts {
            s.add_result(&PingResult::Timeout {
                sequence: 0,
                timestamp: Instant::now(),
            });
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
            HostState::Down {
                reason: "dns: no address".to_string()
            }
        );
    }

    #[test]
    fn up_when_healthy() {
        let s = stats_with(20, 0, 30);
        assert_eq!(
            host_state(Some(&s), true, None),
            HostState::Up { rtt_ms: 30.0 }
        );
    }

    #[test]
    fn degraded_with_some_loss() {
        let s = stats_with(18, 2, 30); // 10% recent loss
        assert!(matches!(
            host_state(Some(&s), true, None),
            HostState::Degraded { .. }
        ));
    }

    #[test]
    fn portal_probe_wins() {
        let states = vec![HostState::Up { rtt_ms: 10.0 }];
        let conn = connectivity(
            &states,
            &ProbeResult::CaptivePortal {
                url: "http://x".into(),
            },
        );
        assert_eq!(
            conn,
            ConnectivityState::CaptivePortal {
                url: "http://x".into()
            }
        );
    }

    #[test]
    fn online_when_all_up() {
        let states = vec![
            HostState::Up { rtt_ms: 10.0 },
            HostState::Up { rtt_ms: 20.0 },
        ];
        assert_eq!(
            connectivity(&states, &ProbeResult::Online),
            ConnectivityState::Online
        );
    }

    #[test]
    fn offline_when_all_down_and_probe_offline() {
        let states = vec![HostState::Down { reason: "x".into() }];
        assert_eq!(
            connectivity(&states, &ProbeResult::Offline),
            ConnectivityState::Offline
        );
    }

    #[test]
    fn title_strings_match_states() {
        let agg = Aggregate {
            hosts_up: 3,
            hosts_total: 3,
            avg_rtt_ms: 42.0,
            worst_loss_pct: 0.0,
        };
        assert!(title(&ConnectivityState::Online, &agg).contains("3/3 up"));
        assert!(title(&ConnectivityState::Online, &agg).contains("42ms"));
        assert!(
            title(&ConnectivityState::CaptivePortal { url: "x".into() }, &agg)
                .contains("captive portal")
        );
        assert!(title(&ConnectivityState::Offline, &agg).contains("offline"));
    }

    #[test]
    fn down_when_all_replies_lost() {
        let s = stats_with(0, 5, 0); // 100% recent loss
        assert_eq!(
            host_state(Some(&s), true, None),
            HostState::Down {
                reason: "no replies".to_string()
            }
        );
    }

    #[test]
    fn offline_when_no_hosts() {
        // An empty host list must never read as Online, even if the probe is Online.
        assert_eq!(
            connectivity(&[], &ProbeResult::Online),
            ConnectivityState::Offline
        );
    }

    #[test]
    fn aggregate_counts_and_worst_loss() {
        let states = vec![
            HostState::Up { rtt_ms: 10.0 },
            HostState::Degraded { loss_pct: 15.0 },
            HostState::Down { reason: "x".into() },
        ];
        let agg = aggregate(&states);
        assert_eq!(agg.hosts_up, 1);
        assert_eq!(agg.hosts_total, 3);
        assert_eq!(agg.avg_rtt_ms, 10.0);
        assert_eq!(agg.worst_loss_pct, 100.0); // a Down host counts as 100% loss
    }

    #[test]
    fn title_degraded_shows_loss() {
        let agg = Aggregate {
            hosts_up: 2,
            hosts_total: 3,
            avg_rtt_ms: 0.0,
            worst_loss_pct: 11.0,
        };
        let t = title(&ConnectivityState::Degraded, &agg);
        assert!(t.contains("2/3 up"));
        assert!(t.contains("11% loss"));
    }
}
