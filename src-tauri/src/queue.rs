use crate::errors::{DaemonError, Result};
use crate::escpos::PrintItem;
use backon::{ExponentialBuilder, Retryable};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_rusqlite::Connection;
use tracing::{debug, error, info, warn};
use sha2::{Sha256, Digest};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJob {
    pub id: String,
    pub restaurant_id: String,
    pub order_id: String,
    pub order_number: String,
    pub station: String,
    pub printer_id: Option<String>,
    pub items: Vec<PrintItem>,
    pub table_number: Option<String>,
    pub customer_name: Option<String>,
    pub order_type: Option<String>,
    pub priority: u8,
    pub timestamp: i64,
    pub status: String,
    pub retry_count: u32,
    pub error_message: Option<String>,
}

pub struct QueueManager {
    conn: Arc<Mutex<Connection>>,
    config: QueueConfig,
}

#[derive(Debug, Clone)]
struct QueueConfig {
    max_retries: u32,
    initial_retry_delay_ms: u64,
    max_retry_delay_ms: u64,
    processing_concurrency: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_retry_delay_ms: 2000,    // 2 seconds
            max_retry_delay_ms: 60000,       // 60 seconds
            processing_concurrency: 5,        // 5 concurrent jobs
        }
    }
}

impl QueueManager {
    /// Derive encryption key from restaurant ID using SHA-256
    ///
    /// This creates a deterministic encryption key from the restaurant ID,
    /// allowing the database to be decrypted on daemon restarts.
    ///
    /// # Arguments
    /// * `restaurant_id` - Unique restaurant identifier
    /// * `salt` - Application-specific salt (should be constant)
    ///
    /// # Security Note
    /// In production, this should use PBKDF2 with proper iteration count (100,000+).
    /// Current implementation uses SHA-256 for simplicity during development.
    pub fn derive_key(restaurant_id: &str, salt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(restaurant_id.as_bytes());
        hasher.update(salt.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Create new queue manager with optional encryption
    ///
    /// # Arguments
    /// * `db_path` - Path to SQLite database file
    /// * `encryption_key` - Optional encryption key (derives PBKDF2 key for sqlcipher)
    ///
    /// # Example
    /// ```
    /// let key = QueueManager::derive_key("rest_abc123", "eatsome-print-queue");
    /// let queue = QueueManager::new(db_path, Some(key)).await?;
    /// ```
    pub async fn new(db_path: PathBuf, encryption_key: Option<String>) -> Result<Self> {
        let conn = Connection::open(db_path).await?;

        // Enable encryption if key provided
        if let Some(key) = encryption_key {
            let key_clone = key.clone();
            conn.call(move |conn| {
                conn.pragma_update(None, "key", &key_clone)?;
                Ok(())
            })
            .await
            .map_err(|e| DaemonError::Queue(format!("Failed to set encryption key: {}", e)))?;

            info!("SQLite encryption enabled for print queue");
        }

        // Create tables
        conn.call(|conn| {
            conn.execute(
                r#"
                CREATE TABLE IF NOT EXISTS print_jobs (
                    id TEXT PRIMARY KEY,
                    restaurant_id TEXT NOT NULL,
                    order_id TEXT NOT NULL,
                    order_number TEXT NOT NULL,
                    station TEXT NOT NULL,
                    printer_id TEXT,
                    items TEXT NOT NULL,
                    table_number TEXT,
                    customer_name TEXT,
                    order_type TEXT,
                    priority INTEGER DEFAULT 3,
                    timestamp INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    retry_count INTEGER DEFAULT 0,
                    error_message TEXT,
                    created_at INTEGER DEFAULT (strftime('%s', 'now')),
                    processing_at INTEGER,
                    completed_at INTEGER
                )
                "#,
                [],
            )?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_status ON print_jobs(status)",
                [],
            )?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_restaurant ON print_jobs(restaurant_id)",
                [],
            )?;

            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_priority ON print_jobs(priority, created_at)",
                [],
            )?;

            Ok(())
        })
        .await?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            config: QueueConfig::default(),
        })
    }

    /// Enqueue a new print job with deduplication
    pub async fn enqueue(&self, job: PrintJob) -> Result<()> {
        let conn = self.conn.lock().await;

        let items_json = serde_json::to_string(&job.items)
            .map_err(|e| DaemonError::Queue(format!("Failed to serialize items: {}", e)))?;

        // Check for duplicate job (same order_id + station within last 5 minutes)
        let job_id_clone = job.id.clone();
        let order_id = job.order_id.clone();
        let station = job.station.clone();

        let duplicate_exists = conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT COUNT(*) FROM print_jobs
                    WHERE order_id = ?1
                      AND station = ?2
                      AND status IN ('pending', 'processing')
                      AND created_at > strftime('%s', 'now', '-5 minutes')
                    "#,
                )?;

                let count: i64 = stmt.query_row(rusqlite::params![order_id, station], |row| {
                    row.get(0)
                })?;

                Ok(count > 0)
            })
            .await
            .map_err(|e| DaemonError::Queue(format!("Failed to check duplicate: {}", e)))?;

        if duplicate_exists {
            tracing::warn!("Duplicate job detected for order_id: {}, station: {} - skipping", job.order_id, job.station);
            return Ok(());
        }

        // Insert job
        conn.call(move |conn| {
            conn.execute(
                r#"
                INSERT INTO print_jobs (
                    id, restaurant_id, order_id, order_number, station, printer_id,
                    items, table_number, customer_name, order_type, priority, timestamp, status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                rusqlite::params![
                    job_id_clone,
                    job.restaurant_id,
                    job.order_id,
                    job.order_number,
                    job.station,
                    job.printer_id,
                    items_json,
                    job.table_number,
                    job.customer_name,
                    job.order_type,
                    job.priority,
                    job.timestamp,
                    job.status,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to enqueue job: {}", e)))
    }

    /// Get next pending jobs (ordered by priority and created_at)
    pub async fn get_pending_jobs(&self, limit: usize) -> Result<Vec<PrintJob>> {
        let conn = self.conn.lock().await;

        let jobs = conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, restaurant_id, order_id, order_number, station, printer_id,
                           items, table_number, customer_name, order_type, priority, timestamp,
                           status, retry_count, error_message
                    FROM print_jobs
                    WHERE status = 'pending'
                    ORDER BY priority ASC, created_at ASC
                    LIMIT ?1
                    "#,
                )?;

                let rows = stmt.query_map([limit], |row| {
                    let items_json: String = row.get(6)?;
                    let items: Vec<PrintItem> = serde_json::from_str(&items_json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

                    Ok(PrintJob {
                        id: row.get(0)?,
                        restaurant_id: row.get(1)?,
                        order_id: row.get(2)?,
                        order_number: row.get(3)?,
                        station: row.get(4)?,
                        printer_id: row.get(5)?,
                        items,
                        table_number: row.get(7)?,
                        customer_name: row.get(8)?,
                        order_type: row.get(9)?,
                        priority: row.get(10)?,
                        timestamp: row.get(11)?,
                        status: row.get(12)?,
                        retry_count: row.get(13)?,
                        error_message: row.get(14)?,
                    })
                })?;

                let mut jobs = Vec::new();
                for job_result in rows {
                    jobs.push(job_result?);
                }

                Ok(jobs)
            })
            .await
            .map_err(|e| DaemonError::Queue(format!("Failed to get pending jobs: {}", e)))?;

        Ok(jobs)
    }

    /// Mark job as processing
    pub async fn mark_processing(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = 'processing',
                    processing_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                [job_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as processing: {}", e)))
    }

    /// Mark job as completed
    pub async fn mark_completed(&self, job_id: &str, print_duration_ms: u64) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = 'completed',
                    completed_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                [job_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as completed: {}", e)))
    }

    /// Mark job as failed
    pub async fn mark_failed(&self, job_id: &str, error_message: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();
        let error_message = error_message.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = 'failed',
                    error_message = ?2,
                    retry_count = retry_count + 1,
                    completed_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                rusqlite::params![job_id, error_message],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as failed: {}", e)))
    }

    /// Retry job (reset to pending with incremented retry count)
    pub async fn retry_job(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = 'pending',
                    retry_count = retry_count + 1,
                    processing_at = NULL
                WHERE id = ?1 AND retry_count < 3
                "#,
                [job_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to retry job: {}", e)))
    }

    /// Get queue statistics
    pub async fn get_stats(&self) -> Result<serde_json::Value> {
        let conn = self.conn.lock().await;

        let stats = conn
            .call(|conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT
                        status,
                        COUNT(*) as count
                    FROM print_jobs
                    GROUP BY status
                    "#,
                )?;

                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?;

                let mut stats = serde_json::Map::new();
                for row_result in rows {
                    let (status, count) = row_result?;
                    stats.insert(status, serde_json::json!(count));
                }

                Ok(serde_json::Value::Object(stats))
            })
            .await
            .map_err(|e| DaemonError::Queue(format!("Failed to get stats: {}", e)))?;

        Ok(stats)
    }

    /// Clean up old completed jobs (older than 7 days)
    pub async fn cleanup_old_jobs(&self) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.call(|conn| {
            conn.execute(
                r#"
                DELETE FROM print_jobs
                WHERE status IN ('completed', 'failed')
                  AND completed_at < strftime('%s', 'now', '-7 days')
                "#,
                [],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to cleanup old jobs: {}", e)))
    }

    /// Process a job with exponential backoff retry
    pub async fn process_with_retry<F, Fut>(&self, job_id: &str, process_fn: F) -> Result<()>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let retry_strategy = ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(self.config.initial_retry_delay_ms))
            .with_max_delay(Duration::from_millis(self.config.max_retry_delay_ms))
            .with_max_times(self.config.max_retries as usize);

        let result = (|| async { process_fn().await })
            .retry(retry_strategy)
            .await;

        match result {
            Ok(_) => {
                self.mark_completed(job_id, 0).await?;
                Ok(())
            }
            Err(e) => {
                self.mark_failed(job_id, &e.to_string()).await?;
                Err(e)
            }
        }
    }

    // TODO: Implement start_processor when job processing is needed
    // Currently commented out due to invalid self parameter type (Arc<Mutex<Self>>)
    // See main.rs for stubbed implementation
}
