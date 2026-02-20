// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Emitter, Manager, State};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri_plugin_store::StoreExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, error, warn, debug};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
#[allow(dead_code)] // ESC/POS protocol library: not all builder methods/enums used yet
mod escpos;
mod printer;
mod queue;
mod job_poller;
#[allow(dead_code)] // Discovery helpers: wrapper functions with default timeouts
mod discovery;
mod errors;
#[allow(dead_code)] // Auth library: token generation/validation API surface
mod auth;
mod circuit_breaker;
mod telemetry;
mod api;
mod status;
mod updater;
mod sentry_init;
mod supabase_client;

use config::AppConfig;
use printer::PrinterManager;
use queue::QueueManager;
use job_poller::JobPoller;
use auth::JWTManager;
use telemetry::{TelemetryCollector, TelemetryReporter};
use errors::DaemonError;
use supabase_client::SupabaseClient;
use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};

/// Per-printer circuit breaker registry
pub struct CircuitBreakerRegistry {
    breakers: Mutex<std::collections::HashMap<String, Arc<CircuitBreaker>>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    fn new() -> Self {
        Self {
            breakers: Mutex::new(std::collections::HashMap::new()),
            config: CircuitBreakerConfig::default(),
        }
    }

    /// Get or create a circuit breaker for a printer
    async fn get_breaker(&self, printer_id: &str) -> Arc<CircuitBreaker> {
        let mut breakers = self.breakers.lock().await;
        breakers
            .entry(printer_id.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(
                    printer_id.to_string(),
                    self.config.clone(),
                ))
            })
            .clone()
    }
}

/// Global application state
pub struct AppState {
    config: Arc<Mutex<AppConfig>>,
    printer_manager: Arc<Mutex<PrinterManager>>,
    queue_manager: Arc<Mutex<QueueManager>>,
    job_poller_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    telemetry: Arc<TelemetryCollector>,
    #[allow(dead_code)] // Used by API server (api.rs), not directly from main
    jwt_manager: Arc<JWTManager>,
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    start_time: Instant,
    /// Shutdown flag: when true, background tasks should drain and stop
    shutdown_requested: Arc<AtomicBool>,
}

// ============================================================================
// Tauri IPC Commands
// ============================================================================

/// Get current configuration
#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let mut config = state.config.lock().await.clone();
    // Always return the compiled version, not the stored one (which may be stale after updates)
    config.version = env!("CARGO_PKG_VERSION").to_string();
    Ok(config)
}

/// Validate restaurant identifier (UUID or restaurant_code)
///
/// Accepts either:
/// - UUID format (e.g., "0faee837-c64f-4ac9-ae2c-62f4f07e0054")
/// - Short restaurant code (e.g., "W434N") — will be resolved to UUID via Supabase
fn validate_restaurant_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("Restaurant ID cannot be empty".to_string());
    }
    // Accept both UUID and short restaurant codes (alphanumeric, 3-10 chars)
    if uuid::Uuid::parse_str(trimmed).is_ok() {
        return Ok(());
    }
    if trimmed.len() >= 3 && trimmed.len() <= 10 && trimmed.chars().all(|c| c.is_alphanumeric()) {
        return Ok(());
    }
    Err(format!(
        "Invalid restaurant identifier: '{}'. Use your restaurant code (e.g., W434N) or UUID.",
        trimmed
    ))
}

