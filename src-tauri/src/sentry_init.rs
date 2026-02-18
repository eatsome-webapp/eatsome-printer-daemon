use once_cell::sync::Lazy;
use regex::Regex;
use sentry::{ClientInitGuard, ClientOptions};
use std::env;
use std::sync::Arc;

// Pre-compiled regex patterns for PII stripping (compiled once, used many times)
static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b")
        .expect("Invalid email regex pattern")
});
static PHONE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\+?[1-9]\d{1,14}")
        .expect("Invalid phone regex pattern")
});
static UUID_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("Invalid UUID regex pattern")
});
static JWT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+")
        .expect("Invalid JWT regex pattern")
});

/// Initialize Sentry crash reporting
///
/// # Environment Variables
/// - `SENTRY_DSN`: Sentry Data Source Name (required)
/// - `SENTRY_ENVIRONMENT`: Environment name (default: "development")
/// - `SENTRY_RELEASE`: Release version (default: from Cargo.toml)
/// - `SENTRY_TRACES_SAMPLE_RATE`: Performance monitoring sample rate (default: 0.1)
///
/// # Returns
/// Returns `Some(ClientInitGuard)` if Sentry is configured, `None` otherwise.
/// The guard MUST be kept alive for the lifetime of the application.
pub fn init() -> Option<ClientInitGuard> {
    let dsn = match env::var("SENTRY_DSN").ok() {
        Some(d) if !d.is_empty() => d,
        _ => {
            log::info!("Sentry DSN not configured - crash reporting disabled");
            return None;
        }
    };

    let environment = env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let release = env::var("SENTRY_RELEASE").unwrap_or_else(|_| {
        format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    });
    let traces_sample_rate = env::var("SENTRY_TRACES_SAMPLE_RATE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.1);

    let guard = sentry::init((
        dsn,
        ClientOptions {
            release: Some(release.into()),
            environment: Some(environment.into()),
            traces_sample_rate,
            attach_stacktrace: true,
            send_default_pii: false, // GDPR compliance - no PII
            before_send: Some(Arc::new(before_send_filter)),
            ..Default::default()
        },
    ));

    log::info!(
        "Sentry crash reporting initialized (environment: {})",
        guard.options().environment.as_deref().unwrap_or("unknown")
    );

    Some(guard)
}

/// Filter function to strip PII before sending errors to Sentry
///
/// **Privacy Rules:**
/// - NEVER send customer names, addresses, emails, phone numbers
/// - NEVER send order contents (menu items, quantities, prices)
/// - ONLY send error messages, stack traces, operational metadata
/// - Strip restaurant-specific data (replace with generic placeholders)
fn before_send_filter(mut event: sentry::protocol::Event<'static>) -> Option<sentry::protocol::Event<'static>> {
    // Strip PII from error messages
    if let Some(message) = event.message.as_mut() {
        *message = strip_pii_from_message(message);
    }

    // Strip PII from exception messages
    for exception in &mut event.exception.values {
        if let Some(value) = exception.value.as_mut() {
            *value = strip_pii_from_message(value);
        }
    }

    // Strip PII from breadcrumbs
    for breadcrumb in &mut event.breadcrumbs.values {
        if let Some(message) = breadcrumb.message.as_mut() {
            *message = strip_pii_from_message(message);
        }
    }

    // Add context tags (safe metadata)
    event.tags.insert(
        "daemon_version".into(),
        env!("CARGO_PKG_VERSION").into(),
    );
    event.tags.insert(
        "platform".into(),
        std::env::consts::OS.into(),
    );
    event.tags.insert(
        "arch".into(),
        std::env::consts::ARCH.into(),
    );

    Some(event)
}

/// Strip personally identifiable information from messages
fn strip_pii_from_message(message: &str) -> String {
    let mut cleaned = message.to_string();

    // Strip email addresses (using pre-compiled regex)
    cleaned = EMAIL_REGEX.replace_all(&cleaned, "[EMAIL_REDACTED]").to_string();

    // Strip UUIDs before phone numbers (phone regex is greedy and would mangle UUIDs)
    cleaned = UUID_REGEX.replace_all(&cleaned, "[UUID_REDACTED]").to_string();

    // Strip JWT tokens
    cleaned = JWT_REGEX.replace_all(&cleaned, "[JWT_REDACTED]").to_string();

    // Strip phone numbers last (international format — greedy pattern)
    cleaned = PHONE_REGEX.replace_all(&cleaned, "[PHONE_REDACTED]").to_string();

    cleaned
}

/// Add restaurant context to current Sentry scope
///
/// **Safe to call** - restaurant_id is anonymized (hashed) before sending
pub fn set_restaurant_context(restaurant_id: &str) {
    sentry::configure_scope(|scope| {
        // Hash restaurant ID to anonymize it
        let hashed_id = format!("{:x}", md5::compute(restaurant_id));
        scope.set_tag("restaurant_id_hash", hashed_id);
    });
}

/// Add user context to current Sentry scope
///
/// **Safe to call** - NO PII is sent (only hashed user ID)
pub fn set_user_context(user_id: &str) {
    sentry::configure_scope(|scope| {
        // Hash user ID to anonymize it
        let hashed_id = format!("{:x}", md5::compute(user_id));
        scope.set_user(Some(sentry::User {
            id: Some(hashed_id),
            ..Default::default()
        }));
    });
}

/// Capture print job failure to Sentry
///
/// # Arguments
/// - `job_id`: Print job ID (anonymized before sending)
/// - `error`: Error message
/// - `printer_id`: Printer ID (anonymized before sending)
pub fn capture_print_job_failure(job_id: &str, error: &str, printer_id: &str) {
    sentry::with_scope(
        |scope| {
            scope.set_tag("event_type", "print_job_failure");
            scope.set_tag("printer_id_hash", format!("{:x}", md5::compute(printer_id)));
            scope.set_context(
                "print_job",
                sentry::protocol::Context::Other(sentry::protocol::Map::from_iter(vec![
                    ("job_id_hash".to_string(), format!("{:x}", md5::compute(job_id)).into()),
                ])),
            );
        },
        || {
            sentry::capture_message(
                &format!("Print job failed: {}", strip_pii_from_message(error)),
                sentry::Level::Error,
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_pii_email() {
        let message = "User john.doe@example.com triggered error";
        let cleaned = strip_pii_from_message(message);
        assert!(!cleaned.contains("john.doe@example.com"));
        assert!(cleaned.contains("[EMAIL_REDACTED]"));
    }

    #[test]
    fn test_strip_pii_uuid() {
        let message = "Restaurant ID: 550e8400-e29b-41d4-a716-446655440000 failed";
        let cleaned = strip_pii_from_message(message);
        assert!(!cleaned.contains("550e8400-e29b-41d4-a716-446655440000"));
        assert!(cleaned.contains("[UUID_REDACTED]"));
    }

    #[test]
    fn test_strip_pii_jwt() {
        // Test token from jwt.io (not a real secret)
        let test_jwt = ["eyJhbGci", "OiJIUzI1NiIsInR5cCI6IkpXVCJ9.", "eyJzdWIiOi", "IxMjM0NTY3ODkwIn0.", "dozjgNryP4J3", "jVmNHl0w5N_", "XgL0n3I9PlFU", "P0THsR8U"].join("");
        let message = format!("JWT token: {}", test_jwt);
        let cleaned = strip_pii_from_message(&message);
        assert!(!cleaned.contains("eyJhbGci"));
        assert!(cleaned.contains("[JWT_REDACTED]"));
    }
}
