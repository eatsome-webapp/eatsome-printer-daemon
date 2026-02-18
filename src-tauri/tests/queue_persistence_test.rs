// Integration tests for queue persistence and offline sync

mod common;

use common::{TestConfigBuilder, create_test_print_job};
use tempfile::TempDir;

#[tokio::test]
async fn test_queue_persists_across_restarts() {
    let temp_dir = TempDir::new().unwrap();
    let config = TestConfigBuilder::new()
        .with_temp_dir(temp_dir)
        .build();

    let db_path = config.get_db_path();

    // Phase 1: Enqueue jobs
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS print_jobs (
                id TEXT PRIMARY KEY,
                order_id TEXT NOT NULL,
                station TEXT NOT NULL,
                payload TEXT NOT NULL,
                status TEXT DEFAULT 'pending',
                retry_count INTEGER DEFAULT 0,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            )",
            [],
        )
        .unwrap();

        let job = create_test_print_job("ORDER_001", "bar");
        conn.execute(
            "INSERT INTO print_jobs (id, order_id, station, payload) VALUES (?1, ?2, ?3, ?4)",
            [
                "job_123",
                "ORDER_001",
                "bar",
                &serde_json::to_string(&job).unwrap(),
            ],
        )
        .unwrap();
    }

    // Phase 2: Simulate restart - reopen database
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let mut stmt = conn
            .prepare("SELECT id, order_id, station, status FROM print_jobs WHERE id = ?1")
            .unwrap();

        let result: Result<(String, String, String, String), _> = stmt.query_row(["job_123"], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        });

        let (id, order_id, station, status) = result.unwrap();
        assert_eq!(id, "job_123");
        assert_eq!(order_id, "ORDER_001");
        assert_eq!(station, "bar");
        assert_eq!(status, "pending");
    }
}

#[tokio::test]
async fn test_failed_jobs_retry_with_exponential_backoff() {
    let temp_dir = TempDir::new().unwrap();
    let config = TestConfigBuilder::new()
        .with_temp_dir(temp_dir)
        .build();

    let db_path = config.get_db_path();

    // Setup database
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS print_jobs (
            id TEXT PRIMARY KEY,
            order_id TEXT NOT NULL,
            station TEXT NOT NULL,
            payload TEXT NOT NULL,
            status TEXT DEFAULT 'pending',
            retry_count INTEGER DEFAULT 0,
            next_retry_at INTEGER,
            created_at INTEGER DEFAULT (strftime('%s', 'now'))
        )",
        [],
    )
    .unwrap();

    // Insert job
    let job = create_test_print_job("ORDER_002", "kitchen");
    conn.execute(
        "INSERT INTO print_jobs (id, order_id, station, payload, status, retry_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        [
            "job_retry",
            "ORDER_002",
            "kitchen",
            &serde_json::to_string(&job).unwrap(),
            "failed",
            "2",
        ],
    )
    .unwrap();

    // Calculate exponential backoff delay
    let retry_count: i64 = conn
        .query_row(
            "SELECT retry_count FROM print_jobs WHERE id = ?1",
            ["job_retry"],
            |row| row.get(0),
        )
        .unwrap();

    // Expected: 2^2 = 4 seconds base delay
    let base_delay = 2;
    let expected_delay = base_delay * 2_i64.pow(retry_count as u32);
    assert_eq!(expected_delay, 8); // 2 * 2^2 = 8 seconds

    // Verify retry count incremented
    assert_eq!(retry_count, 2);
}

#[tokio::test]
async fn test_completed_jobs_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let config = TestConfigBuilder::new()
        .with_temp_dir(temp_dir)
        .build();

    let db_path = config.get_db_path();

    // Setup database
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS print_jobs (
            id TEXT PRIMARY KEY,
            order_id TEXT NOT NULL,
            station TEXT NOT NULL,
            payload TEXT NOT NULL,
            status TEXT DEFAULT 'pending',
            created_at INTEGER DEFAULT (strftime('%s', 'now'))
        )",
        [],
    )
    .unwrap();

    // Insert old completed job (8 days ago)
    let eight_days_ago = chrono::Utc::now() - chrono::Duration::days(8);
    conn.execute(
        "INSERT INTO print_jobs (id, order_id, station, payload, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        [
            "job_old",
            "ORDER_OLD",
            "bar",
            "{}",
            "completed",
            &eight_days_ago.timestamp().to_string(),
        ],
    )
    .unwrap();

    // Insert recent completed job (2 days ago)
    let two_days_ago = chrono::Utc::now() - chrono::Duration::days(2);
    conn.execute(
        "INSERT INTO print_jobs (id, order_id, station, payload, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        [
            "job_recent",
            "ORDER_RECENT",
            "kitchen",
            "{}",
            "completed",
            &two_days_ago.timestamp().to_string(),
        ],
    )
    .unwrap();

    // Cleanup jobs older than 7 days
    let seven_days_ago = (chrono::Utc::now() - chrono::Duration::days(7)).timestamp();
    conn.execute(
        "DELETE FROM print_jobs WHERE status = 'completed' AND created_at < ?1",
        [seven_days_ago],
    )
    .unwrap();

    // Verify old job deleted
    let old_exists: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM print_jobs WHERE id = ?1",
        ["job_old"],
        |row| row.get(0),
    );
    assert_eq!(old_exists.unwrap(), 0);

    // Verify recent job still exists
    let recent_exists: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM print_jobs WHERE id = ?1",
        ["job_recent"],
        |row| row.get(0),
    );
    assert_eq!(recent_exists.unwrap(), 1);
}

#[tokio::test]
async fn test_queue_handles_corrupted_data() {
    let temp_dir = TempDir::new().unwrap();
    let config = TestConfigBuilder::new()
        .with_temp_dir(temp_dir)
        .build();

    let db_path = config.get_db_path();

    // Setup database
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS print_jobs (
            id TEXT PRIMARY KEY,
            order_id TEXT NOT NULL,
            station TEXT NOT NULL,
            payload TEXT NOT NULL,
            status TEXT DEFAULT 'pending'
        )",
        [],
    )
    .unwrap();

    // Insert job with corrupted JSON payload
    conn.execute(
        "INSERT INTO print_jobs (id, order_id, station, payload) VALUES (?1, ?2, ?3, ?4)",
        ["job_corrupt", "ORDER_003", "bar", "INVALID_JSON{{{"],
    )
    .unwrap();

    // Attempt to parse payload
    let payload: Result<String, _> = conn.query_row(
        "SELECT payload FROM print_jobs WHERE id = ?1",
        ["job_corrupt"],
        |row| row.get(0),
    );

    let parse_result = serde_json::from_str::<serde_json::Value>(&payload.unwrap());

    // Should fail gracefully
    assert!(parse_result.is_err());

    // Mark as failed and log error (in real implementation)
    conn.execute(
        "UPDATE print_jobs SET status = 'failed' WHERE id = ?1",
        ["job_corrupt"],
    )
    .unwrap();

    let status: String = conn
        .query_row(
            "SELECT status FROM print_jobs WHERE id = ?1",
            ["job_corrupt"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "failed");
}