/// Save configuration
#[tauri::command]
async fn save_config(
    config: AppConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut config = config;

    // Validate and resolve restaurant identifier
    if let Some(ref restaurant_id) = config.restaurant_id {
        validate_restaurant_id(restaurant_id)?;

        // If not a UUID, treat as restaurant_code and resolve to UUID via Supabase
        if uuid::Uuid::parse_str(restaurant_id.trim()).is_err() {
            let code = restaurant_id.trim().to_uppercase();
            info!("Resolving restaurant code '{}' to UUID...", code);

            // Use anon key for public lookup (no auth_token needed for setup)
            let lookup_key = if config.supabase_anon_key.is_empty() {
                AppConfig::default().supabase_anon_key
            } else {
                config.supabase_anon_key.clone()
            };

            let supabase = SupabaseClient::new(
                config.supabase_url.clone(),
                lookup_key,
                None, // No auth_token needed for restaurant code resolution
            );

            match supabase.resolve_restaurant_code(&code).await {
                Ok(Some(uuid)) => {
                    info!("Resolved restaurant code '{}' → UUID '{}'", code, uuid);
                    config.restaurant_id = Some(uuid);
                }
                Ok(None) => {
                    return Err(format!(
                        "Restaurant code '{}' not found. Check your code and try again.",
                        code
                    ));
                }
                Err(e) => {
                    return Err(format!(
                        "Could not look up restaurant code '{}': {}",
                        code, e
                    ));
                }
            }
        }
    }

    let mut app_config = state.config.lock().await;
    *app_config = config.clone();

    // Save to Tauri store
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("config", serde_json::to_value(&config).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;

    info!("✅ Configuration saved to local store");

    // Sync printers to PrinterManager so test_print works immediately
    {
        let pm = state.printer_manager.lock().await;
        // Clear existing and re-add from config
        for printer in &config.printers {
            pm.add_printer(printer.clone()).await;
        }
    }

    // Sync printers to Supabase via Edge Function
    if let Some(restaurant_id) = &config.restaurant_id {
        if !config.printers.is_empty() && config.auth_token.is_some() {
            info!("Syncing {} printers to Supabase...", config.printers.len());

            let supabase_client = SupabaseClient::new(
                config.supabase_url.clone(),
                config.supabase_anon_key.clone(),
                config.auth_token.clone(),
            );

            let printers_upsert: Vec<supabase_client::PrinterUpsert> = config
                .printers
                .iter()
                .map(|p| supabase_client::PrinterUpsert {
                    id: p.id.clone(),
                    restaurant_id: restaurant_id.clone(),
                    name: p.name.clone(),
                    connection_type: format!("{:?}", p.connection_type).to_lowercase(),
                    address: p.address.clone(),
                    protocol: p.protocol.clone(),
                    capabilities: serde_json::to_value(&p.capabilities)
                        .unwrap_or(serde_json::json!({})),
                    status: "online".to_string(),
                    last_seen: chrono::Utc::now().to_rfc3339(),
                })
                .collect();

            match supabase_client.upsert_printers(printers_upsert).await {
                Ok(_) => {
                    info!("✅ Printers synced to Supabase successfully");
                }
                Err(e) => {
                    error!("❌ Failed to sync printers to Supabase: {}", e);
                    // Don't fail the entire save operation
                    // Printers are still saved locally and will sync on next heartbeat
                    warn!("⚠️  Continuing without Supabase sync (printers saved locally)");
                }
            }
        }
    }

    Ok(())
}

/// Claim a pairing code via the restaurant webapp API
/// Returns the auth token, restaurant ID, and restaurant code
#[tauri::command]
async fn claim_pairing_code(
    code: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    info!("Claiming pairing code: {}...", &code[..std::cmp::min(2, code.len())]);

    // Validate code format (9 digits)
    let trimmed = code.trim();
    if trimmed.len() != 9 || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err("Ongeldige code. Vul 9 cijfers in.".to_string());
    }

    let mut config = state.config.lock().await;
    let webapp_url = config.webapp_url.clone();
    let supabase_url = config.supabase_url.clone();
    let anon_key = config.supabase_anon_key.clone();

    // Get or create a persistent client_id (reused across pairings)
    let client_id = config.client_id.clone().unwrap_or_else(|| {
        let new_id = uuid::Uuid::new_v4().to_string();
        info!("Generated new persistent client_id: {}", new_id);
        new_id
    });

    // Persist client_id if it was just generated
    if config.client_id.is_none() {
        config.client_id = Some(client_id.clone());
        let store = app.store("config.json").map_err(|e| e.to_string())?;
        store.set("config", serde_json::to_value(&*config).map_err(|e| e.to_string())?);
        store.save().map_err(|e| e.to_string())?;
    }
    drop(config);

    // Build client info from system
    let client_info = serde_json::json!({
        "clientId": client_id,
        "name": "Eatsome Printer Service",
        "platform": std::env::consts::OS,
        "version": env!("CARGO_PKG_VERSION"),
    });

    // Create a temporary SupabaseClient (no auth_token yet — we're pairing)
    let client = SupabaseClient::new(supabase_url, anon_key, None);

    let result = client
        .claim_pairing_code(&webapp_url, trimmed, &client_info)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "token": result.token,
        "restaurantId": result.restaurant_id,
        "restaurantCode": result.restaurant_code,
        "expiresIn": result.expires_in,
    }))
}

/// Discover all printers (USB + Network + Bluetooth) with ESC/POS protocol probing
#[tauri::command]
async fn discover_printers(
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    info!("Printer discovery requested (force: {:?})", force);
    let manager = state.printer_manager.lock().await;
    let results = manager.discover_all(force.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())?;

    // Post-discovery: probe unknown printers for ESC/POS support
    // This converts protocol "unknown" → "escpos" or "unsupported"
    let mut printers: Vec<discovery::DiscoveredPrinter> = results
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();

    if printers.iter().any(|p| p.protocol == "unknown") {
        info!("Probing {} printers for ESC/POS protocol support...",
            printers.iter().filter(|p| p.protocol == "unknown").count());
        discovery::probe_unknown_printers(&mut printers).await;
    }

    // Convert back to JSON values
    let json_results: Vec<serde_json::Value> = printers
        .iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .collect();

    Ok(json_results)
}

/// Test print on a specific printer (already added to config)
#[tauri::command]
async fn test_print(
    printer_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Test print requested for printer: {}", printer_id);
    let manager = state.printer_manager.lock().await;
    manager.test_print(&printer_id)
        .await
        .map_err(|e| e.to_string())
}

/// Test print on a discovered printer (not yet added to config)
#[tauri::command]
async fn test_discovered_printer(
    address: String,
    connection_type: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Test print requested for discovered printer: {} ({})", address, connection_type);
    let manager = state.printer_manager.lock().await;
    manager.test_print_direct(&address, &connection_type)
        .await
        .map_err(|e| e.to_string())
}

