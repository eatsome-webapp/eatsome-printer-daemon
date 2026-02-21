use crate::errors::{DaemonError, Result};
use crate::escpos::PrintItem;
use crate::status;
use backon::{ExponentialBuilder, Retryable};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_rusqlite::Connection;
use tracing::{info, warn};
use sha2::Sha256;
use zeroize::Zeroizing;

/// Job priority levels (lower number = higher priority)
#[allow(dead_code)] // Infrastructure: constants document the aging formula and are used by escalate_priority
pub mod priority {
    /// Delivery orders, time-sensitive (dequeues first)
    pub const URGENT: u8 = 1;
    /// Dine-in rush orders
    pub const HIGH: u8 = 2;
    /// Regular dine-in (default)
    pub const NORMAL: u8 = 3;
    /// Prep reminders, non-critical
    pub const LOW: u8 = 4;

    /// Starvation prevention: after this many seconds waiting, a job's
    /// effective priority is boosted by 1 level (e.g., NORMAL → HIGH).
    /// Applied per-level, so after 2x this threshold, LOW → HIGH.
    pub const AGING_THRESHOLD_SECS: i64 = 300; // 5 minutes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJob {
    pub id: String,
    pub restaurant_id: String,
    pub order_id: Option<String>,
    pub order_number: String,
    pub station: String,
    pub station_id: Option<String>,
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
    /// Rate limiter: tracks last enqueue time and count per time window
    rate_limiter: Arc<Mutex<RateLimiterState>>,
}

/// Simple token bucket rate limiter state
struct RateLimiterState {
    /// Enqueue count in current window
    count: u32,
    /// Window start time
    window_start: std::time::Instant,
    /// Max jobs per window
    max_per_window: u32,
    /// Window duration
    window_duration: Duration,
}

impl RateLimiterState {
    fn new() -> Self {
        Self {
            count: 0,
            window_start: std::time::Instant::now(),
            max_per_window: 100, // 100 jobs per minute
            window_duration: Duration::from_secs(60),
        }
    }

    /// Check if rate limit allows a new enqueue
    fn check(&mut self) -> bool {
        let now = std::time::Instant::now();

        // Reset window if expired
        if now.duration_since(self.window_start) >= self.window_duration {
            self.count = 0;
            self.window_start = now;
        }

        if self.count >= self.max_per_window {
            return false;
        }

        self.count += 1;
        true
    }
}

