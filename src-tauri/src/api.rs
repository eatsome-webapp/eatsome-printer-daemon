use crate::auth::{JWTManager, PrinterClaims};
use crate::errors::{DaemonError, Result};
use crate::status;
use crate::queue::{PrintJob, QueueManager};
use crate::telemetry::TelemetryCollector;
use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info};

/// HTTP API server state
#[derive(Clone)]
pub struct ApiState {
    pub queue_manager: Arc<Mutex<QueueManager>>,
    pub telemetry: Arc<TelemetryCollector>,
    pub jwt_manager: Arc<JWTManager>,
    pub restaurant_id: String,
    /// Shared connection state: tracks Supabase Realtime connectivity
    pub supabase_connected: Arc<std::sync::atomic::AtomicBool>,
    /// Daemon start time for uptime calculation
    pub start_time: std::time::Instant,
}

/// Print request payload
#[derive(Debug, Deserialize, Serialize)]
pub struct PrintRequest {
    pub restaurant_id: String,
    pub station: String,
    pub order_id: Option<String>,
    pub order_number: String,
    pub items: Vec<PrintItemRequest>,
    pub table_number: Option<String>,
    pub customer_name: Option<String>,
    pub order_type: Option<String>,
    pub priority: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PrintItemRequest {
    pub quantity: u32,
    pub name: String,
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// Print response
#[derive(Debug, Serialize)]
pub struct PrintResponse {
    pub job_id: String,
    pub status: String,
    pub message: String,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub restaurant_id: String,
    /// Whether Supabase Realtime is currently connected
    pub supabase_connected: bool,
    /// Operational mode: "online" (Supabase connected) or "offline" (local-only)
    pub mode: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

impl IntoResponse for DaemonError {
    fn into_response(self) -> Response {
        let error_string = self.to_string();
        let (status, message) = match self {
            DaemonError::PrinterNotFound(msg) => (StatusCode::NOT_FOUND, msg),
            DaemonError::Queue(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            DaemonError::Config(msg) => (StatusCode::BAD_REQUEST, msg),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
        };

        let body = Json(ErrorResponse {
            error: message.clone(),
            details: Some(error_string),
        });

        (status, body).into_response()
    }
}

/// Extract and validate JWT from Authorization header
async fn extract_claims(headers: &HeaderMap, jwt_manager: &JWTManager) -> Result<PrinterClaims> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| DaemonError::Other(anyhow::anyhow!("Missing Authorization header")))?;

    let token = JWTManager::extract_bearer_token(auth_header)?;
    let claims = jwt_manager.validate_with_permission(&token, "print")?;

    Ok(claims)
}

/// POST /api/print - Submit print job
async fn handle_print(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<PrintRequest>,
) -> Result<Json<PrintResponse>> {
    debug!("Print request received for order: {}", request.order_number);

    // Validate JWT and permissions
    let claims = extract_claims(&headers, &state.jwt_manager).await?;

    // Validate restaurant ID matches token
    if claims.restaurant_id != request.restaurant_id {
        error!(
            "Restaurant ID mismatch: token={}, request={}",
            claims.restaurant_id, request.restaurant_id
        );
        return Err(DaemonError::Other(anyhow::anyhow!(
            "Restaurant ID mismatch"
        )));
    }

    // Validate restaurant ID matches daemon configuration
    if request.restaurant_id != state.restaurant_id {
        error!(
            "Restaurant ID mismatch: daemon={}, request={}",
            state.restaurant_id, request.restaurant_id
        );
        return Err(DaemonError::Config(format!(
            "Restaurant ID mismatch: this daemon is configured for {}",
            state.restaurant_id
        )));
    }

    // Convert to PrintJob
    let job_id = uuid::Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().timestamp_millis();

    let items = request
        .items
        .into_iter()
        .map(|item| crate::escpos::PrintItem {
            quantity: item.quantity,
            name: item.name,
            modifiers: item.modifiers,
            notes: item.notes,
        })
        .collect();

    let print_job = PrintJob {
        id: job_id.clone(),
        restaurant_id: request.restaurant_id,
        order_id: request.order_id,
        order_number: request.order_number.clone(),
        station: request.station,
        printer_id: None,
        items,
        table_number: request.table_number,
        customer_name: request.customer_name,
        order_type: request.order_type,
        priority: request.priority.unwrap_or(3),
        timestamp,
        status: status::PENDING.to_string(),
        retry_count: 0,
        error_message: None,
    };

    // Enqueue job
    let queue = state.queue_manager.lock().await;
    queue.enqueue(print_job).await?;

    info!(
        "Print job enqueued via HTTP API: {} (order: {})",
        job_id, request.order_number
    );

    Ok(Json(PrintResponse {
        job_id,
        status: "queued".to_string(),
        message: format!("Print job queued for order {}", request.order_number),
    }))
}

/// GET /api/health - Health check endpoint
///
/// Reports daemon health, uptime, and Supabase connectivity.
/// POS apps use this to determine if the daemon is reachable and
/// whether to route print jobs via Supabase Realtime or local HTTP API.
async fn handle_health(State(state): State<ApiState>) -> Json<HealthResponse> {
    let uptime_secs = state.start_time.elapsed().as_secs();
    let supabase_connected = state.supabase_connected.load(std::sync::atomic::Ordering::Relaxed);
    let mode = if supabase_connected { "online" } else { "offline" };

    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs,
        restaurant_id: state.restaurant_id.clone(),
        supabase_connected,
        mode: mode.to_string(),
    })
}