/// Start polling for print jobs via Edge Function
///
/// Validates the restaurant ID and auth_token before starting the poller.
/// Gathers printer_ids from config for heartbeat piggyback on each poll.
#[tauri::command]
async fn start_polling(
    restaurant_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Job polling requested for restaurant: {}", restaurant_id);

    // Step 1: Validate UUID format
    validate_restaurant_id(&restaurant_id)?;

    let config = state.config.lock().await;

    // Step 2: Check auth_token exists
    if config.auth_token.is_none() {
        return Err("No auth_token configured. Generate one from POS Devices page.".to_string());
    }

    let supabase_client = Arc::new(SupabaseClient::new(
        config.supabase_url.clone(),
        config.supabase_anon_key.clone(),
        config.auth_token.clone(),
    ));

    // Gather printer_ids for heartbeat piggyback
    let printer_ids: Vec<String> = config.printers.iter().map(|p| p.id.clone()).collect();
    drop(config);

    // Stop existing poller first (prevents duplicates from React strict mode)
    {
        let mut handle = state.job_poller_handle.lock().await;
        if let Some(old_handle) = handle.take() {
            info!("Stopping existing job poller before restarting");
            old_handle.abort();
        }
    }

    // Start the job poller with printer_ids for heartbeat piggyback
    let queue = state.queue_manager.clone();
    let poller_handle = JobPoller::start(
        restaurant_id.clone(),
        supabase_client,
        queue,
        printer_ids,
    );

    let mut handle = state.job_poller_handle.lock().await;
    *handle = Some(poller_handle);

    info!("Job polling started for restaurant {}", restaurant_id);

    // Update Sentry context with restaurant info
    sentry_init::set_restaurant_context(&restaurant_id);
    sentry_init::set_user_context(&restaurant_id);

    Ok(())
}

/// Stop polling for print jobs
#[tauri::command]
async fn stop_polling(state: State<'_, AppState>) -> Result<(), String> {
    info!("Job polling stop requested");

    let mut handle = state.job_poller_handle.lock().await;
    if let Some(h) = handle.take() {
        h.abort();
    }

    info!("Job polling stopped");
    Ok(())
}

/// Get queue statistics
#[tauri::command]
async fn get_queue_stats(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let queue = state.queue_manager.lock().await;
    queue.get_stats().await.map_err(|e| e.to_string())
}

/// Get telemetry metrics
#[tauri::command]
async fn get_metrics(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    Ok(state.telemetry.get_metrics_json().await)
}

/// Get polling connection state
///
/// Returns "connected" if the job poller is running, "disconnected" otherwise.
#[tauri::command]
async fn get_connection_state(state: State<'_, AppState>) -> Result<String, String> {
    let handle = state.job_poller_handle.lock().await;
    if let Some(h) = handle.as_ref() {
        if !h.is_finished() {
            return Ok("connected".to_string());
        }
    }
    Ok("disconnected".to_string())
}

/// Check if printer is online
#[tauri::command]
async fn is_printer_online(
    printer_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let manager = state.printer_manager.lock().await;
    Ok(manager.is_online(&printer_id).await)
}

