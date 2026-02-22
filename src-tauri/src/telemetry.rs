use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Telemetry event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEvent {
    /// Print job completed
    PrintJobCompleted {
        job_id: String,
        order_number: String,
        station: String,
        printer_id: String,
        duration_ms: u64,
        retry_count: u32,
    },
    /// Print job failed
    PrintJobFailed {
        job_id: String,
        order_number: String,
        station: String,
        printer_id: Option<String>,
        error: String,
        retry_count: u32,
    },
    /// Printer status changed
    PrinterStatusChanged {
        printer_id: String,
        old_status: String,
        new_status: String,
    },
    /// Circuit breaker state changed
    CircuitBreakerStateChanged {
        printer_id: String,
        old_state: String,
        new_state: String,
    },
    /// Realtime connection status changed
    RealtimeConnectionChanged {
        restaurant_id: String,
        old_status: String,
        new_status: String,
    },
    /// Failover attempted: primary failed, backup tried
    FailoverAttempted {
        job_id: String,
        primary_printer_id: String,
        backup_printer_id: String,
        success: bool,
    },
    /// Connection pool health statistics
    ConnectionPoolStats {
        active_connections: usize,
        stale_removed: usize,
    },
    /// Queue statistics snapshot
    QueueSnapshot {
        pending: usize,
        processing: usize,
        completed: usize,
        failed: usize,
    },
}

/// Telemetry metrics for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryMetrics {
    /// Total print jobs completed
    pub total_jobs_completed: u64,
    /// Total print jobs failed
    pub total_jobs_failed: u64,
    /// Average print duration (milliseconds)
    pub avg_print_duration_ms: u64,
    /// Success rate (0.0 - 1.0)
    pub success_rate: f64,
    /// Current queue depth
    pub queue_depth: usize,
    /// Printer online count
    pub printers_online: usize,
    /// Printer offline count
    pub printers_offline: usize,
    /// Circuit breakers open
    pub circuit_breakers_open: usize,
    /// Last update timestamp
    pub last_update_ts: u64,
}

