use crate::errors::{DaemonError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info, warn};

/// Result from claiming a pairing code via the webapp API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingResult {
    pub token: String,
    pub restaurant_id: String,
    pub restaurant_code: String,
    pub expires_in: String,
}

/// Supabase client with dual-mode authentication:
/// - Setup mode (anon key): REST RPC for restaurant code resolution + validation
/// - Operations mode (auth_token → Edge Function): all daemon operations
pub struct SupabaseClient {
    client: Client,
    base_url: String,
    anon_key: String,
    auth_token: Option<String>,
}

/// Result from polling for pending jobs, with optional failover config
pub struct PollResult {
    pub jobs: Vec<serde_json::Value>,
    /// Map of primary_printer_id → [backup_printer_ids], refreshed periodically
    pub failover_config: Option<std::collections::HashMap<String, Vec<String>>>,
}

impl SupabaseClient {
    /// Create a new dual-mode Supabase client
    ///
    /// - `anon_key`: Used for Supabase gateway auth + setup RPCs
    /// - `auth_token`: Per-restaurant JWT for Edge Function operations (None during setup)
    pub fn new(supabase_url: String, anon_key: String, auth_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                error!("Failed to create HTTP client with custom config: {}. Using defaults.", e);
                Client::new()
            });

        // Remove trailing slash from URL
        let base_url = supabase_url.trim_end_matches('/').to_string();

        info!("Initialized Supabase client: {} (auth_token: {})", base_url, auth_token.is_some());

        Self {
            client,
            base_url,
            anon_key,
            auth_token,
        }
    }

    // =========================================================================
    // Setup mode (anon key, REST RPC) — pre-auth
    // =========================================================================

    /// Resolve a restaurant code (e.g., "W434N") to its UUID
    pub async fn resolve_restaurant_code(&self, code: &str) -> Result<Option<String>> {
        let url = format!("{}/rest/v1/rpc/resolve_restaurant_code", self.base_url);

        debug!("Resolving restaurant code via RPC: {}", code);

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .header("Content-Type", "application/json")
            .json(&json!({ "code": code }))
            .send()
            .await
            .map_err(|e| {
                warn!("Restaurant code lookup failed: {}", e);
                DaemonError::Network(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Restaurant code RPC failed: {} - {}", status, body);
            return Err(DaemonError::Network(format!(
                "Code lookup failed: {} - {}",
                status, body
            )));
        }

        let uuid: serde_json::Value = response
            .json()
            .await
            .map_err(|e| DaemonError::Network(format!("Parse error: {}", e)))?;

        if let Some(id) = uuid.as_str() {
            info!("Resolved restaurant code '{}' -> UUID '{}'", code, id);
            return Ok(Some(id.to_string()));
        }

        debug!("Restaurant code '{}' not found", code);
        Ok(None)
    }

    /// Validate that a restaurant ID exists in Supabase
    #[allow(dead_code)] // Public API for setup wizard validation
    pub async fn validate_restaurant_exists(&self, restaurant_id: &str) -> Result<bool> {
        let url = format!("{}/rest/v1/restaurants", self.base_url);

        debug!("Validating restaurant ID: {}", restaurant_id);

        let response = self
            .client
            .get(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .query(&[
                ("id", format!("eq.{}", restaurant_id)),
                ("select", "id".to_string()),
                ("limit", "1".to_string()),
            ])
            .send()
            .await
            .map_err(|e| {
                warn!("Restaurant validation request failed: {}", e);
                DaemonError::Network(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Restaurant validation query failed: {} - {}", status, body);
            return Err(DaemonError::Network(format!(
                "Validation query failed: {} - {}",
                status, body
            )));
        }

        let rows: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| DaemonError::Network(format!("Parse error: {}", e)))?;

        let exists = !rows.is_empty();
        debug!("Restaurant {} exists: {}", restaurant_id, exists);
        Ok(exists)
    }

    // =========================================================================
    // Pairing mode — claim a pairing code via the webapp API
    // =========================================================================

    /// Claim a pairing code by calling the restaurant webapp API.
    /// The webapp handles JWT generation + service role operations.
    /// Returns the auth token, restaurant ID, and restaurant code.
    pub async fn claim_pairing_code(
        &self,
        webapp_url: &str,
        code: &str,
        client_info: &serde_json::Value,
    ) -> Result<PairingResult> {
        let url = format!("{}/api/printer/pair", webapp_url.trim_end_matches('/'));

        info!("Claiming pairing code via webapp API: {}...", &code[..2]);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&json!({
                "code": code,
                "clientInfo": client_info,
            }))
            .send()
            .await
            .map_err(|e| {
                warn!("Pairing code claim request failed: {}", e);
                DaemonError::Network(format!("Could not reach restaurant server: {}", e))
            })?;

        let status = response.status();

        if status.as_u16() == 429 {
            return Err(DaemonError::Network(
                "Te veel pogingen. Wacht even en probeer opnieuw.".into(),
            ));
        }

        if !status.is_success() {
            let body: serde_json::Value = response
                .json()
                .await
                .unwrap_or_else(|_| json!({ "error": "Unknown error" }));
            let error_msg = body["error"].as_str().unwrap_or("Pairing failed");
            warn!("Pairing code claim failed: {} - {}", status, error_msg);
            return Err(DaemonError::Network(error_msg.to_string()));
        }

        let result: PairingResult = response
            .json()
            .await
            .map_err(|e| DaemonError::Network(format!("Failed to parse pairing response: {}", e)))?;

        info!(
            "Pairing successful! Restaurant: {} ({})",
            result.restaurant_code, result.restaurant_id
        );

        Ok(result)
    }

    // =========================================================================
    // Operations mode (auth_token → Edge Function) — post-auth
    // =========================================================================

    /// Call the printer-daemon-api Edge Function
    ///
    /// Sends: Authorization: Bearer {anon_key} (Supabase gateway)
    ///        X-Printer-Token: {auth_token} (our custom JWT)
    async fn edge_call(&self, action: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
        let token = self.auth_token.as_ref()
            .ok_or_else(|| DaemonError::Config("No auth_token configured. Generate one from POS Devices page.".into()))?;

        let url = format!("{}/functions/v1/printer-daemon-api", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .header("X-Printer-Token", token)
            .header("Content-Type", "application/json")
            .json(&json!({
                "action": action,
                "payload": payload
            }))
            .send()
            .await
            .map_err(|e| {
                warn!("Edge Function call '{}' failed: {}", action, e);
                DaemonError::Network(e.to_string())
            })?;

        let status = response.status();

        if status.as_u16() == 401 {
            let body = response.text().await.unwrap_or_default();
            warn!("Edge Function auth failed (401): {}", body);
            return Err(DaemonError::Config(
                "Auth token expired or invalid. Generate a new one from POS Devices page.".into(),
            ));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Edge Function '{}' failed: {} - {}", action, status, body);
            return Err(DaemonError::Network(format!(
                "Edge Function '{}' failed: {} - {}",
                action, status, body
            )));
        }

        response
            .json()
            .await
            .map_err(|e| DaemonError::Network(format!("Parse error: {}", e)))
    }

    /// Upsert printers to database via Edge Function
    pub async fn upsert_printers(&self, printers: Vec<PrinterUpsert>) -> Result<()> {
        debug!("Upserting {} printers via Edge Function", printers.len());

        self.edge_call("upsert-printers", json!({ "printers": printers })).await?;

        info!("Successfully upserted {} printers", printers.len());
        Ok(())
    }

    /// Update print job status via Edge Function
    pub async fn update_job_status(
        &self,
        job_id: &str,
        status: &str,
        error_message: Option<&str>,
        print_duration_ms: Option<u64>,
    ) -> Result<()> {
        debug!("Updating job {} status to '{}'", job_id, status);

        let mut payload = json!({
            "job_id": job_id,
            "status": status,
        });

        if let Some(err) = error_message {
            payload["error_message"] = json!(err);
        }
        if let Some(ms) = print_duration_ms {
            payload["print_duration_ms"] = json!(ms);
        }

        self.edge_call("update-job-status", payload).await?;

        debug!("Job {} status updated to '{}'", job_id, status);
        Ok(())
    }

    /// Insert a record into print_jobs_log via Edge Function
    pub async fn insert_job_log(
        &self,
        _restaurant_id: &str,
        order_id: Option<&str>,
        printer_id: Option<&str>,
        station_id: Option<&str>,
        status: &str,
        error_message: Option<&str>,
        print_duration_ms: Option<u64>,
        retry_count: i32,
    ) -> Result<()> {
        debug!("Inserting job log: status={}", status);

        let mut payload = json!({
            "status": status,
            "retry_count": retry_count,
        });

        if let Some(oid) = order_id {
            payload["order_id"] = json!(oid);
        }
        if let Some(pid) = printer_id {
            payload["printer_id"] = json!(pid);
        }
        if let Some(sid) = station_id {
            payload["station_id"] = json!(sid);
        }
        if let Some(err) = error_message {
            payload["error_message"] = json!(err);
        }
        if let Some(ms) = print_duration_ms {
            payload["print_duration_ms"] = json!(ms as i64);
        }

        self.edge_call("insert-job-log", payload).await?;

        debug!("Job log inserted: status={}", status);
        Ok(())
    }

    /// Update a single printer's status via Edge Function (circuit breaker → POS)
    pub async fn update_printer_status(&self, printer_id: &str, status: &str) -> Result<()> {
        debug!("Updating printer {} status to '{}'", printer_id, status);

        self.edge_call("update-printer-status", json!({
            "printer_id": printer_id,
            "status": status,
        })).await?;

        info!("Printer {} status updated to '{}'", printer_id, status);
        Ok(())
    }

    /// Update a printer's status with detailed hardware info from DLE EOT polling.
    /// Sends the status string plus optional detail fields for richer diagnostics.
    pub async fn update_printer_status_detailed(
        &self,
        printer_id: &str,
        status: &str,
        hw_status: &crate::status::PrinterHwStatus,
    ) -> Result<()> {
        debug!("Updating printer {} status to '{}' (detailed: {:?})", printer_id, status, hw_status);

        let error_details: Option<&str> = if hw_status.cutter_error {
            Some("cutter_error")
        } else if hw_status.error {
            Some("general_error")
        } else {
            None
        };

        let paper_status = if !hw_status.paper_present { "out" }
            else if hw_status.paper_near_end { "low" }
            else { "ok" };

        let cover_status = if hw_status.cover_open { "open" } else { "closed" };

        self.edge_call("update-printer-status", json!({
            "printer_id": printer_id,
            "status": status,
            "paper_status": paper_status,
            "cover_status": cover_status,
            "error_details": error_details,
        })).await?;

        info!("Printer {} status updated to '{}' (detailed)", printer_id, status);
        Ok(())
    }

    /// Poll for pending print jobs via Edge Function (without failover config).
    /// If `printer_ids` is non-empty, piggybacks a heartbeat update.
    /// Prefer `poll_pending_jobs_with_failover()` for full functionality.
    #[allow(dead_code)]
    pub async fn poll_pending_jobs(&self, printer_ids: &[String]) -> Result<Vec<serde_json::Value>> {
        let result = self.poll_pending_jobs_with_failover(printer_ids, false).await?;
        Ok(result.jobs)
    }

    /// Poll for pending jobs, optionally including failover config.
    /// When `include_failover` is true, the response includes a failover_config map
    /// of primary_printer_id → [backup_printer_ids].
    pub async fn poll_pending_jobs_with_failover(
        &self,
        printer_ids: &[String],
        include_failover: bool,
    ) -> Result<PollResult> {
        let mut payload = json!({});
        if !printer_ids.is_empty() {
            payload["printer_ids"] = json!(printer_ids);
        }
        if include_failover {
            payload["include_failover_config"] = json!(true);
        }

        let result = self.edge_call("poll-jobs", payload).await?;

        let jobs = result
            .get("jobs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Parse failover config if present
        let failover_config = result.get("failover_config").and_then(|v| {
            v.as_object().map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| {
                        v.as_array().map(|arr| {
                            let backups: Vec<String> = arr
                                .iter()
                                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                                .collect();
                            (k.clone(), backups)
                        })
                    })
                    .collect()
            })
        });

        Ok(PollResult { jobs, failover_config })
    }
}

/// Printer upsert payload
#[derive(Debug, Serialize)]
pub struct PrinterUpsert {
    pub id: String,
    pub restaurant_id: String,
    pub name: String,
    pub connection_type: String,
    pub address: String,
    pub protocol: String,
    pub capabilities: serde_json::Value,
    pub status: String,
    pub last_seen: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = SupabaseClient::new(
            "https://test.supabase.co".to_string(),
            "anon_key".to_string(),
            Some("auth_token".to_string()),
        );

        assert_eq!(client.base_url, "https://test.supabase.co");
        assert_eq!(client.anon_key, "anon_key");
        assert_eq!(client.auth_token, Some("auth_token".to_string()));
    }

    #[test]
    fn test_url_trailing_slash_removed() {
        let client = SupabaseClient::new(
            "https://test.supabase.co/".to_string(),
            "anon_key".to_string(),
            None,
        );

        assert_eq!(client.base_url, "https://test.supabase.co");
    }

    #[test]
    fn test_client_without_auth_token() {
        let client = SupabaseClient::new(
            "https://test.supabase.co".to_string(),
            "anon_key".to_string(),
            None,
        );

        assert!(client.auth_token.is_none());
    }
}
