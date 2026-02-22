use crate::errors::{DaemonError, Result};
use crate::escpos::PrintItem;
use crate::queue::{PrintJob, QueueManager};
use crate::status;
use crate::supabase_client::SupabaseClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Adaptive backoff steps (seconds).
/// Jobs found → snap to index 0 (3s).
/// Empty response or error → advance index (3→5→10→15).
const BACKOFF_STEPS: [u64; 4] = [3, 5, 10, 15];

/// How often to refresh failover config (seconds)
const FAILOVER_REFRESH_INTERVAL: u64 = 300; // 5 minutes

/// Polling-based job fetcher with adaptive backoff.
///
/// Polls the Edge Function for pending print jobs, then enqueues them
/// into the local SQLite queue for processing. Piggybacks heartbeat
/// updates on every poll call (printer_ids sent in payload).
pub struct JobPoller;

impl JobPoller {
    /// Start polling for pending print jobs.
    /// Returns a JoinHandle that can be aborted to stop polling.
    ///
    /// `printer_ids`: IDs of configured printers, sent with each poll
    /// for heartbeat piggyback (last_seen + status='online').
    /// `failover_map`: shared cache updated with failover config from edge function.
    pub fn start(
        restaurant_id: String,
        client: Arc<SupabaseClient>,
        queue_manager: Arc<Mutex<QueueManager>>,
        printer_ids: Vec<String>,
        failover_map: Arc<Mutex<HashMap<String, Vec<String>>>>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff_index: usize = 0;
            let mut last_failover_refresh = std::time::Instant::now()
                - std::time::Duration::from_secs(FAILOVER_REFRESH_INTERVAL); // Force first fetch

            info!(
                "Job poller started (adaptive backoff {:?}s) for restaurant {}, heartbeat printers: {}",
                BACKOFF_STEPS, restaurant_id, printer_ids.len()
            );

            loop {
                let delay = BACKOFF_STEPS[backoff_index];
                tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;

                // Include failover config request every 5 minutes
                let include_failover =
                    last_failover_refresh.elapsed().as_secs() >= FAILOVER_REFRESH_INTERVAL;

                match client
                    .poll_pending_jobs_with_failover(&printer_ids, include_failover)
                    .await
                {
                    Ok(poll_result) => {
                        // Update failover config if received
                        if let Some(config) = poll_result.failover_config {
                            let mut map = failover_map.lock().await;
                            *map = config;
                            last_failover_refresh = std::time::Instant::now();
                            info!("Failover config refreshed ({} primary printers mapped)", map.len());
                        }

                        if !poll_result.jobs.is_empty() {
                            debug!(
                                "Polled {} pending jobs (backoff reset to {}s)",
                                poll_result.jobs.len(),
                                BACKOFF_STEPS[0]
                            );
                            backoff_index = 0;

                            let queue = queue_manager.lock().await;
                            for job_json in &poll_result.jobs {
                                match Self::parse_job(job_json, &restaurant_id) {
                                    Ok(job) => {
                                        if let Err(e) = queue.enqueue(job).await {
                                            debug!("Enqueue skipped (likely dedup): {}", e);
                                        }
                                    }
                                    Err(e) => warn!("Failed to parse polled job: {}", e),
                                }
                            }
                        } else {
                            // No pending jobs — back off
                            if backoff_index < BACKOFF_STEPS.len() - 1 {
                                backoff_index += 1;
                                debug!("No jobs, backing off to {}s", BACKOFF_STEPS[backoff_index]);
                            }
                        }
                    }
                    Err(e) => {
                        // Error — also back off (don't hammer failing endpoint)
                        if backoff_index < BACKOFF_STEPS.len() - 1 {
                            backoff_index += 1;
                        }
                        warn!(
                            "Job poll failed (backoff {}s): {}",
                            BACKOFF_STEPS[backoff_index], e
                        );
                    }
                }
            }
        })
    }

    /// Parse a Supabase row JSON into a PrintJob
    fn parse_job(record: &serde_json::Value, restaurant_id: &str) -> Result<PrintJob> {
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing job id".to_string()))?
            .to_string();

        let order_id = record
            .get("order_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let order_number = record
            .get("order_number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing order_number".to_string()))?
            .to_string();

        let station = record
            .get("station")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing station".to_string()))?
            .to_string();

        let items_json = record
            .get("items")
            .ok_or_else(|| DaemonError::Queue("Missing items".to_string()))?;

        let items: Vec<PrintItem> = serde_json::from_value(items_json.clone())
            .map_err(|e| DaemonError::Queue(format!("Failed to parse items: {}", e)))?;

        let timestamp = record
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

        Ok(PrintJob {
            id,
            restaurant_id: restaurant_id.to_string(),
            order_id,
            order_number,
            station,
            station_id: record.get("station_id").and_then(|v| v.as_str()).map(String::from),
            printer_id: record.get("printer_id").and_then(|v| v.as_str()).map(String::from),
            items,
            table_number: record.get("table_number").and_then(|v| v.as_str()).map(String::from),
            customer_name: record.get("customer_name").and_then(|v| v.as_str()).map(String::from),
            order_type: record.get("order_type").and_then(|v| v.as_str()).map(String::from),
            priority: record.get("priority").and_then(|v| v.as_u64()).unwrap_or(3) as u8,
            timestamp,
            status: status::PENDING.to_string(),
            retry_count: 0,
            error_message: None,
        })
    }
}
