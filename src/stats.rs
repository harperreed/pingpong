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

        let median = if sorted_rtts.len() % 2 == 0 {
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