/// GET /api/queue/stats - Queue statistics
async fn handle_queue_stats(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    // Validate JWT (requires 'status' permission)
    let _claims = extract_claims(&headers, &state.jwt_manager).await?;

    let queue = state.queue_manager.lock().await;
    let stats = queue.get_stats().await?;

    Ok(Json(stats))
}

/// GET /api/metrics - Telemetry metrics (Prometheus format)
async fn handle_metrics(State(state): State<ApiState>) -> String {
    state.telemetry.export_prometheus().await
}

/// GET /api/metrics/json - Telemetry metrics (JSON format)
async fn handle_metrics_json(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    // Validate JWT (requires 'status' permission)
    let _claims = extract_claims(&headers, &state.jwt_manager).await?;

    let metrics = state.telemetry.get_metrics_json().await;
    Ok(Json(metrics))
}

/// Create HTTP API router
pub fn create_router(state: ApiState) -> Router {
    Router::new()
        .route("/api/print", post(handle_print))
        .route("/api/health", get(handle_health))
        .route("/api/queue/stats", get(handle_queue_stats))
        .route("/api/metrics", get(handle_metrics))
        .route("/api/metrics/json", get(handle_metrics_json))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(AllowOrigin::predicate(|origin, _| {
                            let o = origin.as_bytes();
                            // Allow localhost origins (Tauri webview + local dev)
                            o.starts_with(b"http://localhost")
                                || o.starts_with(b"https://localhost")
                                || o.starts_with(b"http://127.0.0.1")
                                || o.starts_with(b"http://tauri.localhost")
                                || o.starts_with(b"https://tauri.localhost")
                                // Allow production restaurant webapp
                                || o == b"https://eatsome-restaurant.vercel.app"
                        }))
                        .allow_methods([
                            axum::http::Method::GET,
                            axum::http::Method::POST,
                            axum::http::Method::OPTIONS,
                        ])
                        .allow_headers(tower_http::cors::Any),
                ),
        )
        .with_state(state)
}

/// Start HTTP API server
pub async fn start_api_server(
    addr: &str,
    state: ApiState,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let router = create_router(state.clone());

    info!("Starting HTTP API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, router)
        .await
        .map_err(|e| {
            error!("HTTP API server error: {}", e);
            e.into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::PrinterClaims;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::path::PathBuf;
    use tower::ServiceExt;

    async fn create_test_state() -> ApiState {
        let queue_manager = Arc::new(Mutex::new(
            QueueManager::new(PathBuf::from(":memory:"), None).await.unwrap(),
        ));
        let telemetry = Arc::new(TelemetryCollector::new());
        let jwt_manager = Arc::new(JWTManager::new("test_secret_key_1234567890".to_string()));

        ApiState {
            queue_manager,
            telemetry,
            jwt_manager,
            restaurant_id: "rest_123".to_string(),
            supabase_connected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            start_time: std::time::Instant::now(),
        }
    }

    async fn create_test_token(state: &ApiState) -> String {
        let claims = PrinterClaims::new(
            "rest_123".to_string(),
            None,
            vec!["print".to_string(), "status".to_string()],
        );
        state.jwt_manager.generate_token(&claims).unwrap()
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = create_test_state().await;
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_print_endpoint_requires_auth() {
        let state = create_test_state().await;
        let app = create_router(state);

        let print_request = PrintRequest {
            restaurant_id: "rest_123".to_string(),
            station: "bar".to_string(),
            order_id: Some("order_1".to_string()),
            order_number: "R001-0001".to_string(),
            items: vec![],
            table_number: None,
            customer_name: None,
            order_type: None,
            priority: None,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/print")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&print_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should fail without Authorization header
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_print_endpoint_with_valid_token() {
        let state = create_test_state().await;
        let token = create_test_token(&state).await;
        let app = create_router(state);

        let print_request = PrintRequest {
            restaurant_id: "rest_123".to_string(),
            station: "bar".to_string(),
            order_id: Some("order_1".to_string()),
            order_number: "R001-0001".to_string(),
            items: vec![PrintItemRequest {
                quantity: 2,
                name: "Beer".to_string(),
                modifiers: vec![],
                notes: None,
            }],
            table_number: Some("5".to_string()),
            customer_name: None,
            order_type: Some("dine-in".to_string()),
            priority: Some(3),
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/print")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::from(serde_json::to_string(&print_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