#[allow(dead_code)] // Infrastructure: retry config used by process_with_retry
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
    /// Derive encryption key from restaurant ID using PBKDF2-HMAC-SHA256
    ///
    /// Uses 100,000 iterations for proper key stretching, making brute-force
    /// attacks on the derived key computationally expensive.
    ///
    /// Returns a `Zeroizing<String>` that automatically zeros memory on drop,
    /// preventing the key from lingering in memory after use.
    ///
    /// # Arguments
    /// * `restaurant_id` - Unique restaurant identifier (used as password)
    /// * `salt` - Application-specific salt (combined with fixed prefix)
    pub fn derive_key(restaurant_id: &str, salt: &str) -> Zeroizing<String> {
        let full_salt = format!("eatsome-printer-daemon:{}", salt);
        let key = pbkdf2::pbkdf2_hmac_array::<Sha256, 32>(
            restaurant_id.as_bytes(),
            full_salt.as_bytes(),
            100_000,
        );
        Zeroizing::new(hex::encode(key))
    }

    /// Create new queue manager with optional encryption
    ///
    /// If the database was previously encrypted with a different key (e.g., legacy
    /// SHA-256 derivation), the database is recreated since the print queue contains
    /// only ephemeral job data with a 7-day retention policy.
    ///
    /// The encryption key is wrapped in `Zeroizing<String>` to ensure it's zeroed
    /// from memory after use (defense against memory scanning attacks).
    ///
    /// # Arguments
    /// * `db_path` - Path to SQLite database file
    /// * `encryption_key` - Optional PBKDF2-derived encryption key for sqlcipher
    pub async fn new(db_path: PathBuf, encryption_key: Option<Zeroizing<String>>) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DaemonError::Queue(format!("Failed to create database directory: {}", e)))?;
        }

        let conn = if let Some(ref key) = encryption_key {
            Self::open_encrypted(&db_path, key).await?
        } else {
            Connection::open(&db_path).await
                .map_err(|e| DaemonError::Queue(format!("Failed to open database: {}", e)))?
        };

        // Migration: make order_id nullable (v1.1.6+)
        // SQLite doesn't support ALTER COLUMN, so drop and recreate if needed.
        // Print queue data is ephemeral — safe to recreate.
        conn.call(|conn| {
            let mut stmt = conn.prepare("PRAGMA table_info(print_jobs)")?;
            let mut order_id_notnull = false;
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(1)?;
                let notnull: bool = row.get(3)?;
                Ok((name, notnull))
            })?;
            for row in rows {
                let (name, notnull) = row?;
                if name == "order_id" && notnull {
                    order_id_notnull = true;
                }
            }
            if order_id_notnull {
                tracing::info!("Migrating print_jobs: making order_id nullable");
                conn.execute("DROP TABLE print_jobs", [])?;
            }
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Migration check failed: {}", e)))?;

        // Migration: unify status vocabulary ('processing' → 'printing')
        // Only runs if print_jobs table already exists (skips on fresh/in-memory DBs)
        conn.call(|conn| {
            let table_exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='print_jobs'",
                [],
                |row| row.get(0),
            )?;
            if table_exists {
                let changed = conn.execute(
                    "UPDATE print_jobs SET status = ?1 WHERE status = 'processing'",
                    [status::PRINTING],
                )?;
                if changed > 0 {
                    tracing::info!("Migrated {changed} jobs from 'processing' to 'printing'");
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Status migration failed: {}", e)))?;

        // Migration: add retry_after column (v1.2+)
        // ALTER TABLE ADD COLUMN is safe in SQLite — no-ops if column already exists would error,
        // so we check first.
        conn.call(|conn| {
            let table_exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='print_jobs'",
                [],
                |row| row.get(0),
            )?;
            if table_exists {
                let has_column: bool = conn
                    .prepare("PRAGMA table_info(print_jobs)")?
                    .query_map([], |row| row.get::<_, String>(1))?
                    .any(|name| name.as_deref() == Ok("retry_after"));
                if !has_column {
                    conn.execute("ALTER TABLE print_jobs ADD COLUMN retry_after INTEGER", [])?;
                    tracing::info!("Migrated print_jobs: added retry_after column");
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("retry_after migration failed: {}", e)))?;

        // Create tables
        conn.call(|conn| {
            conn.execute(
                r#"
                CREATE TABLE IF NOT EXISTS print_jobs (
                    id TEXT PRIMARY KEY,
                    restaurant_id TEXT NOT NULL,
                    order_id TEXT,
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
                    completed_at INTEGER,
                    retry_after INTEGER
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
            rate_limiter: Arc::new(Mutex::new(RateLimiterState::new())),
        })
    }

    /// Open an encrypted database, verifying the key works.
    ///
    /// If the key doesn't match (e.g., database was encrypted with legacy SHA-256),
    /// the database file is removed and recreated since print queue data is ephemeral.
    async fn open_encrypted(db_path: &PathBuf, key: &str) -> Result<Connection> {
        let conn = Connection::open(db_path).await
            .map_err(|e| DaemonError::Queue(format!("Failed to open database: {}", e)))?;

        // Set encryption key
        let key_str = key.to_string();
        conn.call(move |conn| {
            conn.pragma_update(None, "key", &key_str)?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to set encryption key: {}", e)))?;

        // Verify key works by querying sqlite_master
        let key_valid = conn
            .call(|conn| {
                match conn.query_row(
                    "SELECT count(*) FROM sqlite_master",
                    [],
                    |row| row.get::<_, i64>(0),
                ) {
                    Ok(_) => Ok(true),
                    Err(_) => Ok(false),
                }
            })
            .await
            .unwrap_or(false);

        if key_valid {
            info!("SQLite encryption verified (PBKDF2-derived key)");
            return Ok(conn);
        }

        // Key mismatch - database was likely encrypted with legacy SHA-256 key
        // Print queue is ephemeral (7-day retention), so recreate is safe
        warn!("Queue database encryption key mismatch (likely legacy SHA-256) - recreating database");
        drop(conn);

        if db_path.exists() {
            std::fs::remove_file(db_path)
                .map_err(|e| DaemonError::Queue(format!("Failed to remove old database: {}", e)))?;
        }

        // Open fresh database with PBKDF2-derived key
        let conn = Connection::open(db_path).await
            .map_err(|e| DaemonError::Queue(format!("Failed to create new database: {}", e)))?;

        let key_str = key.to_string();
        conn.call(move |conn| {
            conn.pragma_update(None, "key", &key_str)?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to set encryption key on new database: {}", e)))?;

        info!("Created new encrypted queue database with PBKDF2-derived key");
        Ok(conn)
    }

    /// Enqueue a new print job with deduplication
    #[tracing::instrument(skip(self, job), fields(job_id = %job.id, order = %job.order_number, station = %job.station))]
    pub async fn enqueue(&self, job: PrintJob) -> Result<()> {
        // Rate limit check (100 jobs/minute)
        {
            let mut limiter = self.rate_limiter.lock().await;
            if !limiter.check() {
                warn!("Rate limit exceeded: >100 jobs/minute - rejecting job {}", job.id);
                return Err(DaemonError::Queue(
                    "Rate limit exceeded: too many print jobs per minute".to_string()
                ));
            }
        }

        let conn = self.conn.lock().await;

        let items_json = serde_json::to_string(&job.items)
            .map_err(|e| DaemonError::Queue(format!("Failed to serialize items: {}", e)))?;

        // Check for duplicate job (same order_id + station within last 5 minutes)
        // Skip deduplication for test prints (order_id is None)
        let job_id_clone = job.id.clone();
        let order_id = job.order_id.clone();
        let station = job.station.clone();

        if let Some(ref oid) = order_id {
            let oid_clone = oid.clone();
            let station_clone = station.clone();

            let duplicate_exists = conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        r#"
                        SELECT COUNT(*) FROM print_jobs
                        WHERE order_id = ?1
                          AND station = ?2
                          AND status IN (?3, ?4)
                          AND created_at > strftime('%s', 'now', '-5 minutes')
                        "#,
                    )?;

                    let count: i64 = stmt.query_row(rusqlite::params![oid_clone, station_clone, status::PENDING, status::PRINTING], |row| {
                        row.get(0)
                    })?;

                    Ok(count > 0)
                })
                .await
                .map_err(|e| DaemonError::Queue(format!("Failed to check duplicate: {}", e)))?;

            if duplicate_exists {
                tracing::warn!("Duplicate job detected for order_id: {}, station: {} - skipping", oid, job.station);
                return Ok(());
            }
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

    /// Get next pending jobs ordered by effective priority with aging.
    ///
    /// Uses priority aging to prevent starvation: for every 5 minutes a job waits,
    /// its effective priority is boosted by 1 level. This ensures low-priority jobs
    /// eventually get processed even when high-priority jobs keep arriving.
    ///
    /// Effective priority = MAX(1, priority - (wait_seconds / 300))
    pub async fn get_pending_jobs(&self, limit: usize) -> Result<Vec<PrintJob>> {
        let conn = self.conn.lock().await;
        let aging_threshold = priority::AGING_THRESHOLD_SECS;

        let jobs = conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT id, restaurant_id, order_id, order_number, station, printer_id,
                           items, table_number, customer_name, order_type, priority, timestamp,
                           status, retry_count, error_message
                    FROM print_jobs
                    WHERE status = ?3
                      AND (retry_after IS NULL OR retry_after <= strftime('%s', 'now'))
                    ORDER BY
                        MAX(1, priority - (strftime('%s', 'now') - created_at) / ?2) ASC,
                        created_at ASC
                    LIMIT ?1
                    "#,
                )?;

                let rows = stmt.query_map(rusqlite::params![limit, aging_threshold, status::PENDING], |row| {
                    let items_json: String = row.get(6)?;
                    let items: Vec<PrintItem> = serde_json::from_str(&items_json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

                    Ok(PrintJob {
                        id: row.get(0)?,
                        restaurant_id: row.get(1)?,
                        order_id: row.get(2)?,
                        order_number: row.get(3)?,
                        station: row.get(4)?,
                        station_id: None, // Local queue doesn't store station_id
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

    /// Mark job as printing
    #[tracing::instrument(skip(self), fields(job_id))]
    pub async fn mark_printing(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = ?2,
                    processing_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                rusqlite::params![job_id, status::PRINTING],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as printing: {}", e)))
    }

    /// Mark job as completed
    #[tracing::instrument(skip(self), fields(job_id, duration_ms = print_duration_ms))]
    pub async fn mark_completed(&self, job_id: &str, print_duration_ms: u64) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = ?2,
                    completed_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                rusqlite::params![job_id, status::COMPLETED],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as completed: {}", e)))
    }

    /// Mark job as failed
    #[tracing::instrument(skip(self), fields(job_id))]
    pub async fn mark_failed(&self, job_id: &str, error_message: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();
        let error_message = error_message.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = ?3,
                    error_message = ?2,
                    retry_count = retry_count + 1,
                    completed_at = strftime('%s', 'now')
                WHERE id = ?1
                "#,
                rusqlite::params![job_id, error_message, status::FAILED],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to mark job as failed: {}", e)))
    }

    /// Retry job with exponential backoff (reset to pending with incremented retry count)
    ///
    /// Backoff formula: delay = min(2^retry_count * 2s, 60s)
    /// retry 0 → 2s, retry 1 → 4s, retry 2 → 8s (max 3 retries)
    pub async fn retry_job(&self, job_id: &str) -> Result<()> {
        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            // Get current retry_count to calculate backoff
            let retry_count: u32 = conn.query_row(
                "SELECT retry_count FROM print_jobs WHERE id = ?1",
                [&job_id],
                |row| row.get(0),
            )?;

            // Exponential backoff: min(2^retry_count * 2, 60) seconds
            let delay_secs = std::cmp::min(2u64.pow(retry_count) * 2, 60);

            conn.execute(
                r#"
                UPDATE print_jobs
                SET status = ?3,
                    retry_count = retry_count + 1,
                    processing_at = NULL,
                    retry_after = strftime('%s', 'now') + ?2
                WHERE id = ?1 AND retry_count < 3
                "#,
                rusqlite::params![job_id, delay_secs, status::PENDING],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to retry job: {}", e)))
    }

    /// Escalate a pending job's priority (lower number = higher priority)
    ///
    /// Used when a job needs urgent attention (e.g., customer waiting).
    /// Clamps to URGENT (1) minimum.
    #[tracing::instrument(skip(self), fields(job_id, new_priority))]
    pub async fn escalate_priority(&self, job_id: &str, new_priority: u8) -> Result<()> {
        let clamped = new_priority.max(priority::URGENT);
        info!("Escalating job {} priority to {}", job_id, clamped);

        let conn = self.conn.lock().await;
        let job_id = job_id.to_string();

        conn.call(move |conn| {
            conn.execute(
                r#"
                UPDATE print_jobs
                SET priority = ?2
                WHERE id = ?1 AND status = ?3
                "#,
                rusqlite::params![job_id, clamped, status::PENDING],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to escalate priority: {}", e)))
    }

    /// Get queue statistics with explicit total, pending, processing, completed, failed counts
    ///
    /// Returns a structured JSON object that the frontend can consume directly.
    /// Uses COALESCE to ensure zero-counts are returned even when no jobs exist.
    pub async fn get_stats(&self) -> Result<serde_json::Value> {
        let conn = self.conn.lock().await;

        let stats = conn
            .call(|conn| {
                let total: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM print_jobs",
                    [],
                    |row| row.get(0),
                )?;

                let pending: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM print_jobs WHERE status = ?1",
                    [status::PENDING],
                    |row| row.get(0),
                )?;

                let printing: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM print_jobs WHERE status = ?1",
                    [status::PRINTING],
                    |row| row.get(0),
                )?;

                let completed: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM print_jobs WHERE status = ?1",
                    [status::COMPLETED],
                    |row| row.get(0),
                )?;

                let failed: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM print_jobs WHERE status = ?1",
                    [status::FAILED],
                    |row| row.get(0),
                )?;

                Ok(serde_json::json!({
                    "total": total,
                    "pending": pending,
                    "printing": printing,
                    "completed": completed,
                    "failed": failed
                }))
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
                WHERE status IN (?1, ?2)
                  AND completed_at < strftime('%s', 'now', '-7 days')
                "#,
                rusqlite::params![status::COMPLETED, status::FAILED],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to cleanup old jobs: {}", e)))
    }

    /// Delete ALL jobs from the queue (used during factory reset)
    pub async fn clear_all_jobs(&self) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.call(|conn| {
            conn.execute("DELETE FROM print_jobs", [])?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to clear all jobs: {}", e)))
    }

    /// Process a job with exponential backoff retry
    #[allow(dead_code)] // Infrastructure: will be called when job processor loop is implemented
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

    /// Flush SQLite WAL (Write-Ahead Log) to main database file
    ///
    /// Called during graceful shutdown to ensure all queued data is persisted.
    /// Uses TRUNCATE mode which reclaims WAL file space.
    pub async fn flush_db(&self) -> Result<()> {
        info!("Flushing SQLite queue database to disk...");
        let conn = self.conn.lock().await;

        conn.call(|conn| {
            conn.execute_batch(
                "PRAGMA wal_checkpoint(TRUNCATE); PRAGMA synchronous = FULL;",
            )?;
            Ok(())
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to flush database: {}", e)))?;

        info!("SQLite queue database flushed successfully");
        Ok(())
    }

    /// Get count of in-progress jobs (for shutdown drain monitoring)
    pub async fn get_processing_count(&self) -> Result<u64> {
        let conn = self.conn.lock().await;

        conn.call(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM print_jobs WHERE status = ?1",
                [status::PRINTING],
                |row| row.get(0),
            )?;
            Ok(count as u64)
        })
        .await
        .map_err(|e| DaemonError::Queue(format!("Failed to count printing jobs: {}", e)))
    }

    // TODO: Implement start_processor when job processing is needed
    // Currently commented out due to invalid self parameter type (Arc<Mutex<Self>>)
    // See main.rs for stubbed implementation
}