/// Add printer to configuration
#[tauri::command]
async fn add_printer(
    printer: config::PrinterConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Adding printer: {} ({})", printer.name, printer.id);

    let manager = state.printer_manager.lock().await;
    manager.add_printer(printer.clone()).await;

    // Update config
    let mut config = state.config.lock().await;
    config.printers.push(printer);

    // Save to Tauri store
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("config", serde_json::to_value(&*config).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;

    Ok(())
}

/// Remove printer from configuration
#[tauri::command]
async fn remove_printer(
    printer_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Removing printer: {}", printer_id);

    let manager = state.printer_manager.lock().await;
    manager.remove_printer(&printer_id).await;

    // Update config
    let mut config = state.config.lock().await;
    config.printers.retain(|p| p.id != printer_id);

    // Save to Tauri store
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("config", serde_json::to_value(&*config).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;

    Ok(())
}

/// Get daemon uptime in seconds
#[tauri::command]
async fn get_uptime(state: State<'_, AppState>) -> Result<u64, String> {
    Ok(state.start_time.elapsed().as_secs())
}

/// Generate a print preview for a test receipt
///
/// Returns a parsed receipt structure that the frontend can render
/// using monospace fonts to simulate thermal printer output.
#[tauri::command]
async fn preview_test_print() -> Result<escpos::ParsedReceipt, String> {
    let commands = escpos::format_test_print(escpos::PaperWidth::Width80mm);
    Ok(escpos::parse_escpos(&commands, escpos::PaperWidth::Width80mm))
}

/// Generate a print preview for a kitchen receipt
#[tauri::command]
async fn preview_kitchen_receipt(
    station: String,
    order_number: String,
    order_type: Option<String>,
    table_number: Option<String>,
    customer_name: Option<String>,
    priority: u8,
    items: Vec<escpos::PrintItem>,
) -> Result<escpos::ParsedReceipt, String> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let commands = escpos::format_kitchen_receipt(
        &station,
        &order_number,
        order_type.as_deref(),
        table_number.as_deref(),
        customer_name.as_deref(),
        priority,
        &items,
        timestamp,
        escpos::PaperWidth::Width80mm,
    );
    Ok(escpos::parse_escpos(&commands, escpos::PaperWidth::Width80mm))
}

/// Escalate a pending job's priority (lower = higher priority, min 1)
#[tauri::command]
async fn escalate_job_priority(
    job_id: String,
    new_priority: u8,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Escalating job {} priority to {}", job_id, new_priority);
    let queue = state.queue_manager.lock().await;
    queue.escalate_priority(&job_id, new_priority).await.map_err(|e| e.to_string())
}

/// Get circuit breaker status for a specific printer
#[tauri::command]
async fn get_circuit_breaker_status(
    printer_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let breaker = state.circuit_breakers.get_breaker(&printer_id).await;
    let status = breaker.get_status().await;
    serde_json::to_value(status).map_err(|e| e.to_string())
}

/// Reset circuit breaker for a specific printer (admin function)
#[tauri::command]
async fn reset_circuit_breaker(
    printer_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Resetting circuit breaker for printer: {}", printer_id);
    let breaker = state.circuit_breakers.get_breaker(&printer_id).await;
    breaker.reset().await;
    Ok(())
}

/// Manually trigger queue cleanup (remove old completed/failed jobs)
#[tauri::command]
async fn cleanup_queue(state: State<'_, AppState>) -> Result<(), String> {
    info!("Manual queue cleanup requested");
    let queue = state.queue_manager.lock().await;
    queue.cleanup_old_jobs().await.map_err(|e| e.to_string())
}

/// Clear all jobs from the queue (used during factory reset)
#[tauri::command]
async fn clear_queue(state: State<'_, AppState>) -> Result<(), String> {
    info!("Full queue clear requested (factory reset)");
    let queue = state.queue_manager.lock().await;
    queue.clear_all_jobs().await.map_err(|e| e.to_string())
}

/// Get event history from telemetry
#[tauri::command]
async fn get_event_history(
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<(u64, telemetry::TelemetryEvent)>, String> {
    Ok(state.telemetry.get_event_history(limit).await)
}

/// Read last N lines from log file for debugging
#[tauri::command]
async fn get_log_tail(lines: usize) -> Result<String, String> {
    let log_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("Logs")
        .join("EatsomePrinterService")
        .join("app.log");

    match std::fs::read_to_string(&log_path) {
        Ok(content) => {
            let all_lines: Vec<&str> = content.lines().collect();
            let start_index = all_lines.len().saturating_sub(lines);
            let tail_lines: Vec<&str> = all_lines[start_index..].to_vec();
            Ok(tail_lines.join("\n"))
        }
        Err(e) => Err(format!("Failed to read log file: {}", e)),
    }
}

/// Get log file path for user reference
#[tauri::command]
async fn get_log_path() -> Result<String, String> {
    let log_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("Logs")
        .join("EatsomePrinterService")
        .join("app.log");

    Ok(log_path.display().to_string())
}

// ============================================================================
// System Tray
// ============================================================================

fn setup_system_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Create menu items
    let status = MenuItem::with_id(app, "status", "Status: Idle", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show Dashboard", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide Window", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, Some("cmd+q"))?;

    // Build menu
    let menu = Menu::with_items(app, &[&status, &show, &hide, &quit])?;

    // Create tray icon
    let mut tray_builder = TrayIconBuilder::new()
        .tooltip("Eatsome Printer Service")
        .menu(&menu);

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    } else {
        warn!("No default window icon found for system tray");
    }

    let _tray = tray_builder
        .on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "quit" => {
                    info!("Graceful shutdown initiated from tray menu");
                    let state = app.state::<AppState>();
                    state.shutdown_requested.store(true, Ordering::SeqCst);

                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app_handle.state::<AppState>();

                        // Drain: wait for in-flight jobs to complete (max 10s)
                        for i in 0..20 {
                            let queue = state.queue_manager.lock().await;
                            match queue.get_processing_count().await {
                                Ok(0) => {
                                    info!("All in-flight jobs drained after {}ms", i * 500);
                                    break;
                                }
                                Ok(count) => {
                                    debug!("Draining {} in-flight jobs... ({}ms elapsed)", count, i * 500);
                                }
                                Err(e) => {
                                    warn!("Failed to check processing count: {}", e);
                                    break;
                                }
                            }
                            drop(queue);
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }

                        // Flush SQLite WAL to ensure queue data is persisted
                        let queue = state.queue_manager.lock().await;
                        if let Err(e) = queue.flush_db().await {
                            error!("Failed to flush queue on shutdown: {}", e);
                        }
                        drop(queue);

                        info!("Graceful shutdown complete, exiting");
                        app_handle.exit(0);
                    });
                }
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                "hide" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                        info!("Window hidden to system tray");
                    }
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click on tray icon → toggle window visibility
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    // Intercept window close → hide to tray instead of quitting
    // This is critical for a daemon: closing the window must NOT stop the print service
    if let Some(window) = app.get_webview_window("main") {
        let win = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win.hide();
                info!("Window close intercepted - hidden to system tray");
            }
        });
    }

    Ok(())
}

