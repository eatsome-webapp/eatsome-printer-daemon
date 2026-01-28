use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,
    pub restaurant_id: Option<String>,
    pub location_id: Option<String>,
    pub auth_token: Option<String>,
    pub supabase_url: String,
    pub supabase_anon_key: String,
    pub service_role_key: String,
    pub printers: Vec<PrinterConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterConfig {
    pub id: String,
    pub name: String,
    pub connection_type: ConnectionType,
    pub address: String,
    pub protocol: String,
    pub station: Option<String>,
    pub is_primary: bool,
    pub capabilities: PrinterCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    USB,
    Network,
    Bluetooth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterCapabilities {
    pub cutter: bool,
    pub drawer: bool,
    pub qrcode: bool,
    pub max_width: u16,
}

impl AppConfig {
    pub fn database_path(&self) -> PathBuf {
        let config_dir = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .map(|p| p.join("Library/Application Support/com.eatsome.printer-service"))
                .unwrap_or_else(|| PathBuf::from("."))
        } else if cfg!(target_os = "windows") {
            dirs::config_dir()
                .map(|p| p.join("Eatsome Printer Service"))
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            dirs::config_dir()
                .map(|p| p.join("eatsome-printer-service"))
                .unwrap_or_else(|| PathBuf::from("."))
        };

        config_dir.join("print-queue.db")
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            restaurant_id: None,
            location_id: None,
            auth_token: None,
            supabase_url: "https://gtlpzikuozrdgomsvqmo.supabase.co".to_string(),
            supabase_anon_key: String::new(),
            service_role_key: String::new(),
            printers: Vec::new(),
        }
    }
}
