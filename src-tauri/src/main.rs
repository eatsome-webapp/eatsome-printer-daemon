// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Manager, State};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri_plugin_store::StoreExt;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, error, warn, debug};

mod config;
mod escpos;
mod printer;
mod queue;
mod realtime;
mod discovery;
mod errors;
mod auth;
mod circuit_breaker;
mod routing;
mod telemetry;
mod api;
mod updater;
mod sentry_init;

use config::AppConfig;
use printer::PrinterManager;
use queue::QueueManager;
use realtime::RealtimeClient;
use auth::JWTManager;
use routing::KitchenRouter;
use telemetry::{TelemetryCollector, TelemetryReporter};
use errors::DaemonError;

/// Global application state
pub struct AppState {
    config: Arc<Mutex<AppConfig>>,
    printer_manager: Arc<Mutex<PrinterManager>>,
    queue_manager: Arc<Mutex<QueueManager>>,
    realtime_client: Arc<Mutex<Option<RealtimeClient>>>,
    kitchen_router: Arc<routing::KitchenRouter>,
    telemetry: Arc<TelemetryCollector>,
    jwt_manager: Arc<JWTManager>,
    start_time: Instant,
}

// ============================================================================
// Tauri IPC Commands
// ============================================================================

/// Get current configuration
#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.lock().await;
    Ok(config.clone())
}