// ============================================================================
// Background Tasks
// ============================================================================

/// Create a SupabaseClient from the current config, if possible.
/// Returns None if restaurant_id or auth_token is missing.
fn create_supabase_client_from_config(cfg: &AppConfig) -> Option<SupabaseClient> {
    cfg.restaurant_id.as_ref()?;
    if cfg.auth_token.is_none() {
        debug!("No auth_token configured, skipping Supabase client creation");
        return None;
    }
    Some(SupabaseClient::new(
        cfg.supabase_url.clone(),
        cfg.supabase_anon_key.clone(),
        cfg.auth_token.clone(),
    ))
}

/// Start background job processor with parallel execution, circuit breaker, and failover
async fn start_job_processor(
    queue_manager: Arc<Mutex<QueueManager>>,
    printer_manager: Arc<Mutex<PrinterManager>>,
    telemetry: Arc<TelemetryCollector>,
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    config: Arc<Mutex<AppConfig>>,
    shutdown: Arc<AtomicBool>,
) {
    info!("Starting background job processor (concurrency: 5, failover: enabled)");
    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));

    tokio::spawn(async move {
        let mut poll_interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

        loop {
            poll_interval.tick().await;

            // Check shutdown flag
            if shutdown.load(Ordering::Relaxed) {
                info!("Job processor stopping (shutdown requested)");
                break;
            }

            // Get pending jobs from queue
            let queue = queue_manager.lock().await;
            let pending_jobs = match queue.get_pending_jobs(5).await {
                Ok(jobs) => jobs,
                Err(e) => {
                    error!("Failed to get pending jobs: {}", e);
                    continue;
                }
            };
            drop(queue);

            if pending_jobs.is_empty() {
                continue;
            }

            debug!("Processing {} pending jobs", pending_jobs.len());

            for job in pending_jobs {
                let queue_mgr = queue_manager.clone();
                let printer_mgr = printer_manager.clone();
                let telem = telemetry.clone();
                let breakers = circuit_breakers.clone();
                let permit = semaphore.clone();
                let cfg = config.clone();

                tokio::spawn(async move {
                    // Acquire semaphore permit (limits concurrency to 5)
                    let _permit = match permit.acquire().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };

                    let job_id = job.id.clone();
                    let printer_id = job.printer_id.clone().unwrap_or_else(|| "unknown".to_string());
                    let start = std::time::Instant::now();

                    // Create Supabase client for status reporting (best-effort)
                    let supabase = {
                        let config_guard = cfg.lock().await;
                        create_supabase_client_from_config(&config_guard)
                    };

                    // Mark as processing (local + Supabase)
                    {
                        let queue = queue_mgr.lock().await;
                        if let Err(e) = queue.mark_printing(&job_id).await {
                            error!("Failed to mark job {} as printing: {}", job_id, e);
                            return;
                        }
                    }
                    if let Some(ref client) = supabase {
                        let _ = client.update_job_status(&job_id, status::PRINTING, None, None).await;
                    }

                    // Execute print with circuit breaker (120s total timeout)
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(120),
                        try_print_with_failover(
                            &printer_id,
                            &job,
                            &printer_mgr,
                            &breakers,
                        ),
                    ).await;

                    // Flatten timeout result
                    let result = match result {
                        Ok(inner) => inner,
                        Err(_) => {
                            error!("Print job {} timed out after 120s", job_id);
                            Err(DaemonError::PrintJob("Total job timeout exceeded (120s)".to_string()))
                        }
                    };

                    let duration_ms = start.elapsed().as_millis() as u64;

                    match result {
                        Ok(used_printer) => {
                            // Mark completed locally
                            let queue = queue_mgr.lock().await;
                            let _ = queue.mark_completed(&job_id, duration_ms).await;
                            drop(queue);

                            // Report to Supabase (best-effort, fire-and-forget)
                            if let Some(ref client) = supabase {
                                let _ = client.update_job_status(&job_id, status::COMPLETED, None, Some(duration_ms)).await;
                                let _ = client.insert_job_log(
                                    &job.restaurant_id,
                                    job.order_id.as_deref(),
                                    Some(&used_printer),
                                    None, // station_id is a UUID, job.station is name — skip for now
                                    status::COMPLETED,
                                    None,
                                    Some(duration_ms),
                                    job.retry_count as i32,
                                ).await;
                            }

                            telem.record_event(telemetry::TelemetryEvent::PrintJobCompleted {
                                job_id: job_id.clone(),
                                order_number: job.order_number.clone(),
                                station: job.station.clone(),
                                printer_id: used_printer.clone(),
                                duration_ms,
                                retry_count: job.retry_count,
                            }).await;
                            if used_printer != printer_id {
                                warn!("Print job {} completed via failover to {} ({}ms)", job_id, used_printer, duration_ms);
                            } else {
                                info!("Print job {} completed in {}ms", job_id, duration_ms);
                            }
                        }
                        Err(e) => {
                            let queue = queue_mgr.lock().await;
                            let _ = queue.mark_failed(&job_id, &e.to_string()).await;

                            // Auto-retry: if under max retries, reset to pending
                            if job.retry_count < 3 {
                                match queue.retry_job(&job_id).await {
                                    Ok(_) => {
                                        drop(queue);
                                        // Report retry to Supabase
                                        if let Some(ref client) = supabase {
                                            let _ = client.update_job_status(&job_id, status::PENDING, None, None).await;
                                        }
                                        warn!(
                                            "Print job {} failed (attempt {}/3), re-queued for retry: {}",
                                            job_id, job.retry_count + 1, e
                                        );
                                    }
                                    Err(retry_err) => {
                                        error!("Failed to re-queue job {} for retry: {}", job_id, retry_err);
                                    }
                                }
                            } else {
                                drop(queue);
                                // Permanently failed — report to Supabase
                                if let Some(ref client) = supabase {
                                    let _ = client.update_job_status(&job_id, status::FAILED, Some(&e.to_string()), None).await;
                                    let _ = client.insert_job_log(
                                        &job.restaurant_id,
                                        job.order_id.as_deref(),
                                        Some(&printer_id),
                                        None,
                                        status::FAILED,
                                        Some(&e.to_string()),
                                        None,
                                        job.retry_count as i32,
                                    ).await;
                                }

                                telem.record_event(telemetry::TelemetryEvent::PrintJobFailed {
                                    job_id: job_id.clone(),
                                    order_number: job.order_number.clone(),
                                    station: job.station.clone(),
                                    printer_id: Some(printer_id.clone()),
                                    error: e.to_string(),
                                    retry_count: job.retry_count,
                                }).await;
                                error!("Print job {} permanently failed after {} retries: {}", job_id, job.retry_count, e);
                                sentry_init::capture_print_job_failure(&job_id, &e.to_string(), &printer_id);
                            }
                        }
                    }
                });
            }
        }
    });
}

