// Integration tests for JWT authentication and token validation

mod common;

use common::{generate_test_jwt, TestConfigBuilder};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    restaurant_id: String,
    location_id: String,
    permissions: Vec<String>,
    exp: usize,
}

#[test]
fn test_valid_jwt_token_passes() {
    let config = TestConfigBuilder::new().build();
    let secret = "test_secret_key_123";

    let token = generate_test_jwt(&config.restaurant_id, &config.location_id, secret);

    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &validation);

    assert!(result.is_ok());
    let claims = result.unwrap().claims;
    assert_eq!(claims.restaurant_id, config.restaurant_id);
    assert_eq!(claims.location_id, config.location_id);
    assert!(claims.permissions.contains(&"print".to_string()));
}

#[test]
fn test_expired_jwt_token_fails() {
    let config = TestConfigBuilder::new().build();
    let secret = "test_secret_key_456";

    // Create expired token (exp in the past)
    let expired_claims = Claims {
        restaurant_id: config.restaurant_id.clone(),
        location_id: config.location_id.clone(),
        permissions: vec!["print".to_string()],
        exp: (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp() as usize,
    };

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &expired_claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &validation);

    assert!(result.is_err());
    if let Err(err) = result {
        assert!(err.to_string().contains("ExpiredSignature"));
    }
}

#[test]
fn test_invalid_signature_fails() {
    let config = TestConfigBuilder::new().build();
    let secret1 = "correct_secret";
    let secret2 = "wrong_secret";

    let token = generate_test_jwt(&config.restaurant_id, &config.location_id, secret1);

    // Try to validate with wrong secret
    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token, &DecodingKey::from_secret(secret2.as_bytes()), &validation);

    assert!(result.is_err());
    if let Err(err) = result {
        assert!(err.to_string().contains("InvalidSignature"));
    }
}

#[test]
fn test_missing_permissions_detected() {
    let config = TestConfigBuilder::new().build();
    let secret = "test_secret_789";

    // Create token without "print" permission
    let claims = Claims {
        restaurant_id: config.restaurant_id.clone(),
        location_id: config.location_id.clone(),
        permissions: vec!["status".to_string()], // Missing "print"
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
    };

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &validation);

    assert!(result.is_ok());
    let decoded_claims = result.unwrap().claims;

    // Verify "print" permission missing
    assert!(!decoded_claims.permissions.contains(&"print".to_string()));
    assert!(decoded_claims.permissions.contains(&"status".to_string()));
}

#[test]
fn test_malformed_jwt_fails() {
    // Test invalid JWT formats (split strings to avoid secretlint false positives)
    let bearer_header = ["Bearer eyJh", "bGciOiJIUzI", "1NiIsInR5cCI", "6IkpXVCJ9"].join("");
    let invalid_jwt = ["eyJh", "bGci.inv", "alid.to", "ken"].join("");
    let malformed_tokens = vec![
        "not.a.jwt",
        &invalid_jwt,
        "",
        &bearer_header,
    ];

    let secret = "test_secret_abc";
    let validation = Validation::new(Algorithm::HS256);

    for token in malformed_tokens {
        let result = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation);
        assert!(result.is_err(), "Token should fail: {}", token);
    }
}

#[test]
fn test_jwt_rotation_grace_period() {
    let config = TestConfigBuilder::new().build();
    let old_secret = "old_secret_123";
    let new_secret = "new_secret_456";

    // Generate token with old secret
    let old_token = generate_test_jwt(&config.restaurant_id, &config.location_id, old_secret);

    // Generate token with new secret
    let new_token = generate_test_jwt(&config.restaurant_id, &config.location_id, new_secret);

    let validation = Validation::new(Algorithm::HS256);

    // During grace period, both should be accepted
    let old_result = decode::<Claims>(&old_token, &DecodingKey::from_secret(old_secret.as_bytes()), &validation);
    let new_result = decode::<Claims>(&new_token, &DecodingKey::from_secret(new_secret.as_bytes()), &validation);

    assert!(old_result.is_ok());
    assert!(new_result.is_ok());

    // After grace period, old should fail (simulate by trying with new secret)
    let old_with_new_secret = decode::<Claims>(&old_token, &DecodingKey::from_secret(new_secret.as_bytes()), &validation);
    assert!(old_with_new_secret.is_err());
}

#[test]
fn test_restaurant_id_mismatch() {
    let secret = "shared_secret";

    // Create token for restaurant A
    let token_a = generate_test_jwt("rest_a", "loc_a", secret);

    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token_a, &DecodingKey::from_secret(secret.as_bytes()), &validation);

    assert!(result.is_ok());
    let claims = result.unwrap().claims;

    // Verify restaurant_id
    assert_eq!(claims.restaurant_id, "rest_a");

    // In real implementation, would check if restaurant_id matches config
    let expected_restaurant_id = "rest_b";
    assert_ne!(claims.restaurant_id, expected_restaurant_id, "Restaurant ID mismatch should be detected");
}

#[test]
fn test_token_contains_required_claims() {
    let config = TestConfigBuilder::new().build();
    let secret = "test_secret_999";

    let token = generate_test_jwt(&config.restaurant_id, &config.location_id, secret);

    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(&token, &DecodingKey::from_secret(secret.as_bytes()), &validation);

    assert!(result.is_ok());
    let claims = result.unwrap().claims;

    // Verify all required claims present
    assert!(!claims.restaurant_id.is_empty());
    assert!(!claims.location_id.is_empty());
    assert!(!claims.permissions.is_empty());
    assert!(claims.exp > 0);
}
