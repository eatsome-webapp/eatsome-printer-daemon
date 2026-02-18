// Common test utilities and fixtures

use std::sync::Arc;
use tokio::sync::RwLock;
use tempfile::TempDir;

/// Mock printer for testing
#[derive(Clone)]
#[allow(dead_code)]
pub struct MockPrinter {
    pub id: String,
    pub name: String,
    pub is_online: Arc<RwLock<bool>>,
    pub print_count: Arc<RwLock<u32>>,
    pub last_command: Arc<RwLock<Option<Vec<u8>>>>,
    pub should_fail: Arc<RwLock<bool>>,
}

#[allow(dead_code)]
impl MockPrinter {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            is_online: Arc::new(RwLock::new(true)),
            print_count: Arc::new(RwLock::new(0)),
            last_command: Arc::new(RwLock::new(None)),
            should_fail: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn print(&self, commands: Vec<u8>) -> Result<(), String> {
        if !*self.is_online.read().await {
            return Err("Printer offline".to_string());
        }

        if *self.should_fail.read().await {
            return Err("Simulated printer failure".to_string());
        }

        *self.last_command.write().await = Some(commands);
        *self.print_count.write().await += 1;
        Ok(())
    }

    pub async fn set_online(&self, online: bool) {
        *self.is_online.write().await = online;
    }

    pub async fn set_should_fail(&self, fail: bool) {
        *self.should_fail.write().await = fail;
    }

    pub async fn get_print_count(&self) -> u32 {
        *self.print_count.read().await
    }

    pub async fn get_last_command(&self) -> Option<Vec<u8>> {
        self.last_command.read().await.clone()
    }
}

/// Test configuration builder
pub struct TestConfigBuilder {
    restaurant_id: String,
    location_id: String,
    temp_dir: Option<TempDir>,
}

#[allow(dead_code)]
impl TestConfigBuilder {
    pub fn new() -> Self {
        Self {
            restaurant_id: "test_rest_123".to_string(),
            location_id: "test_loc_456".to_string(),
            temp_dir: None,
        }
    }

    pub fn with_restaurant_id(mut self, id: &str) -> Self {
        self.restaurant_id = id.to_string();
        self
    }

    pub fn with_location_id(mut self, id: &str) -> Self {
        self.location_id = id.to_string();
        self
    }

    pub fn with_temp_dir(mut self, dir: TempDir) -> Self {
        self.temp_dir = Some(dir);
        self
    }

    pub fn build(self) -> TestConfig {
        TestConfig {
            restaurant_id: self.restaurant_id,
            location_id: self.location_id,
            temp_dir: self.temp_dir,
        }
    }
}

pub struct TestConfig {
    pub restaurant_id: String,
    pub location_id: String,
    pub temp_dir: Option<TempDir>,
}

impl TestConfig {
    pub fn get_db_path(&self) -> String {
        if let Some(ref dir) = self.temp_dir {
            format!("{}/test-queue.db", dir.path().display())
        } else {
            ":memory:".to_string()
        }
    }
}

/// Generate test JWT token
pub fn generate_test_jwt(restaurant_id: &str, location_id: &str, secret: &str) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        restaurant_id: String,
        location_id: String,
        permissions: Vec<String>,
        exp: usize,
    }

    let claims = Claims {
        restaurant_id: restaurant_id.to_string(),
        location_id: location_id.to_string(),
        permissions: vec!["print".to_string(), "status".to_string()],
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("Failed to generate test JWT")
}

/// Create test print job payload
pub fn create_test_print_job(order_id: &str, station: &str) -> serde_json::Value {
    serde_json::json!({
        "job_id": format!("job_{}", uuid::Uuid::new_v4()),
        "order_id": order_id,
        "station": station,
        "items": [
            {
                "name": "Test Item 1",
                "quantity": 2,
                "price": 10.50,
                "modifiers": ["No onions"]
            },
            {
                "name": "Test Item 2",
                "quantity": 1,
                "price": 15.00,
                "modifiers": []
            }
        ],
        "table_number": "T-05",
        "order_number": "R001-20260128-0042",
        "timestamp": chrono::Utc::now().timestamp()
    })
}