/// Save configuration
#[tauri::command]
async fn save_config(
    config: AppConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut app_config = state.config.lock().await;
    *app_config = config.clone();

    // Save to Tauri store
    let store = app.store("config.json").map_err(|e| e.to_string())?;
    store.set("config", serde_json::to_value(&config).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;

    info!("Configuration saved");
    Ok(())
}

/// Discover all printers (USB + Network + Bluetooth)
#[tauri::command]
async fn discover_printers(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    info!("Printer discovery requested");
    let manager = state.printer_manager.lock().await;
    manager.discover_all()
        .await
        .map_err(|e| e.to_string())
}

/// Test print on a specific printer
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

/// Connect to Supabase Realtime
#[tauri::command]
async fn connect_realtime(
    restaurant_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Realtime connection requested for restaurant: {}", restaurant_id);

    let config = state.config.lock().await;
    let queue = state.queue_manager.clone();

    let mut client = RealtimeClient::new(
        config.supabase_url.clone(),
        config.service_role_key.clone(),
    );

    client.connect(&restaurant_id, queue)
        .await
        .map_err(|e| e.to_string())?;

    let mut realtime = state.realtime_client.lock().await;
    *realtime = Some(client);

    info!("Realtime connection established");
    Ok(())
}

/// Disconnect from Supabase Realtime
#[tauri::command]
async fn disconnect_realtime(state: State<'_, AppState>) -> Result<(), String> {
    info!("Realtime disconnection requested");

    let mut realtime = state.realtime_client.lock().await;
    if let Some(client) = realtime.take() {
        client.disconnect().await.map_err(|e| e.to_string())?;
    }

    info!("Realtime connection closed");
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

/// Get realtime connection state
#[tauri::command]
async fn get_connection_state(state: State<'_, AppState>) -> Result<String, String> {
    let realtime = state.realtime_client.lock().await;

    if let Some(client) = realtime.as_ref() {
        let state = client.get_connection_state().await;
        Ok(serde_json::to_string(&state).unwrap_or_else(|_| "\"unknown\"".to_string()))
    } else {
        Ok("\"disconnected\"".to_string())
    }
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

/// Get routing groups
#[tauri::command]
async fn get_routing_groups(
    state: State<'_, AppState>,
) -> Result<Vec<routing::RoutingGroup>, String> {
    Ok(state.kitchen_router.get_routing_groups().await)
}

/// Add routing group
#[tauri::command]
async fn add_routing_group(
    group: routing::RoutingGroup,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Adding routing group: {} ({})", group.name, group.display_name);
    state.kitchen_router.add_routing_group(group).await;
    Ok(())
}

/// Get daemon uptime in seconds
#[tauri::command]
async fn get_uptime(state: State<'_, AppState>) -> Result<u64, String> {
    Ok(state.start_time.elapsed().as_secs())
}

/// Manually trigger queue cleanup (remove old completed/failed jobs)
#[tauri::command]
async fn cleanup_queue(state: State<'_, AppState>) -> Result<(), String> {
    info!("Manual queue cleanup requested");
    let queue = state.queue_manager.lock().await;
    queue.cleanup_old_jobs().await.map_err(|e| e.to_string())
}

/// Get event history from telemetry
#[tauri::command]
async fn get_event_history(
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<(u64, telemetry::TelemetryEvent)>, String> {
    Ok(state.telemetry.get_event_history(limit).await)
}

// ============================================================================
// System Tray
// ============================================================================

fn setup_system_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Create menu items
    let status = MenuItem::with_id(app, "status", "Status: Idle", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show Setup", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, Some("cmd+q"))?;

    // Build menu
    let menu = Menu::with_items(app, &[&status, &show, &quit])?;

    // Create tray icon
    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "quit" => {
                    info!("Quit requested from tray menu");
                    std::process::exit(0);
                }
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

// ============================================================================
// Background Tasks
// ============================================================================

/// Start background job processor
async fn start_job_processor(
    queue_manager: Arc<Mutex<QueueManager>>,
    printer_manager: Arc<Mutex<PrinterManager>>,
    telemetry: Arc<TelemetryCollector>,
) {
    info!("Starting background job processor");

    // Start the queue processor with print function
    let printer_clone = printer_manager.clone();
    let telemetry_clone = telemetry.clone();

    // Process jobs in a loop (stub implementation)
    // TODO: Implement proper job processing with queue polling
    tokio::spawn(async move {
        info!("Job processor started");
        // This would poll the queue and process jobs
        // For now, this is a placeholder that allows compilation
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
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
    // Initialize logging with structured output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
        )
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .init();

    info!("========================================");
    info!("Eatsome Printer Service Starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    info!("========================================");

    // Initialize Sentry crash reporting (must be kept alive)
    let _sentry_guard = sentry_init::init();

    // Initialize components
    // Config will be loaded from Tauri store in setup
    let config = AppConfig::default();

    if let Some(restaurant_id) = &config.restaurant_id {
        info!("Restaurant ID: {}", restaurant_id);
    } else {
        warn!("No restaurant ID configured - daemon in setup mode");
    }

    // Initialize printer manager
    let printer_manager = PrinterManager::new();

    // Initialize queue manager with encryption
    let encryption_key = config.restaurant_id.as_ref()
        .map(|id| QueueManager::derive_key(id, "eatsome-print-queue"));

    let queue_manager = QueueManager::new(config.database_path(), encryption_key).await
        .expect("Failed to initialize queue manager");
    info!("Database initialized at: {:?}", config.database_path());

    // Initialize telemetry
    let telemetry = Arc::new(TelemetryCollector::new());

    // Initialize JWT manager
    let jwt_secret = config.restaurant_id.as_ref()
        .map(|id| format!("eatsome_printer_{}", id))
        .unwrap_or_else(|| "eatsome_printer_default".to_string());
    let jwt_manager = Arc::new(JWTManager::new(jwt_secret));

    // Initialize kitchen router
    let kitchen_router = Arc::new(KitchenRouter::new());

    // Update kitchen router with configured printers
    kitchen_router.update_printers(config.printers.clone()).await;

    // Create application state
    let state = AppState {
        config: Arc::new(Mutex::new(config.clone())),
        printer_manager: Arc::new(Mutex::new(printer_manager)),
        queue_manager: Arc::new(Mutex::new(queue_manager)),
        realtime_client: Arc::new(Mutex::new(None)),
        kitchen_router,
        telemetry: telemetry.clone(),
        jwt_manager: jwt_manager.clone(),
        start_time: Instant::now(),
    };

    // Start background tasks
    let queue_clone = state.queue_manager.clone();
    let printer_clone = state.printer_manager.clone();
    let telemetry_clone = telemetry.clone();

    tokio::spawn(async move {
        start_job_processor(queue_clone, printer_clone, telemetry_clone).await;
    });

    // Start cleanup task
    start_cleanup_task(state.queue_manager.clone()).await;

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
        };

        tokio::spawn(async move {
            if let Err(e) = api::start_api_server("127.0.0.1:8043", api_state).await {
                error!("Failed to start HTTP API server: {}", e);
            }
        });
    }

    info!("Background services initialized");

    // Start Tauri application
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .setup(|app| {
            // Load config from store
            let store = app.store("config.json")?;
            if let Some(stored_config) = store.get("config") {
                if let Ok(_config) = serde_json::from_value::<AppConfig>(stored_config.clone()) {
                    info!("Config loaded from store");
                    // TODO: Update managed state with stored config
                    // This requires async context which isn't available in setup
                }
            } else {
                info!("No stored config found, using defaults");
            }

            setup_system_tray(app.handle())?;
            info!("System tray initialized");

            // TODO: Start auto-updater background task
            // This requires moving app.handle() into async context
            info!("Auto-updater initialization deferred to runtime");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            discover_printers,
            test_print,
            connect_realtime,
            disconnect_realtime,
            get_queue_stats,
            get_metrics,
            get_connection_state,
            is_printer_online,
            add_printer,
            remove_printer,
            get_routing_groups,
            add_routing_group,
            get_uptime,
            cleanup_queue,
            get_event_history,
            updater::check_for_updates,
            updater::get_update_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    info!("Eatsome Printer Service shutting down...");
}
