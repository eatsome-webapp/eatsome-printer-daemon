/**
 * Update Checker Module
 *
 * Checks for updates from GitHub releases on a schedule.
 * Only NOTIFIES the user — never installs automatically.
 * The restaurant owner decides when to update via the UI.
 */

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{info, warn, error};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Update check interval (6 hours)
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Update checker state
pub struct UpdateChecker {
    app: AppHandle,
    available_version: Arc<Mutex<Option<String>>>,
}

impl UpdateChecker {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            available_version: Arc::new(Mutex::new(None)),
        }
    }

    /// Start background update checker (notify-only, never auto-installs)
    pub async fn start(self: Arc<Self>) {
        info!("Starting update checker (notify-only mode)");

        tokio::spawn(async move {
            // First check after 60 seconds (let the app stabilize)
            tokio::time::sleep(Duration::from_secs(60)).await;

            if let Err(e) = self.check_for_update().await {
                error!("Initial update check failed: {}", e);
            }

            let mut interval = interval(CHECK_INTERVAL);

            loop {
                interval.tick().await;

                if let Err(e) = self.check_for_update().await {
                    error!("Update check failed: {}", e);
                }
            }
        });
    }

    /// Check for updates — emit event to frontend if available
    async fn check_for_update(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Checking for updates...");

        let updater = self.app.updater_builder().build()?;

        match updater.check().await {
            Ok(Some(update)) => {
                info!(
                    "Update available: {} -> {}",
                    update.current_version,
                    update.version
                );

                // Store the available version
                {
                    let mut ver = self.available_version.lock().await;
                    *ver = Some(update.version.clone());
                }

                // Notify frontend
                let _ = self.app.emit("update-available", serde_json::json!({
                    "current_version": update.current_version,
                    "latest_version": update.version,
                }));
            }
            Ok(None) => {
                info!("No updates available");

                // Clear any previously stored version
                {
                    let mut ver = self.available_version.lock().await;
                    *ver = None;
                }
            }
            Err(e) => {
                warn!("Update check failed: {}", e);
            }
        }

        Ok(())
    }
}

/// Manual update check (triggered by user clicking "Check for updates")
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<serde_json::Value, String> {
    info!("Manual update check requested");

    let updater = app.updater_builder().build().map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => {
            let _ = app.emit("update-available", serde_json::json!({
                "current_version": update.current_version,
                "latest_version": update.version,
            }));

            Ok(serde_json::json!({
                "available": true,
                "current_version": update.current_version,
                "latest_version": update.version,
            }))
        }
        Ok(None) => Ok(serde_json::json!({
            "available": false,
            "current_version": env!("CARGO_PKG_VERSION"),
        })),
        Err(e) => Err(format!("Update check failed: {}", e)),
    }
}

/// Install update (triggered by user clicking "Update now")
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, String> {
    info!("User-initiated update install");

    let _ = app.emit("update-installing", ());

    let updater = app.updater_builder().build().map_err(|e| e.to_string())?;

    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            info!("Downloading and installing v{}...", version);

            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(_) => {
                    info!("Update v{} installed — restarting", version);
                    let _ = app.emit("update-installed", ());

                    // Short delay so the frontend can show "Restarting..."
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    app.restart();

                    Ok(format!("Updated to v{}", version))
                }
                Err(e) => {
                    error!("Install failed: {}", e);
                    let _ = app.emit("update-error", format!("{}", e));
                    Err(format!("Install failed: {}", e))
                }
            }
        }
        Ok(None) => {
            Err("No update available".to_string())
        }
        Err(e) => {
            Err(format!("Update check failed: {}", e))
        }
    }
}