/// Try printing on the specified printer with circuit breaker protection.
/// Returns the printer_id that successfully printed.
async fn try_print_with_failover(
    printer_id: &str,
    job: &queue::PrintJob,
    printer_manager: &Arc<Mutex<PrinterManager>>,
    circuit_breakers: &Arc<CircuitBreakerRegistry>,
) -> errors::Result<String> {
    let breaker = circuit_breakers.get_breaker(printer_id).await;
    let pm = printer_manager.clone();
    let pid = printer_id.to_string();
    let job_clone = job.clone();

    let result = breaker.execute(|| {
        let pm = pm.clone();
        let pid = pid.clone();
        let job_clone = job_clone.clone();
        async move {
            let manager = pm.lock().await;
            manager.print_to_printer(&pid, &job_clone).await
        }
    }).await;

    match result {
        Ok(_) => Ok(printer_id.to_string()),
        Err(e) => {
            warn!("Printer {} failed for job {}: {}", printer_id, job.id, e);
            Err(e)
        }
    }
}

/// Register printers in Supabase on startup (upsert once, retry until success).
///
/// Heartbeat updates are now piggybacked on poll-jobs calls (Wave B),
/// so this function only needs to run once to register printer records.
/// Retries every 60s until successful, then stops.
async fn start_printer_registration(
    config: Arc<Mutex<AppConfig>>,
    telemetry: Arc<TelemetryCollector>,
) {
    info!("Starting printer registration (one-time upsert with retry)");

    tokio::spawn(async move {
        loop {
            let cfg = config.lock().await;
            let restaurant_id = match &cfg.restaurant_id {
                Some(id) => id.clone(),
                None => {
                    drop(cfg);
                    // No restaurant configured yet — wait and retry
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue;
                }
            };
            let supabase_url = cfg.supabase_url.clone();
            let anon_key = cfg.supabase_anon_key.clone();
            let auth_token = cfg.auth_token.clone();
            let printer_configs = cfg.printers.clone();
            drop(cfg);

            if printer_configs.is_empty() {
                debug!("No printers configured, skipping registration");
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                continue;
            }

            if auth_token.is_none() {
                warn!("No auth_token configured, skipping registration");
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                continue;
            }

            let client = SupabaseClient::new(supabase_url, anon_key, auth_token);

            let now = chrono::Utc::now().to_rfc3339();
            let printers_to_upsert: Vec<supabase_client::PrinterUpsert> = printer_configs
                .iter()
                .map(|p| {
                    let conn_type = match p.connection_type {
                        config::ConnectionType::USB => "usb",
                        config::ConnectionType::Network => "network",
                        config::ConnectionType::Bluetooth => "bluetooth",
                    };
                    supabase_client::PrinterUpsert {
                        id: p.id.clone(),
                        restaurant_id: restaurant_id.clone(),
                        name: p.name.clone(),
                        connection_type: conn_type.to_string(),
                        address: p.address.clone(),
                        protocol: p.protocol.clone(),
                        capabilities: serde_json::to_value(&p.capabilities).unwrap_or_default(),
                        status: "online".to_string(),
                        last_seen: now.clone(),
                    }
                })
                .collect();

            let printer_count = printers_to_upsert.len();
            match client.upsert_printers(printers_to_upsert).await {
                Ok(_) => {
                    info!("Registered {} printers in Supabase (one-time)", printer_count);
                    telemetry.update_printer_counts(printer_count, 0).await;
                    // Success — stop retrying. Heartbeats are now handled by poll-jobs piggyback.
                    break;
                }
                Err(e) => {
                    warn!("Failed to register printers: {}. Retrying in 60s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            }
        }
    });
}

/// Start periodic queue metrics snapshot (every 30s) with Tauri event push
async fn start_queue_metrics(
    queue_manager: Arc<Mutex<QueueManager>>,
    telemetry: Arc<TelemetryCollector>,
    app_handle: Option<tauri::AppHandle>,
) {
    info!("Starting queue metrics snapshot (30s interval, events: {})", app_handle.is_some());

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

        loop {
            interval.tick().await;

            let queue = queue_manager.lock().await;
            if let Ok(stats) = queue.get_stats().await {
                let pending = stats.get("pending").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let processing = stats.get("printing").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let completed = stats.get("completed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let failed = stats.get("failed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                drop(queue);

                telemetry.record_event(telemetry::TelemetryEvent::QueueSnapshot {
                    pending,
                    processing,
                    completed,
                    failed,
                }).await;

                // Push stats to frontend via Tauri events (real-time dashboard update)
                if let Some(ref handle) = app_handle {
                    let _ = handle.emit("queue-stats-updated", &stats);
                }
            }
        }
    });
}

/// Start periodic cleanup task
async fn start_cleanup_task(queue_manager: Arc<Mutex<QueueManager>>) {
    info!("Starting periodic cleanup task (daily)");

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(24 * 60 * 60));

        loop {
            interval.tick().await;

            info!("Running daily queue cleanup");
            let queue = queue_manager.lock().await;
            if let Err(e) = queue.cleanup_old_jobs().await {
                error!("Cleanup task failed: {}", e);
            }
        }
    });
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() {
    // Initialize Sentry crash reporting FIRST (guard must outlive tracing)
    let _sentry_guard = sentry_init::init();

    // Initialize logging with file output for debugging
    // Logs go to: ~/Library/Logs/EatsomePrinterService/app.log (macOS)
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Library")
        .join("Logs")
        .join("EatsomePrinterService");

    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::never(&log_dir, "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Build tracing subscriber with file logging + Sentry integration
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("eatsome_printer_daemon=debug".parse().unwrap())
        .add_directive(tracing::Level::DEBUG.into());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(false)
        .with_writer(non_blocking);

    // Sentry layer: ERROR → Sentry Event, WARN → Breadcrumb, others → Ignore
    let sentry_layer = sentry_tracing::layer().event_filter(|md| {
        match *md.level() {
            tracing::Level::ERROR => sentry_tracing::EventFilter::Event,
            tracing::Level::WARN => sentry_tracing::EventFilter::Breadcrumb,
            _ => sentry_tracing::EventFilter::Ignore,
        }
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(sentry_layer)
        .init();

    info!("========================================");
    info!("Eatsome Printer Service Starting...");
    info!("Version: v{}", env!("CARGO_PKG_VERSION"));
    info!("Log file: {}", log_dir.join("app.log").display());
    info!("Sentry: {}", if _sentry_guard.is_some() { "enabled" } else { "disabled" });
    info!("========================================");

    // Initialize components
    // Config will be loaded from Tauri store in setup
    let config = AppConfig::default();

    if let Some(restaurant_id) = &config.restaurant_id {
        info!("Restaurant ID: {}", restaurant_id);
        sentry_init::set_restaurant_context(restaurant_id);
    } else {
        warn!("No restaurant ID configured - daemon in setup mode");
    }

    // Initialize printer manager
    let printer_manager = match PrinterManager::new() {
        Ok(pm) => pm,
        Err(e) => {
            error!("Failed to initialize PrinterManager: {}", e);
            warn!("USB support will be unavailable - continuing with network/BLE only");
            // Create a fallback without USB context - this is handled by discovery
            // falling back to network-only mode
            PrinterManager::new().unwrap_or_else(|_| {
                error!("Critical: Cannot initialize PrinterManager at all");
                std::process::exit(1);
            })
        }
    };

    // Initialize queue manager with encryption
    let encryption_key = config.restaurant_id.as_ref()
        .map(|id| QueueManager::derive_key(id, "eatsome-print-queue"));

    let queue_manager = match QueueManager::new(config.database_path(), encryption_key).await {
        Ok(qm) => qm,
        Err(e) => {
            error!("Failed to initialize queue manager: {}", e);
            error!("Cannot proceed without queue storage - exiting");
            std::process::exit(1);
        }
    };
    info!("Database initialized at: {:?}", config.database_path());

    // Initialize telemetry
    let telemetry = Arc::new(TelemetryCollector::new());

    // Initialize JWT manager
    let jwt_secret = config.restaurant_id.as_ref()
        .map(|id| format!("eatsome_printer_{}", id))
        .unwrap_or_else(|| "eatsome_printer_default".to_string());
    let jwt_manager = Arc::new(JWTManager::new(jwt_secret));

    // Initialize circuit breaker registry
    let circuit_breakers = Arc::new(CircuitBreakerRegistry::new());

    // Initialize shutdown flag
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    // Create application state
    let state = AppState {
        config: Arc::new(Mutex::new(config.clone())),
        printer_manager: Arc::new(Mutex::new(printer_manager)),
        queue_manager: Arc::new(Mutex::new(queue_manager)),
        job_poller_handle: Arc::new(Mutex::new(None)),
        telemetry: telemetry.clone(),
        jwt_manager: jwt_manager.clone(),
        circuit_breakers: circuit_breakers.clone(),
        start_time: Instant::now(),
        shutdown_requested: shutdown_requested.clone(),
    };

    // Start background tasks
    let queue_clone = state.queue_manager.clone();
    let printer_clone = state.printer_manager.clone();
    let telemetry_clone = telemetry.clone();
    let breakers_clone = circuit_breakers.clone();
    let config_clone = state.config.clone();
    let shutdown_clone = shutdown_requested.clone();

    tokio::spawn(async move {
        start_job_processor(queue_clone, printer_clone, telemetry_clone, breakers_clone, config_clone, shutdown_clone).await;
    });

    // Start cleanup task
    start_cleanup_task(state.queue_manager.clone()).await;

    // Start periodic queue metrics snapshot (app_handle set later in setup)
    start_queue_metrics(state.queue_manager.clone(), telemetry.clone(), None).await;

    // Register printers in Supabase (one-time upsert, heartbeats piggybacked on polls)
    start_printer_registration(
        state.config.clone(),
        telemetry.clone(),
    ).await;

    // Start telemetry reporter (reports every 5 minutes)
    let reporter = TelemetryReporter::new(telemetry.clone());
    reporter.start_reporting(300).await;

    // Start HTTP API server (fallback)
    if let Some(restaurant_id) = &config.restaurant_id {
        let api_state = api::ApiState {
            queue_manager: state.queue_manager.clone(),
            telemetry: telemetry.clone(),
            jwt_manager: jwt_manager.clone(),
            restaurant_id: restaurant_id.clone(),
            supabase_connected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            start_time: state.start_time,
        };

        tokio::spawn(async move {
            if let Err(e) = api::start_api_server("127.0.0.1:8043", api_state).await {
                error!("Failed to start HTTP API server: {}", e);
            }
        });

        // Note: heartbeat is piggybacked on poll-jobs calls (no separate heartbeat task)
    }

    info!("Background services initialized");

    // Start Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .setup(|app| {
            // Load config from store and apply to managed state
            let store = app.store("config.json")?;
            if let Some(stored_config) = store.get("config") {
                match serde_json::from_value::<AppConfig>(stored_config.clone()) {
                    Ok(loaded_config) => {
                        info!("Config loaded from store (restaurant: {:?}, {} printers)",
                            loaded_config.restaurant_id, loaded_config.printers.len());

                        let state = app.state::<AppState>();
                        let config_arc = state.config.clone();
                        let pm_arc = state.printer_manager.clone();
                        let loaded = loaded_config.clone();

                        // Apply stored config to the managed state (spawn, not block_on:
                        // setup runs inside the tokio runtime, so block_on would panic)
                        tauri::async_runtime::spawn(async move {
                            let mut config = config_arc.lock().await;
                            *config = loaded.clone();
                            drop(config);

                            let pm = pm_arc.lock().await;
                            for printer in &loaded.printers {
                                pm.add_printer(printer.clone()).await;
                            }
                            drop(pm);

                            info!("Stored config applied: {} printers registered", loaded.printers.len());
                        });

                        // Set Sentry context from stored config
                        if let Some(ref restaurant_id) = loaded_config.restaurant_id {
                            sentry_init::set_restaurant_context(restaurant_id);
                            sentry_init::set_user_context(restaurant_id);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse stored config: {} - using defaults", e);
                    }
                }
            } else {
                info!("No stored config found, using defaults");
            }

            setup_system_tray(app.handle())?;
            info!("System tray initialized");

            // Start update checker (notify-only, user decides when to install)
            let handle = app.handle().clone();
            let checker = Arc::new(updater::UpdateChecker::new(handle));
            tauri::async_runtime::spawn(async move {
                checker.start().await;
            });
            info!("Update checker initialized (6h check interval, notify-only)");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            claim_pairing_code,
            discover_printers,
            test_print,
            test_discovered_printer,
            start_polling,
            stop_polling,
            get_queue_stats,
            get_metrics,
            get_connection_state,
            is_printer_online,
            add_printer,
            remove_printer,
            get_uptime,
            escalate_job_priority,
            preview_test_print,
            preview_kitchen_receipt,
            cleanup_queue,
            clear_queue,
            get_circuit_breaker_status,
            reset_circuit_breaker,
            get_event_history,
            get_log_tail,
            get_log_path,
            updater::check_for_updates,
            updater::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    info!("Eatsome Printer Service shutting down...");
}