impl Default for TelemetryMetrics {
    fn default() -> Self {
        Self {
            total_jobs_completed: 0,
            total_jobs_failed: 0,
            avg_print_duration_ms: 0,
            success_rate: 1.0,
            queue_depth: 0,
            printers_online: 0,
            printers_offline: 0,
            circuit_breakers_open: 0,
            last_update_ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Telemetry collector for aggregating metrics
pub struct TelemetryCollector {
    /// Current metrics
    metrics: Arc<RwLock<TelemetryMetrics>>,
    /// Event history (last 1000 events)
    event_history: Arc<RwLock<Vec<(u64, TelemetryEvent)>>>,
    /// Print duration samples (for averaging, max 1000)
    print_durations: Arc<RwLock<Vec<u64>>>,
}

impl TelemetryCollector {
    /// Create new telemetry collector
    pub fn new() -> Self {
        info!("Initializing telemetry collector");
        Self {
            metrics: Arc::new(RwLock::new(TelemetryMetrics::default())),
            event_history: Arc::new(RwLock::new(Vec::new())),
            print_durations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record telemetry event
    pub async fn record_event(&self, event: TelemetryEvent) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Update metrics based on event type
        let mut metrics = self.metrics.write().await;

        match &event {
            TelemetryEvent::PrintJobCompleted {
                duration_ms,
                ..
            } => {
                metrics.total_jobs_completed += 1;

                // Update average print duration
                let mut durations = self.print_durations.write().await;
                durations.push(*duration_ms);

                // Keep only last 1000 samples
                if durations.len() > 1000 {
                    durations.remove(0);
                }

                let sum: u64 = durations.iter().sum();
                metrics.avg_print_duration_ms = sum / durations.len() as u64;

                // Update success rate
                let total = metrics.total_jobs_completed + metrics.total_jobs_failed;
                if total > 0 {
                    metrics.success_rate = metrics.total_jobs_completed as f64 / total as f64;
                }

                debug!(
                    "Print job completed - Total: {}, Avg duration: {}ms, Success rate: {:.2}%",
                    metrics.total_jobs_completed,
                    metrics.avg_print_duration_ms,
                    metrics.success_rate * 100.0
                );
            }
            TelemetryEvent::PrintJobFailed { .. } => {
                metrics.total_jobs_failed += 1;

                // Update success rate
                let total = metrics.total_jobs_completed + metrics.total_jobs_failed;
                if total > 0 {
                    metrics.success_rate = metrics.total_jobs_completed as f64 / total as f64;
                }

                debug!(
                    "Print job failed - Total failed: {}, Success rate: {:.2}%",
                    metrics.total_jobs_failed,
                    metrics.success_rate * 100.0
                );
            }
            TelemetryEvent::QueueSnapshot {
                pending,
                processing,
                ..
            } => {
                metrics.queue_depth = *pending + *processing;
                debug!("Queue snapshot - Depth: {}, Pending: {}, Processing: {}", metrics.queue_depth, pending, processing);
            }
            TelemetryEvent::CircuitBreakerStateChanged { new_state, .. } => {
                // Count open circuit breakers
                // Note: This is simplified, in production you'd maintain a map of all breakers
                if new_state == "open" {
                    metrics.circuit_breakers_open += 1;
                } else if new_state == "closed" {
                    metrics.circuit_breakers_open = metrics.circuit_breakers_open.saturating_sub(1);
                }
                debug!("Circuit breakers open: {}", metrics.circuit_breakers_open);
            }
            TelemetryEvent::PrinterStatusChanged { printer_id, old_status, new_status } => {
                debug!("Printer {} status: {} → {}", printer_id, old_status, new_status);
            }
            TelemetryEvent::FailoverAttempted { job_id, primary_printer_id, backup_printer_id, success } => {
                if *success {
                    info!(
                        "Failover succeeded: job {} routed {} → {}",
                        job_id, primary_printer_id, backup_printer_id
                    );
                } else {
                    debug!(
                        "Failover attempted: job {} tried {} → {} (failed)",
                        job_id, primary_printer_id, backup_printer_id
                    );
                }
            }
            TelemetryEvent::ConnectionPoolStats { active_connections, stale_removed } => {
                debug!("Connection pool: {} active, {} stale removed", active_connections, stale_removed);
            }
            _ => {}
        }

        metrics.last_update_ts = timestamp;
        drop(metrics);

        // Store event in history
        let mut history = self.event_history.write().await;
        history.push((timestamp, event));

        // Keep only last 1000 events
        if history.len() > 1000 {
            history.remove(0);
        }
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> TelemetryMetrics {
        self.metrics.read().await.clone()
    }

    /// Get event history (last N events)
    pub async fn get_event_history(&self, limit: usize) -> Vec<(u64, TelemetryEvent)> {
        let history = self.event_history.read().await;
        let start = history.len().saturating_sub(limit);
        history[start..].to_vec()
    }

    /// Get metrics summary as JSON
    pub async fn get_metrics_json(&self) -> serde_json::Value {
        let metrics = self.get_metrics().await;
        serde_json::to_value(&metrics).unwrap_or_default()
    }

    /// Reset all metrics (for testing)
    #[allow(dead_code)]
    pub async fn reset(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = TelemetryMetrics::default();

        let mut history = self.event_history.write().await;
        history.clear();

        let mut durations = self.print_durations.write().await;
        durations.clear();

        info!("Telemetry metrics reset");
    }

    /// Update printer online/offline counts
    pub async fn update_printer_counts(&self, online: usize, offline: usize) {
        let mut metrics = self.metrics.write().await;
        metrics.printers_online = online;
        metrics.printers_offline = offline;
        debug!("Printers - Online: {}, Offline: {}", online, offline);
    }

    /// Export metrics for external monitoring (Prometheus format)
    pub async fn export_prometheus(&self) -> String {
        let metrics = self.get_metrics().await;

        format!(
            "# HELP printer_jobs_completed_total Total number of completed print jobs\n\
             # TYPE printer_jobs_completed_total counter\n\
             printer_jobs_completed_total {}\n\
             \n\
             # HELP printer_jobs_failed_total Total number of failed print jobs\n\
             # TYPE printer_jobs_failed_total counter\n\
             printer_jobs_failed_total {}\n\
             \n\
             # HELP printer_avg_duration_ms Average print duration in milliseconds\n\
             # TYPE printer_avg_duration_ms gauge\n\
             printer_avg_duration_ms {}\n\
             \n\
             # HELP printer_success_rate Print job success rate (0.0 - 1.0)\n\
             # TYPE printer_success_rate gauge\n\
             printer_success_rate {:.4}\n\
             \n\
             # HELP printer_queue_depth Current queue depth (pending + processing)\n\
             # TYPE printer_queue_depth gauge\n\
             printer_queue_depth {}\n\
             \n\
             # HELP printer_online Number of printers online\n\
             # TYPE printer_online gauge\n\
             printer_online {}\n\
             \n\
             # HELP printer_offline Number of printers offline\n\
             # TYPE printer_offline gauge\n\
             printer_offline {}\n\
             \n\
             # HELP printer_circuit_breakers_open Number of circuit breakers in OPEN state\n\
             # TYPE printer_circuit_breakers_open gauge\n\
             printer_circuit_breakers_open {}\n",
            metrics.total_jobs_completed,
            metrics.total_jobs_failed,
            metrics.avg_print_duration_ms,
            metrics.success_rate,
            metrics.queue_depth,
            metrics.printers_online,
            metrics.printers_offline,
            metrics.circuit_breakers_open,
        )
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Periodic telemetry reporter (sends metrics to external systems)
pub struct TelemetryReporter {
    collector: Arc<TelemetryCollector>,
}

impl TelemetryReporter {
    /// Create new telemetry reporter
    pub fn new(collector: Arc<TelemetryCollector>) -> Self {
        Self { collector }
    }

    /// Start periodic reporting task
    pub async fn start_reporting(&self, interval_secs: u64) {
        let collector = self.collector.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

            loop {
                interval.tick().await;

                let metrics = collector.get_metrics().await;

                info!(
                    "Telemetry Report - Jobs: {} completed, {} failed | Success: {:.1}% | Avg duration: {}ms | Queue: {} | Printers: {} online, {} offline",
                    metrics.total_jobs_completed,
                    metrics.total_jobs_failed,
                    metrics.success_rate * 100.0,
                    metrics.avg_print_duration_ms,
                    metrics.queue_depth,
                    metrics.printers_online,
                    metrics.printers_offline,
                );

                // TODO: Send to external monitoring system (Sentry, Prometheus, etc.)
                // This is where you'd send metrics to your monitoring backend
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_print_job_completed() {
        let collector = TelemetryCollector::new();

        collector
            .record_event(TelemetryEvent::PrintJobCompleted {
                job_id: "job_1".to_string(),
                order_number: "R001-0001".to_string(),
                station: "bar".to_string(),
                printer_id: "printer_1".to_string(),
                duration_ms: 150,
                retry_count: 0,
            })
            .await;

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.total_jobs_completed, 1);
        assert_eq!(metrics.avg_print_duration_ms, 150);
        assert_eq!(metrics.success_rate, 1.0);
    }

    #[tokio::test]
    async fn test_record_print_job_failed() {
        let collector = TelemetryCollector::new();

        collector
            .record_event(TelemetryEvent::PrintJobFailed {
                job_id: "job_2".to_string(),
                order_number: "R001-0002".to_string(),
                station: "kitchen".to_string(),
                printer_id: Some("printer_2".to_string()),
                error: "Printer offline".to_string(),
                retry_count: 3,
            })
            .await;

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.total_jobs_failed, 1);
        assert_eq!(metrics.success_rate, 0.0);
    }

    #[tokio::test]
    async fn test_success_rate_calculation() {
        let collector = TelemetryCollector::new();

        // 3 completed, 1 failed = 75% success rate
        for i in 0..3 {
            collector
                .record_event(TelemetryEvent::PrintJobCompleted {
                    job_id: format!("job_{}", i),
                    order_number: format!("R001-000{}", i),
                    station: "bar".to_string(),
                    printer_id: "printer_1".to_string(),
                    duration_ms: 100,
                    retry_count: 0,
                })
                .await;
        }

        collector
            .record_event(TelemetryEvent::PrintJobFailed {
                job_id: "job_fail".to_string(),
                order_number: "R001-0004".to_string(),
                station: "bar".to_string(),
                printer_id: Some("printer_1".to_string()),
                error: "Test error".to_string(),
                retry_count: 3,
            })
            .await;

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.total_jobs_completed, 3);
        assert_eq!(metrics.total_jobs_failed, 1);
        assert!((metrics.success_rate - 0.75).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_event_history_limit() {
        let collector = TelemetryCollector::new();

        // Record 1500 events (exceeds 1000 limit)
        for i in 0..1500 {
            collector
                .record_event(TelemetryEvent::PrintJobCompleted {
                    job_id: format!("job_{}", i),
                    order_number: format!("R001-{:04}", i),
                    station: "bar".to_string(),
                    printer_id: "printer_1".to_string(),
                    duration_ms: 100,
                    retry_count: 0,
                })
                .await;
        }

        let history = collector.get_event_history(2000).await;

        // Should only keep last 1000
        assert_eq!(history.len(), 1000);
    }

    #[tokio::test]
    async fn test_prometheus_export() {
        let collector = TelemetryCollector::new();

        collector
            .record_event(TelemetryEvent::PrintJobCompleted {
                job_id: "job_1".to_string(),
                order_number: "R001-0001".to_string(),
                station: "bar".to_string(),
                printer_id: "printer_1".to_string(),
                duration_ms: 200,
                retry_count: 0,
            })
            .await;

        collector.update_printer_counts(2, 1).await;

        let prometheus = collector.export_prometheus().await;

        assert!(prometheus.contains("printer_jobs_completed_total 1"));
        assert!(prometheus.contains("printer_avg_duration_ms 200"));
        assert!(prometheus.contains("printer_online 2"));
        assert!(prometheus.contains("printer_offline 1"));
    }
}
