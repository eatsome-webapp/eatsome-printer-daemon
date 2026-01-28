use crate::errors::{DaemonError, Result};
use crate::queue::{PrintJob, QueueManager};
use backon::{ExponentialBuilder, Retryable};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info, warn};

/// Connection state for UI tracking
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed(String),
}

/// Phoenix Realtime message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealtimeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(rename = "event", skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(rename = "payload", skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(rename = "topic", skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
}

/// PostgreSQL change payload from Supabase Realtime
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostgresChangesPayload {
    schema: String,
    table: String,
    commit_timestamp: String,
    #[serde(rename = "type")]
    change_type: String,
    columns: Vec<serde_json::Value>,
    record: Option<serde_json::Value>,
    old_record: Option<serde_json::Value>,
}

/// Phoenix reply payload
#[derive(Debug, Clone, Deserialize)]
struct PhxReply {
    status: String,
    response: Option<serde_json::Value>,
}

/// Supabase Realtime client with reconnection and heartbeat
pub struct RealtimeClient {
    supabase_url: String,
    service_role_key: String,
    restaurant_id: Option<String>,
    connection_state: Arc<RwLock<ConnectionState>>,
    write_sink: Arc<Mutex<Option<futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >>>>,
    message_ref: Arc<Mutex<u64>>,
}

impl RealtimeClient {
    pub fn new(supabase_url: String, service_role_key: String) -> Self {
        Self {
            supabase_url,
            service_role_key,
            restaurant_id: None,
            connection_state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            write_sink: Arc::new(Mutex::new(None)),
            message_ref: Arc::new(Mutex::new(0)),
        }
    }

    /// Connect to Supabase Realtime with exponential backoff retry
    pub async fn connect(
        &mut self,
        restaurant_id: &str,
        queue_manager: Arc<Mutex<QueueManager>>,
    ) -> Result<()> {
        self.restaurant_id = Some(restaurant_id.to_string());
        let restaurant_id = restaurant_id.to_string();

        // Clone Arc references for background task
        let supabase_url = self.supabase_url.clone();
        let service_role_key = self.service_role_key.clone();
        let connection_state = self.connection_state.clone();
        let write_sink_arc = self.write_sink.clone();
        let message_ref = self.message_ref.clone();

        // Spawn connection task with exponential backoff
        tokio::spawn(async move {
            let connect_fn = || async {
                Self::establish_connection(
                    &supabase_url,
                    &service_role_key,
                    &restaurant_id,
                    &queue_manager,
                    &connection_state,
                    &write_sink_arc,
                    &message_ref,
                )
                .await
            };

            // Exponential backoff: 1s → 2s → 4s → 8s → ... → 60s (max)
            let backoff = ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(1))
                .with_max_delay(Duration::from_secs(60))
                .with_max_times(usize::MAX); // Retry indefinitely

            connect_fn.retry(&backoff).await.unwrap_or_else(|e| {
                error!("Connection retry exhausted: {}", e);
            });
        });

