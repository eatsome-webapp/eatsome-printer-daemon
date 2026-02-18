/**
 * Auto-Updater Module
 *
 * Handles automatic updates from GitHub releases with smart idle detection.
 * Only installs updates when no print jobs are active to avoid interrupting operations.
 */

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{info, warn, error};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::queue::QueueManager;

/// Update check interval (6 hours)
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Minimum idle time before installing update (5 minutes)
const MIN_IDLE_TIME: Duration = Duration::from_secs(5 * 60);

/// Update checker state
pub struct UpdateChecker {
    app: AppHandle,
    queue_manager: Arc<Mutex<QueueManager>>,
    last_check: Arc<Mutex<Option<std::time::Instant>>>,
}

impl UpdateChecker {
    pub fn new(app: AppHandle, queue_manager: Arc<Mutex<QueueManager>>) -> Self {
        Self {
            app,
            queue_manager,
            last_check: Arc::new(Mutex::new(None)),
        }
    }

    /// Start background update checker
    pub async fn start(self: Arc<Self>) {
        info!("Starting auto-updater background task");

        tokio::spawn(async move {
            let mut interval = interval(CHECK_INTERVAL);

            loop {
                interval.tick().await;

                if let Err(e) = self.check_and_install().await {
                    error!("Update check failed: {}", e);
                }
            }
        });
    }

    /// Check for updates and install if available + system is idle
    async fn check_and_install(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Checking for updates...");

        // Update last check time
        {
            let mut last_check = self.last_check.lock().await;
            *last_check = Some(std::time::Instant::now());
        }

        // Check if update available
        let updater = self.app.updater_builder().build()?;

        match updater.check().await {
            Ok(Some(update)) => {
                info!(
                    "Update available: {} -> {}",
                    update.current_version,
                    update.version
                );

                // Emit event to frontend
                let _ = self.app.emit("update-available", &update.version);

                // Wait for idle state before installing
                self.wait_for_idle().await;

                info!("System idle - installing update");
                let _ = self.app.emit("update-installing", ());

                // Download and install
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(_) => {
                        info!("Update installed successfully - restart required");
                        let _ = self.app.emit("update-installed", ());

                        // Restart application
                        self.app.restart();
                    }
                    Err(e) => {
                        error!("Failed to install update: {}", e);
                        let _ = self.app.emit("update-error", format!("{}", e));
                    }
                }
            }
            Ok(None) => {
                info!("No updates available");
            }
            Err(e) => {
                warn!("Update check failed: {}", e);
            }
        }

        Ok(())
    }

    /// Wait until system is idle (no active print jobs for MIN_IDLE_TIME)
    async fn wait_for_idle(&self) {
        info!("Waiting for idle state before installing update...");

        let mut idle_since: Option<std::time::Instant> = None;

        loop {
            // Check queue depth
            let queue = self.queue_manager.lock().await;
            let stats = queue.get_stats().await.ok();
            drop(queue);

            let is_idle = stats
                .and_then(|s| s.get("pending").and_then(|v| v.as_u64()))
                .map(|pending| pending == 0)
                .unwrap_or(true);

            if is_idle {
                // System is idle
                if let Some(since) = idle_since {
                    if since.elapsed() >= MIN_IDLE_TIME {
                        info!("System has been idle for {:?} - proceeding with update", MIN_IDLE_TIME);
                        return;
                    }
                } else {
                    // Start tracking idle time
                    idle_since = Some(std::time::Instant::now());
                    info!("System became idle - waiting {:?} before update", MIN_IDLE_TIME);
                }
            } else {
                // System is busy - reset idle timer
                if idle_since.is_some() {
                    info!("Print jobs active - resetting idle timer");
                    idle_since = None;
                }
            }

            // Check every 30 seconds
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }

    /// Get time since last check
    #[allow(dead_code)] // Admin utility for monitoring
    pub async fn time_since_last_check(&self) -> Option<Duration> {
        let last_check = self.last_check.lock().await;
        last_check.map(|instant| instant.elapsed())
    }
}

/// Manual update check (triggered by user)
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, String> {
    info!("Manual update check requested");

    let updater = app.updater_builder().build().map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => {
            Ok(format!(
                "Update available: {} -> {}",
                update.current_version,
                update.version
            ))
        }
        Ok(None) => Ok("No updates available".to_string()),
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

/// Get update status (for UI display)
#[tauri::command]
pub async fn get_update_status(
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let updater = app.updater_builder().build().map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => Ok(serde_json::json!({
            "available": true,
            "current_version": update.current_version,
            "latest_version": update.version,
            "release_notes": update.body,
            "release_date": update.date.map(|d| d.to_string()).unwrap_or_else(|| "unknown".to_string()),
        })),
        Ok(None) => Ok(serde_json::json!({
            "available": false,
            "current_version": env!("CARGO_PKG_VERSION"),
        })),
        Err(e) => Err(format!("Failed to check for updates: {}", e)),
    }
}
