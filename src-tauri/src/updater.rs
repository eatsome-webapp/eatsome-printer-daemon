/**
 * Update Checker Module
 *
 * Checks for updates from GitHub releases on a schedule.
 * Only NOTIFIES the user — never installs automatically.
 * The restaurant owner decides when to update via the UI.
 *
 * Linux .deb installs use a custom update flow:
 * Tauri's built-in updater only handles AppImage on Linux.
 * For .deb installs, we fetch latest.json ourselves, download the .deb,
 * and install via `pkexec dpkg -i` (graphical sudo prompt).
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

/// Updater endpoint (must match tauri.conf.json plugins.updater.endpoints[0])
const UPDATER_ENDPOINT: &str =
    "https://github.com/eatsome-webapp/eatsome-printer-daemon/releases/latest/download/latest.json";

/// Temp path for downloaded .deb updates
const DEB_TEMP_PATH: &str = "/tmp/eatsome-printer-update.deb";

// ============================================================================
// Linux .deb Detection & Custom Update Flow
// ============================================================================

/// Detect if we're running from a .deb install (vs AppImage).
///
/// When running as AppImage, the `APPIMAGE` env var is set by the runtime.
/// Its absence on Linux means we were installed via .deb (running from /usr/bin/).
fn is_deb_install() -> bool {
    cfg!(target_os = "linux") && std::env::var("APPIMAGE").is_err()
}

/// Parsed update info from latest.json for .deb installs
#[derive(Debug)]
struct DebUpdateInfo {
    version: String,
    url: String,
}

/// Fetch latest.json and extract the linux-x86_64-deb platform entry.
///
/// The Tauri updater generates a latest.json with platform keys like:
/// - `linux-x86_64` (AppImage)
/// - `linux-x86_64-deb` (.deb package)
/// We specifically need the `-deb` variant.
async fn fetch_deb_update_info() -> Result<Option<DebUpdateInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(UPDATER_ENDPOINT)
        .header("User-Agent", "eatsome-printer-daemon")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest.json: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("latest.json returned status {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse latest.json: {}", e))?;

    let version = json["version"]
        .as_str()
        .ok_or("Missing 'version' in latest.json")?
        .to_string();

    // Check if an update is needed
    let current_version = env!("CARGO_PKG_VERSION");
    if version == current_version {
        return Ok(None);
    }

    // Extract the .deb platform URL
    let deb_entry = &json["platforms"]["linux-x86_64-deb"];
    if deb_entry.is_null() {
        return Err("No linux-x86_64-deb platform in latest.json".to_string());
    }

    let url = deb_entry["url"]
        .as_str()
        .ok_or("Missing 'url' in linux-x86_64-deb platform")?
        .to_string();

    Ok(Some(DebUpdateInfo { version, url }))
}

/// Download the .deb file to a temp location and install via pkexec.
///
/// `pkexec` shows a graphical PolicyKit sudo dialog — no terminal needed.
/// Falls back to an error message if pkexec is unavailable.
async fn install_deb_update(app: &AppHandle, info: &DebUpdateInfo) -> Result<(), String> {
    info!("Downloading .deb update v{} from {}", info.version, info.url);

    // Download .deb to temp file
    let client = reqwest::Client::new();
    let resp = client
        .get(&info.url)
        .header("User-Agent", "eatsome-printer-daemon")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download returned status {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    tokio::fs::write(DEB_TEMP_PATH, &bytes)
        .await
        .map_err(|e| format!("Failed to write {}: {}", DEB_TEMP_PATH, e))?;

    info!(
        "Downloaded {} bytes to {}",
        bytes.len(),
        DEB_TEMP_PATH
    );

    // Install via pkexec dpkg -i (graphical sudo dialog)
    let output = tokio::process::Command::new("pkexec")
        .args(["dpkg", "-i", DEB_TEMP_PATH])
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "pkexec niet gevonden. Handmatig updaten: download .deb van GitHub".to_string()
            } else {
                format!("Failed to run pkexec: {}", e)
            }
        })?;

    // Clean up temp file (best-effort)
    let _ = tokio::fs::remove_file(DEB_TEMP_PATH).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dpkg -i failed: {}", stderr));
    }

    info!("Successfully installed .deb update v{}", info.version);

    // Restart the app
    let _ = app.emit("update-installed", ());
    tokio::time::sleep(Duration::from_millis(500)).await;
    app.restart();
}

// ============================================================================
// Update Checker (Background Loop)
// ============================================================================

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
        info!("Starting update checker (notify-only mode, deb_install={})", is_deb_install());

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

    /// Check for updates — emit event to frontend if available.
    ///
    /// For .deb installs: fetches latest.json directly and compares versions.
    /// For AppImage/macOS/Windows: uses Tauri's built-in updater.
    async fn check_for_update(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Checking for updates...");

        if is_deb_install() {
            return self.check_for_update_deb().await;
        }

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

    /// .deb-specific update check: fetch latest.json and compare versions
    async fn check_for_update_deb(&self) -> Result<(), Box<dyn std::error::Error>> {
        match fetch_deb_update_info().await {
            Ok(Some(info)) => {
                info!(
                    "Update available (deb): {} -> {}",
                    env!("CARGO_PKG_VERSION"),
                    info.version
                );

                {
                    let mut ver = self.available_version.lock().await;
                    *ver = Some(info.version.clone());
                }

                let _ = self.app.emit("update-available", serde_json::json!({
                    "current_version": env!("CARGO_PKG_VERSION"),
                    "latest_version": info.version,
                }));
            }
            Ok(None) => {
                info!("No updates available (deb)");
                let mut ver = self.available_version.lock().await;
                *ver = None;
            }
            Err(e) => {
                warn!("Deb update check failed: {}", e);
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tauri IPC Commands
// ============================================================================

/// Manual update check (triggered by user clicking "Check for updates")
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<serde_json::Value, String> {
    info!("Manual update check requested");

    // For .deb installs, use our custom check
    if is_deb_install() {
        return match fetch_deb_update_info().await {
            Ok(Some(info)) => {
                let _ = app.emit("update-available", serde_json::json!({
                    "current_version": env!("CARGO_PKG_VERSION"),
                    "latest_version": info.version,
                }));

                Ok(serde_json::json!({
                    "available": true,
                    "current_version": env!("CARGO_PKG_VERSION"),
                    "latest_version": info.version,
                }))
            }
            Ok(None) => Ok(serde_json::json!({
                "available": false,
                "current_version": env!("CARGO_PKG_VERSION"),
            })),
            Err(e) => Err(format!("Update check failed: {}", e)),
        };
    }

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
///
/// For .deb installs: downloads .deb and installs via pkexec dpkg -i
/// For AppImage/macOS/Windows: uses Tauri's built-in download_and_install
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, String> {
    info!("User-initiated update install");

    let _ = app.emit("update-installing", ());

    // For .deb installs, use our custom flow
    if is_deb_install() {
        let info = fetch_deb_update_info()
            .await?
            .ok_or("No update available")?;

        install_deb_update(&app, &info).await?;
        return Ok(format!("Updated to v{}", info.version));
    }

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
