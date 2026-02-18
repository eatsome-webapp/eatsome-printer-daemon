use crate::errors::{DaemonError, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, warn};

/// JWT Claims for printer service authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterClaims {
    /// Restaurant ID
    pub restaurant_id: String,
    /// Location ID (optional)
    pub location_id: Option<String>,
    /// Permissions
    pub permissions: Vec<String>,
    /// Issued at (Unix timestamp)
    pub iat: u64,
    /// Expires at (Unix timestamp)
    pub exp: u64,
}

impl PrinterClaims {
    /// Create new claims with default 24-hour expiration
    pub fn new(
        restaurant_id: String,
        location_id: Option<String>,
        permissions: Vec<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            restaurant_id,
            location_id,
            permissions,
            iat: now,
            exp: now + (24 * 60 * 60), // 24 hours
        }
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.exp < now
    }

    /// Check if token has specific permission
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(&permission.to_string())
    }

    /// Check if token is within rotation grace period (1 hour before expiration)
    pub fn needs_rotation(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Rotation window: 1 hour before expiration
        self.exp.saturating_sub(3600) < now
    }
}

/// JWT Token Manager for printer service authentication
pub struct JWTManager {
    /// Secret key for signing/verifying tokens
    secret: String,
}

impl JWTManager {
    /// Create new JWT manager with secret key
    pub fn new(secret: String) -> Self {
        Self { secret }
    }

    /// Generate JWT token from claims
    pub fn generate_token(&self, claims: &PrinterClaims) -> Result<String> {
        let token = encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| {
            error!("Failed to generate JWT token: {}", e);
            DaemonError::Other(anyhow::anyhow!("Failed to generate token: {}", e))
        })?;

        debug!("Generated JWT token for restaurant: {}", claims.restaurant_id);
        Ok(token)
    }

    /// Validate and decode JWT token
    pub fn validate_token(&self, token: &str) -> Result<PrinterClaims> {
        let mut validation = Validation::default();
        validation.validate_exp = true;

        let token_data = decode::<PrinterClaims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| {
            warn!("JWT validation failed: {}", e);
            DaemonError::Other(anyhow::anyhow!("Invalid token: {}", e))
        })?;

        let claims = token_data.claims;

        // Additional checks
        if claims.is_expired() {
            error!("Token expired for restaurant: {}", claims.restaurant_id);
            return Err(DaemonError::Other(anyhow::anyhow!("Token expired")));
        }

        debug!("Token validated for restaurant: {}", claims.restaurant_id);
        Ok(claims)
    }

    /// Validate token and check for specific permission
    pub fn validate_with_permission(&self, token: &str, permission: &str) -> Result<PrinterClaims> {
        let claims = self.validate_token(token)?;

        if !claims.has_permission(permission) {
            error!(
                "Insufficient permissions for restaurant {}: missing '{}'",
                claims.restaurant_id, permission
            );
            return Err(DaemonError::Other(anyhow::anyhow!(
                "Insufficient permissions: missing '{}'",
                permission
            )));
        }

        debug!(
            "Token validated with permission '{}' for restaurant: {}",
            permission, claims.restaurant_id
        );
        Ok(claims)
    }

    /// Validate token and check for restaurant ID match
    pub fn validate_for_restaurant(&self, token: &str, restaurant_id: &str) -> Result<PrinterClaims> {
        let claims = self.validate_token(token)?;

        if claims.restaurant_id != restaurant_id {
            error!(
                "Restaurant ID mismatch: token={}, expected={}",
                claims.restaurant_id, restaurant_id
            );
            return Err(DaemonError::Other(anyhow::anyhow!(
                "Restaurant ID mismatch"
            )));
        }

        Ok(claims)
    }

    /// Extract token from Authorization header (Bearer format)
    pub fn extract_bearer_token(auth_header: &str) -> Result<String> {
        if !auth_header.starts_with("Bearer ") {
            return Err(DaemonError::Other(anyhow::anyhow!(
                "Invalid Authorization header format"
            )));
        }

        Ok(auth_header.trim_start_matches("Bearer ").to_string())
    }
}

/// Token rotation handler for graceful token updates
pub struct TokenRotationHandler {
    jwt_manager: JWTManager,
    current_token: String,
    previous_token: Option<String>,
}

impl TokenRotationHandler {
    /// Create new rotation handler
    pub fn new(jwt_manager: JWTManager, initial_token: String) -> Self {
        Self {
            jwt_manager,
            current_token: initial_token,
            previous_token: None,
        }
    }

    /// Rotate token (store previous, set new current)
    pub fn rotate(&mut self, new_token: String) {
        debug!("Rotating token");
        self.previous_token = Some(self.current_token.clone());
        self.current_token = new_token;
    }

    /// Validate token (tries current, then previous during rotation window)
    pub fn validate(&self, token: &str) -> Result<PrinterClaims> {
        // Try current token
        if let Ok(claims) = self.jwt_manager.validate_token(token) {
            return Ok(claims);
        }

        // Try previous token (1-hour grace period)
        if let Some(prev_token) = &self.previous_token {
            if token == prev_token {
                if let Ok(claims) = self.jwt_manager.validate_token(prev_token) {
                    // Check if still within grace period
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    if claims.exp.saturating_sub(3600) < now {
                        warn!("Using previous token during rotation grace period");
                        return Ok(claims);
                    }
                }
            }
        }

        Err(DaemonError::Other(anyhow::anyhow!("Token validation failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate_token() {
        let secret = "test_secret_key_1234567890".to_string();
        let manager = JWTManager::new(secret);

        let claims = PrinterClaims::new(
            "rest_123".to_string(),
            Some("loc_456".to_string()),
            vec!["print".to_string(), "status".to_string()],
        );

        let token = manager.generate_token(&claims).unwrap();
        let validated = manager.validate_token(&token).unwrap();

        assert_eq!(validated.restaurant_id, "rest_123");
        assert_eq!(validated.location_id, Some("loc_456".to_string()));
        assert!(validated.has_permission("print"));
        assert!(validated.has_permission("status"));
        assert!(!validated.has_permission("admin"));
    }

    #[test]
    fn test_permission_check() {
        let secret = "test_secret_key_1234567890".to_string();
        let manager = JWTManager::new(secret);

        let claims = PrinterClaims::new(
            "rest_123".to_string(),
            None,
            vec!["print".to_string()],
        );

        let token = manager.generate_token(&claims).unwrap();

        // Should succeed with correct permission
        assert!(manager.validate_with_permission(&token, "print").is_ok());

        // Should fail with missing permission
        assert!(manager.validate_with_permission(&token, "admin").is_err());
    }

    #[test]
    fn test_restaurant_id_validation() {
        let secret = "test_secret_key_1234567890".to_string();
        let manager = JWTManager::new(secret);

        let claims = PrinterClaims::new("rest_123".to_string(), None, vec!["print".to_string()]);

        let token = manager.generate_token(&claims).unwrap();

        // Should succeed with correct restaurant ID
        assert!(manager.validate_for_restaurant(&token, "rest_123").is_ok());

        // Should fail with wrong restaurant ID
        assert!(manager.validate_for_restaurant(&token, "rest_999").is_err());
    }

    #[test]
    fn test_bearer_token_extraction() {
        let token = "example.jwt.token";
        let auth_header = format!("Bearer {}", token);

        let extracted = JWTManager::extract_bearer_token(&auth_header).unwrap();
        assert_eq!(extracted, token);

        // Should fail without Bearer prefix
        assert!(JWTManager::extract_bearer_token(token).is_err());
    }
}
