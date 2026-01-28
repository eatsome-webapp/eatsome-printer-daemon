use thiserror::Error;

#[derive(Error, Debug)]
pub enum DaemonError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Printer not found: {0}")]
    PrinterNotFound(String),

    #[error("Printer offline: {0}")]
    PrinterOffline(String),

    #[error("USB error: {0}")]
    Usb(#[from] rusb::Error),

    #[error("Bluetooth error: {0}")]
    Bluetooth(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Database error: {0}")]
    Database(#[from] tokio_rusqlite::Error),

    #[error("Realtime connection error: {0}")]
    Realtime(String),

    #[error("Queue error: {0}")]
    Queue(String),

    #[error("Print job failed: {0}")]
    PrintJob(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DaemonError>;