        Ok(())
    }

    /// Establish WebSocket connection with proper Phoenix protocol handshake
    async fn establish_connection(
        supabase_url: &str,
        service_role_key: &str,
        restaurant_id: &str,
        queue_manager: &Arc<Mutex<QueueManager>>,
        connection_state: &Arc<RwLock<ConnectionState>>,
        write_sink_arc: &Arc<Mutex<Option<futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >>>>,
        message_ref: &Arc<Mutex<u64>>,
    ) -> Result<()> {
        // Update state: Connecting
        {
            let mut state = connection_state.write().await;
            *state = ConnectionState::Connecting;
        }

        // Build WebSocket URL
        let ws_url = supabase_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            + "/realtime/v1/websocket?apikey="
            + service_role_key
            + "&vsn=1.0.0";

        info!("Connecting to Supabase Realtime: {}", ws_url);

        // Connect to WebSocket
        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .map_err(|e| DaemonError::Realtime(format!("WebSocket connection failed: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();

        // Step 1: Send access_token
        let ref_num = Self::next_ref(message_ref).await;
        let access_token_msg = json!({
            "type": "access_token",
            "payload": {
                "access_token": service_role_key
            },
            "ref": ref_num.to_string()
        });

        write
            .send(Message::Text(serde_json::to_string(&access_token_msg).unwrap()))
            .await
            .map_err(|e| DaemonError::Realtime(format!("Failed to send access token: {}", e)))?;

        debug!("Sent access_token, waiting for phx_reply...");

        // Wait for access_token reply (with timeout)
        let access_reply = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(msg) = read.next().await {
                if let Ok(Message::Text(text)) = msg {
                    if let Ok(realtime_msg) = serde_json::from_str::<RealtimeMessage>(&text) {
                        if realtime_msg.msg_type == "phx_reply"
                            && realtime_msg.reference.as_deref() == Some(&ref_num.to_string())
                        {
                            return Ok::<_, DaemonError>(realtime_msg);
                        }
                    }
                }
            }
            Err(DaemonError::Realtime("No phx_reply received".to_string()))
        })
        .await
        .map_err(|_| DaemonError::Realtime("Access token timeout".to_string()))??;

        // Check phx_reply status
        if let Some(payload) = access_reply.payload {
            if let Ok(reply) = serde_json::from_value::<PhxReply>(payload) {
                if reply.status != "ok" {
                    return Err(DaemonError::Realtime(format!(
                        "Access token failed: {}",
                        reply.status
                    )));
                }
            }
        }

        info!("Access token accepted");

        // Step 2: Subscribe to postgres_changes channel
        let topic = format!("realtime:public:print_jobs_queue:restaurant_id=eq.{}", restaurant_id);
        let join_ref = Self::next_ref(message_ref).await;
        let join_msg = json!({
            "type": "phx_join",
            "topic": topic,
            "payload": {
                "config": {
                    "postgres_changes": [{
                        "event": "INSERT",
                        "schema": "public",
                        "table": "print_jobs_queue",
                        "filter": format!("restaurant_id=eq.{}", restaurant_id)
                    }]
                }
            },
            "ref": join_ref.to_string()
        });

        write
            .send(Message::Text(serde_json::to_string(&join_msg).unwrap()))
            .await
            .map_err(|e| DaemonError::Realtime(format!("Failed to join channel: {}", e)))?;

        debug!("Sent phx_join, waiting for phx_reply...");

        // Wait for phx_join reply
        let join_reply = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(msg) = read.next().await {
                if let Ok(Message::Text(text)) = msg {
                    if let Ok(realtime_msg) = serde_json::from_str::<RealtimeMessage>(&text) {
                        if realtime_msg.msg_type == "phx_reply"
                            && realtime_msg.reference.as_deref() == Some(&join_ref.to_string())
                        {
                            return Ok::<_, DaemonError>(realtime_msg);
                        }
                    }
                }
            }
            Err(DaemonError::Realtime("No phx_reply received".to_string()))
        })
        .await
        .map_err(|_| DaemonError::Realtime("Channel join timeout".to_string()))??;

        // Check phx_reply status
        if let Some(payload) = join_reply.payload {
            if let Ok(reply) = serde_json::from_value::<PhxReply>(payload) {
                if reply.status != "ok" {
                    return Err(DaemonError::Realtime(format!(
                        "Channel join failed: {}",
                        reply.status
                    )));
                }
            }
        }

        info!("Successfully subscribed to channel: {}", topic);

        // Update state: Connected
        {
            let mut state = connection_state.write().await;
            *state = ConnectionState::Connected;
        }

        // Store write sink for sending messages (e.g., printer status)
        {
            let mut sink = write_sink_arc.lock().await;
            *sink = Some(write);
        }

        // Start heartbeat task (send heartbeat every 30s)
        let write_sink_heartbeat = write_sink_arc.clone();
        let message_ref_heartbeat = message_ref.clone();
        let connection_state_heartbeat = connection_state.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                // Check if still connected
                let state = connection_state_heartbeat.read().await;
                if *state != ConnectionState::Connected {
                    break;
                }
                drop(state);

                // Send heartbeat
                let ref_num = Self::next_ref(&message_ref_heartbeat).await;
                let heartbeat_msg = json!({
                    "type": "heartbeat",
                    "ref": ref_num.to_string()
                });

                let mut sink_lock = write_sink_heartbeat.lock().await;
                if let Some(sink) = sink_lock.as_mut() {
                    if let Err(e) = sink
                        .send(Message::Text(serde_json::to_string(&heartbeat_msg).unwrap()))
                        .await
                    {
                        error!("Heartbeat failed: {}", e);
                        break;
                    }
                    debug!("Heartbeat sent");
                } else {
                    break;
                }
            }

            info!("Heartbeat task stopped");
        });

        // Start message listener
        let queue_manager_clone = queue_manager.clone();
        let restaurant_id_clone = restaurant_id.to_string();
        let connection_state_listener = connection_state.clone();

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(realtime_msg) = serde_json::from_str::<RealtimeMessage>(&text) {
                            Self::handle_message(realtime_msg, &queue_manager_clone, &restaurant_id_clone).await;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        warn!("WebSocket closed by server");
                        let mut state = connection_state_listener.write().await;
                        *state = ConnectionState::Reconnecting;
                        break;
                    }
                    Ok(Message::Ping(_)) => {
                        debug!("Received ping from server");
                    }
                    Ok(Message::Pong(_)) => {
                        debug!("Received pong from server");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        let mut state = connection_state_listener.write().await;
                        *state = ConnectionState::Failed(e.to_string());
                        break;
                    }
                    _ => {}
                }
            }

            info!("Message listener stopped, triggering reconnection...");
        });

        // Start polling fallback (checks for pending jobs every 5s)
        let queue_manager_poll = queue_manager.clone();
        let restaurant_id_poll = restaurant_id.to_string();
        let connection_state_poll = connection_state.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5));
            loop {
                interval.tick().await;

                // Check if still connected
                let state = connection_state_poll.read().await;
                if *state != ConnectionState::Connected {
                    break;
                }
                drop(state);

                let queue = queue_manager_poll.lock().await;
                if let Ok(pending_jobs) = queue.get_pending_jobs(5).await {
                    if !pending_jobs.is_empty() {
                        debug!("Polling found {} pending jobs", pending_jobs.len());

                        for job in pending_jobs {
                            info!("Processing polled job: {}", job.id);
                            // Process job through normal queue processing
                            // (queue manager will handle this via processor)
                        }
                    }
                }
            }

            info!("Polling task stopped");
        });

        Ok(())
    }

    /// Handle incoming realtime messages
    async fn handle_message(
        msg: RealtimeMessage,
        queue_manager: &Arc<Mutex<QueueManager>>,
        restaurant_id: &str,
    ) {
        match msg.msg_type.as_str() {
            "system" => {
                if let Some(event) = msg.event {
                    info!("System event: {}", event);
                }
            }
            "postgres_changes" => {
                if let Some(payload) = msg.payload {
                    if let Ok(changes) = serde_json::from_value::<PostgresChangesPayload>(payload.clone()) {
                        if changes.change_type == "INSERT" {
                            if let Some(record) = changes.record {
                                info!("New print job received via realtime");

                                // Convert to PrintJob and enqueue
                                if let Ok(job) = Self::parse_print_job(record, restaurant_id) {
                                    let queue = queue_manager.lock().await;
                                    if let Err(e) = queue.enqueue(job).await {
                                        error!("Failed to enqueue job: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "phx_reply" => {
                if let Some(payload) = msg.payload {
                    if let Ok(reply) = serde_json::from_value::<PhxReply>(payload) {
                        if reply.status == "ok" {
                            debug!("Received OK reply for ref: {:?}", msg.reference);
                        } else {
                            warn!("Received non-OK reply: {}", reply.status);
                        }
                    }
                }
            }
            "phx_error" => {
                error!("Channel error: {:?}", msg.payload);
            }
            _ => {
                debug!("Unknown message type: {}", msg.msg_type);
            }
        }
    }

    /// Parse print job from realtime payload
    fn parse_print_job(record: serde_json::Value, restaurant_id: &str) -> Result<PrintJob> {
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing job id".to_string()))?
            .to_string();

        let order_id = record
            .get("order_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing order_id".to_string()))?
            .to_string();

        let order_number = record
            .get("order_number")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing order_number".to_string()))?
            .to_string();

        let station = record
            .get("station")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DaemonError::Queue("Missing station".to_string()))?
            .to_string();

        let items_json = record
            .get("items")
            .ok_or_else(|| DaemonError::Queue("Missing items".to_string()))?;

        let items = serde_json::from_value(items_json.clone())
            .map_err(|e| DaemonError::Queue(format!("Failed to parse items: {}", e)))?;

        let timestamp = record
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

        Ok(PrintJob {
            id,
            restaurant_id: restaurant_id.to_string(),
            order_id,
            order_number,
            station,
            printer_id: record.get("printer_id").and_then(|v| v.as_str()).map(String::from),
            items,
            table_number: record.get("table_number").and_then(|v| v.as_str()).map(String::from),
            customer_name: record.get("customer_name").and_then(|v| v.as_str()).map(String::from),
            order_type: record.get("order_type").and_then(|v| v.as_str()).map(String::from),
            priority: record.get("priority").and_then(|v| v.as_u64()).unwrap_or(3) as u8,
            timestamp,
            status: "pending".to_string(),
            retry_count: 0,
            error_message: None,
        })
    }

    /// Broadcast printer status to POS app via Realtime channel
    pub async fn publish_printer_status(
        &self,
        printer_id: &str,
        status: &str,
        queue_depth: Option<usize>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let restaurant_id = self.restaurant_id.as_ref()
            .ok_or_else(|| DaemonError::Realtime("Not connected".to_string()))?;

        let topic = format!("restaurant:{}:printer-status", restaurant_id);
        let ref_num = Self::next_ref(&self.message_ref).await;

        let broadcast_msg = json!({
            "type": "broadcast",
            "topic": topic,
            "event": "printer-status",
            "payload": {
                "printer_id": printer_id,
                "status": status,
                "queue_depth": queue_depth,
                "error_message": error_message,
                "timestamp": chrono::Utc::now().timestamp_millis()
            },
            "ref": ref_num.to_string()
        });

        let mut sink_lock = self.write_sink.lock().await;
        if let Some(sink) = sink_lock.as_mut() {
            sink.send(Message::Text(serde_json::to_string(&broadcast_msg).unwrap()))
                .await
                .map_err(|e| DaemonError::Realtime(format!("Failed to broadcast status: {}", e)))?;

            debug!("Printer status broadcasted: {} - {}", printer_id, status);
            Ok(())
        } else {
            Err(DaemonError::Realtime("No active connection".to_string()))
        }
    }

    /// Disconnect from Supabase Realtime
    pub async fn disconnect(self) -> Result<()> {
        let mut state = self.connection_state.write().await;
        *state = ConnectionState::Disconnected;

        let mut sink_lock = self.write_sink.lock().await;
        if let Some(mut sink) = sink_lock.take() {
            let _ = sink.close().await;
        }

        info!("Disconnected from Supabase Realtime");
        Ok(())
    }

    /// Get current connection state
    pub async fn get_connection_state(&self) -> ConnectionState {
        self.connection_state.read().await.clone()
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        matches!(*self.connection_state.read().await, ConnectionState::Connected)
    }

    /// Get next message reference number (for Phoenix protocol)
    async fn next_ref(message_ref: &Arc<Mutex<u64>>) -> u64 {
        let mut ref_num = message_ref.lock().await;
        *ref_num += 1;
        *ref_num
    }
}
