// ABOUTME: Statistics collection and analysis for ping results
// ABOUTME: Maintains circular buffers of ping data and computes real-time metrics

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum PingResult {
    Success {
        rtt: Duration,
        #[allow(dead_code)]
        sequence: u16,
        #[allow(dead_code)]
        timestamp: Instant,
    },
    Timeout {
        #[allow(dead_code)]
        sequence: u16,
        #[allow(dead_code)]
        timestamp: Instant,
    },
    Error {
        #[allow(dead_code)]
        error: String,
        #[allow(dead_code)]
        sequence: u16,
        #[allow(dead_code)]
        timestamp: Instant,
    },
}

impl PingResult {
    #[allow(dead_code)]
    pub fn timestamp(&self) -> Instant {
        match self {
            PingResult::Success { timestamp, .. } => *timestamp,
            PingResult::Timeout { timestamp, .. } => *timestamp,
            PingResult::Error { timestamp, .. } => *timestamp,
        }
    }

    #[allow(dead_code)]
    pub fn sequence(&self) -> u16 {
        match self {
            PingResult::Success { sequence, .. } => *sequence,
            PingResult::Timeout { sequence, .. } => *sequence,
            PingResult::Error { sequence, .. } => *sequence,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, PingResult::Success { .. })
    }

    pub fn rtt(&self) -> Option<Duration> {
        match self {
            PingResult::Success { rtt, .. } => Some(*rtt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PingStats {
    history: VecDeque<PingResult>,
    max_history: usize,
    total_pings: u64,
    successful_pings: u64,
    timeouts: u64,
    errors: u64,
}

impl PingStats {
    pub fn new(max_history: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_history),
            max_history,
            total_pings: 0,
            successful_pings: 0,
            timeouts: 0,
            errors: 0,
        }
    }

    pub fn add_result(&mut self, result: &PingResult) {
        // Add to history
        if self.history.len() >= self.max_history {
            self.history.pop_front();
        }
        self.history.push_back(result.clone());

        // Update counters
        self.total_pings += 1;
        match result {
            PingResult::Success { .. } => self.successful_pings += 1,
            PingResult::Timeout { .. } => self.timeouts += 1,
            PingResult::Error { .. } => self.errors += 1,
        }
    }

    pub fn packet_loss_percent(&self) -> f64 {
        if self.total_pings == 0 {
            0.0
        } else {
            ((self.total_pings - self.successful_pings) as f64 / self.total_pings as f64) * 100.0
        }
    }

    pub fn packet_loss_percent_recent(&self, window_size: usize) -> f64 {
        let recent_results: Vec<_> = self.history.iter().rev().take(window_size).collect();
        if recent_results.is_empty() {
            return 0.0;
        }

        let successful = recent_results.iter().filter(|r| r.is_success()).count();
        let total = recent_results.len();

        ((total - successful) as f64 / total as f64) * 100.0
    }

    pub fn rtt_stats(&self) -> RttStats {
        let rtts: Vec<Duration> = self.history.iter().filter_map(|r| r.rtt()).collect();

        if rtts.is_empty() {
            return RttStats::default();
        }

        let mut sorted_rtts = rtts.clone();
        sorted_rtts.sort();

        let min = *sorted_rtts.first().unwrap();
        let max = *sorted_rtts.last().unwrap();

        let sum: Duration = rtts.iter().sum();
        let avg = sum / rtts.len() as u32;

        let median = if sorted_rtts.len().is_multiple_of(2) {
            let mid = sorted_rtts.len() / 2;
            (sorted_rtts[mid - 1] + sorted_rtts[mid]) / 2
        } else {
            sorted_rtts[sorted_rtts.len() / 2]
        };

        // Calculate jitter (standard deviation of RTT)
        let variance: f64 = rtts
            .iter()
            .map(|rtt| {
                let diff = rtt.as_secs_f64() - avg.as_secs_f64();
                diff * diff
            })
            .sum::<f64>()
            / rtts.len() as f64;

        let jitter = Duration::from_secs_f64(variance.sqrt());

        RttStats {
            min,
            max,
            avg,
            median,
            jitter,
        }
    }

    pub fn connection_quality(&self) -> ConnectionQuality {
        let loss_percent = self.packet_loss_percent_recent(20); // Last 20 pings
        let rtt_stats = self.rtt_stats();

        // Quality based on packet loss and RTT
        if loss_percent > 10.0 || rtt_stats.avg > Duration::from_millis(500) {
            ConnectionQuality::Poor
        } else if loss_percent > 2.0 || rtt_stats.avg > Duration::from_millis(100) {
            ConnectionQuality::Fair
        } else {
            ConnectionQuality::Good
        }
    }

    #[allow(dead_code)]
    pub fn recent_results(&self, count: usize) -> Vec<&PingResult> {
        self.history.iter().rev().take(count).collect()
    }

    #[allow(dead_code)]
    pub fn history(&self) -> &VecDeque<PingResult> {
        &self.history
    }

    pub fn total_pings(&self) -> u64 {
        self.total_pings
    }

    #[allow(dead_code)]
    pub fn successful_pings(&self) -> u64 {
        self.successful_pings
    }

    #[allow(dead_code)]
    pub fn timeouts(&self) -> u64 {
        self.timeouts
    }

    #[allow(dead_code)]
    pub fn errors(&self) -> u64 {
        self.errors
    }

    #[allow(dead_code)]
    pub fn rtt_history_for_graph(&self, points: usize) -> Vec<Option<f64>> {
        let total_points = self.history.len();
        if total_points == 0 {
            return vec![None; points];
        }

        let step = if total_points <= points {
            1
        } else {
            total_points / points
        };

        let mut graph_points = Vec::with_capacity(points);

        for i in (0..total_points).step_by(step).take(points) {
            if let Some(result) = self.history.get(i) {
                graph_points.push(result.rtt().map(|rtt| rtt.as_secs_f64() * 1000.0));
            // Convert to ms
            } else {
                graph_points.push(None);
            }
        }

        // Pad with None if needed
        while graph_points.len() < points {
            graph_points.push(None);
        }

        graph_points
    }
}

#[derive(Debug, Clone, Default)]
pub struct RttStats {
    #[allow(dead_code)]
    pub min: Duration,
    #[allow(dead_code)]
    pub max: Duration,
    pub avg: Duration,
    #[allow(dead_code)]
    pub median: Duration,
    #[allow(dead_code)]
    pub jitter: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionQuality {
    Good,
    Fair,
    Poor,
}

impl ConnectionQuality {
    #[allow(dead_code)]
    pub fn color(&self) -> &'static str {
        match self {
            ConnectionQuality::Good => "green",
            ConnectionQuality::Fair => "yellow",
            ConnectionQuality::Poor => "red",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            ConnectionQuality::Good => "●",
            ConnectionQuality::Fair => "◐",
            ConnectionQuality::Poor => "○",
        }
    }
}

#[cfg(test)]
mod stats_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn success(ms: u64) -> PingResult {
        PingResult::Success {
            rtt: Duration::from_millis(ms),
            sequence: 0,
            timestamp: Instant::now(),
        }
    }

    fn timeout() -> PingResult {
        PingResult::Timeout {
            sequence: 0,
            timestamp: Instant::now(),
        }
    }

    fn error_result() -> PingResult {
        PingResult::Error {
            error: "test".to_string(),
            sequence: 0,
            timestamp: Instant::now(),
        }
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
        s.add_result(&error_result());
        assert_eq!(s.total_pings(), 5);
        assert!((s.packet_loss_percent() - 60.0).abs() < 1e-9); // 3 of 5 non-success
    }

    #[test]
    fn recent_loss_uses_only_window() {
        let mut s = PingStats::new(100);
        for _ in 0..10 {
            s.add_result(&success(10));
        }
        for _ in 0..2 {
            s.add_result(&timeout());
        }
        // window of 2 = the two most recent, both timeouts = 100%
        assert!((s.packet_loss_percent_recent(2) - 100.0).abs() < 1e-9);
        // window of 12 = 2/12 lost
        assert!((s.packet_loss_percent_recent(12) - (2.0 / 12.0 * 100.0)).abs() < 1e-9);
    }

    #[test]
    fn rtt_min_max_avg_median_odd() {
        let mut s = PingStats::new(100);
        for ms in [10u64, 20, 30] {
            s.add_result(&success(ms));
        }
        let r = s.rtt_stats();
        assert_eq!(r.min, Duration::from_millis(10));
        assert_eq!(r.max, Duration::from_millis(30));
        assert_eq!(r.avg, Duration::from_millis(20));
        assert_eq!(r.median, Duration::from_millis(20));
    }

    #[test]
    fn rtt_median_even() {
        let mut s = PingStats::new(100);
        for ms in [10u64, 20, 30, 40] {
            s.add_result(&success(ms));
        }
        // even count -> mean of the two middle values (20, 30) = 25
        assert_eq!(s.rtt_stats().median, Duration::from_millis(25));
    }

    #[test]
    fn jitter_zero_for_constant_rtt() {
        let mut s = PingStats::new(100);
        for _ in 0..5 {
            s.add_result(&success(42));
        }
        assert!(s.rtt_stats().jitter < Duration::from_micros(50));
    }

    #[test]
    fn quality_thresholds() {
        let mut good = PingStats::new(100);
        for _ in 0..20 {
            good.add_result(&success(10));
        }
        assert_eq!(good.connection_quality(), ConnectionQuality::Good);

        let mut poor = PingStats::new(100);
        for _ in 0..20 {
            poor.add_result(&timeout());
        }
        assert_eq!(poor.connection_quality(), ConnectionQuality::Poor);
    }

    #[test]
    fn history_is_bounded() {
        let mut s = PingStats::new(3);
        for ms in [1u64, 2, 3, 4, 5] {
            s.add_result(&success(ms));
        }
        // Buffer caps at 3, so the oldest (1, 2) are dropped: min over {3,4,5} is 3.
        assert_eq!(s.rtt_stats().min, Duration::from_millis(3));
        assert_eq!(s.total_pings(), 5); // cumulative counter is unaffected by the cap
    }
}
