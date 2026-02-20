# Sentry Crash Reporting Setup

Complete guide for integrating Sentry crash reporting into the Eatsome Printer Service.

## Overview

Sentry provides real-time error tracking and performance monitoring for both:

- **Rust Backend** (main process) - Captures panics, errors, and performance issues
- **React Frontend** (renderer process) - Captures JS errors, UI issues, and user sessions

**Privacy:** All PII (customer names, emails, order contents) is stripped before sending to Sentry.

## Setup Steps

### 1. Create Sentry Account

1. Go to [https://sentry.io/signup/](https://sentry.io/signup/)
2. Sign up for free account (5,000 events/month)
3. Choose "Rust" as primary platform

### 2. Create Project

1. Click "Create Project"
2. Platform: **Rust**
3. Project name: `eatsome-printer-service`
4. Set alert frequency: **On every new issue**
5. Click "Create Project"

### 3. Get DSN (Data Source Name)

1. Go to Project Settings → Client Keys (DSN)
2. Copy the DSN (format: `https://<key>@<organization>.ingest.sentry.io/<project-id>`)
3. Example: `https://abc123def456@o123456.ingest.sentry.io/789012`

### 4. Configure Environment Variables

Create `.env` file in `apps/printer-daemon-tauri/`:

```bash
# Rust Backend
SENTRY_DSN=https://abc123def456@o123456.ingest.sentry.io/789012
SENTRY_ENVIRONMENT=production
SENTRY_TRACES_SAMPLE_RATE=0.1

# React Frontend (Vite requires VITE_ prefix)
VITE_SENTRY_DSN=https://abc123def456@o123456.ingest.sentry.io/789012
VITE_SENTRY_ENVIRONMENT=production
VITE_SENTRY_TRACES_SAMPLE_RATE=0.1
VITE_APP_VERSION=1.0.0
```

**Important:** Add `.env` to `.gitignore` (already done) - NEVER commit DSN to git!

### 5. Install Dependencies

```bash
cd apps/printer-daemon-tauri
pnpm install  # Installs @sentry/tauri
```

Rust dependencies are already configured in `Cargo.toml`:

```toml
sentry = { version = "0.34", features = ["backtrace", "contexts", "panic", "reqwest", "rustls"] }
sentry-tracing = "0.34"
```

### 6. Test Error Reporting

**Test Rust Backend:**

```rust
// Add to any Rust file temporarily
panic!("Test panic - this should appear in Sentry");
```

**Test React Frontend:**

```typescript
// Add to any React component temporarily
throw new Error('Test error - this should appear in Sentry')
```

**Verify in Sentry:**

1. Go to Sentry dashboard
2. Navigate to Issues
3. You should see test errors within 1-2 seconds

## Privacy & GDPR Compliance

### What We NEVER Send

❌ Customer names
❌ Customer emails
❌ Customer phone numbers
❌ Customer addresses
❌ Order contents (menu items, quantities, prices)
❌ Restaurant addresses
❌ Credit card numbers
❌ Any payment information
❌ JWT tokens (in plain text)
❌ Database passwords

### What We DO Send

✅ Error messages (with PII stripped)
✅ Stack traces
✅ Anonymized restaurant ID (hashed)
✅ Anonymized user ID (hashed)
✅ Printer model/vendor (not serial numbers)
✅ App version
✅ Platform (macOS/Windows/Linux)
✅ Event timestamps
✅ Performance metrics

### PII Stripping

All data is automatically filtered through `strip_pii_from_message()` which:

**Emails:** `john.doe@example.com` → `[EMAIL_REDACTED]`
**Phone:** `+1234567890` → `[PHONE_REDACTED]`
**UUIDs:** `550e8400-e29b-41d4-a716-446655440000` → `[UUID_REDACTED]`
**JWT:** `eyJhbGci...` → `[JWT_REDACTED]`

### GDPR Settings

Configure in Sentry dashboard → Settings → Security & Privacy:

- **Data Retention:** 30 days (default: 90 days)
- **IP Anonymization:** Enabled
- **Send Default PII:** Disabled
- **Data Scrubbing:** Enabled

## Error Context

### Rust Backend Context

Automatically captured:

```rust
// Restaurant context (anonymized)
sentry_init::set_restaurant_context(&restaurant_id);

// User context (anonymized)
sentry_init::set_user_context(&user_id);

// Print job failures
sentry_init::capture_print_job_failure(&job_id, &error, &printer_id);
```

### React Frontend Context

Automatically captured:

```typescript
// Restaurant context (anonymized)
setRestaurantContext(restaurantId)

// User context (anonymized)
setUserContext(userId)

// Setup wizard progress
captureSetupProgress('printer-discovery', { printerCount: 3 })

// Printer discovery
capturePrinterDiscovery(3, ['usb', 'network'])

// Authentication
captureAuthentication(true)
```

## Performance Monitoring

### Transaction Sampling

Default: **10% of transactions** are sent to Sentry

**Why not 100%?**

- Reduces Sentry quota usage (free tier: 10,000 transactions/month)
- Performance overhead on high-volume restaurants

**Increase for debugging:**

```bash
SENTRY_TRACES_SAMPLE_RATE=1.0  # 100% sampling
```

### Custom Transactions

```rust
// Rust: Track print job processing time
let transaction = sentry::start_transaction(
    sentry::TransactionContext::new("print_job", "task")
);
// ... print job logic
transaction.finish();
```

```typescript
// React: Track setup wizard time
const transaction = Sentry.startTransaction({
  name: 'setup-wizard',
  op: 'ui.action',
})
// ... wizard logic
transaction.finish()
```

## Alerts & Notifications

### Email Alerts

Configure in Sentry dashboard → Alerts:

1. **High Error Rate:**
   - Condition: More than 10 errors in 5 minutes
   - Action: Email team
   - Example: Printer offline affecting multiple orders

2. **New Error Type:**
   - Condition: First occurrence of error
   - Action: Email team + Slack
   - Example: New crash due to printer firmware update

3. **Performance Degradation:**
   - Condition: P95 latency > 500ms
   - Action: Email team
   - Example: Slow database queries

### Slack Integration

1. Go to Sentry dashboard → Settings → Integrations
2. Click "Slack"
3. Authorize workspace
4. Create alert rule:
   - Channel: `#printer-service-alerts`
   - Frequency: **On every new issue**

## Session Replay (React Frontend)

Sentry Session Replay captures user interactions for debugging UI issues.

**Privacy Settings:**

```typescript
replaysSessionSampleRate: 0.1,  // 10% of sessions
replaysOnErrorSampleRate: 1.0,  // 100% of sessions with errors
maskAllText: true,               // Hide all text content
blockAllMedia: true,             // Block images/videos
```

**What's Captured:**

- ✅ Mouse movements (not click coordinates)
- ✅ Page navigation
- ✅ Button clicks (not which button)
- ❌ Text input (masked)
- ❌ Images (blocked)
- ❌ Videos (blocked)

**Use Cases:**

- Understanding "button doesn't work" reports
- Debugging complex UI state issues
- Reproducing crashes

## Dashboard & Analysis

### Key Metrics to Monitor

1. **Error Rate:**
   - Go to Sentry dashboard → Stats
   - Track errors/minute over time
   - Spike = new issue or production incident

2. **Affected Users:**
   - Go to Issues → Sort by "Users affected"
   - Prioritize fixes based on impact

3. **Most Common Errors:**
   - Go to Issues → Sort by "Events"
   - Identify systematic problems

4. **Performance:**
   - Go to Performance → Transactions
   - Identify slow operations

### Custom Dashboards

Create custom dashboard for Eatsome:

1. Go to Dashboards → Create Dashboard
2. Add widgets:
   - **Error Rate:** Line chart, last 24 hours
   - **Top Errors:** Table, last 7 days
   - **Print Job Failures:** Bar chart, grouped by printer
   - **Platform Distribution:** Pie chart (macOS/Windows/Linux)

## Troubleshooting

### "Sentry DSN not configured" in Logs

**Cause:** `.env` file missing or DSN empty

**Fix:**

1. Create `.env` file in `apps/printer-daemon-tauri/`
2. Add `SENTRY_DSN=...` and `VITE_SENTRY_DSN=...`
3. Restart daemon

### Errors Not Appearing in Sentry

**Cause:** Incorrect DSN or network firewall

**Fix:**

1. Verify DSN format: `https://<key>@<org>.ingest.sentry.io/<project>`
2. Test network: `curl https://sentry.io/api/0/`
3. Check firewall allows outbound HTTPS to `*.ingest.sentry.io`

### Too Many Events (Quota Exceeded)

**Cause:** High error rate or sampling too high

**Fix:**

1. Lower sampling rate: `SENTRY_TRACES_SAMPLE_RATE=0.05` (5%)
2. Add ignore rules in Sentry dashboard:
   - Settings → Inbound Filters
   - Ignore specific error types (e.g., network timeouts)
3. Upgrade to paid plan if needed

### PII Leaking Through

**Cause:** New PII pattern not caught by filters

**Fix:**

1. Update `strip_pii_from_message()` in `sentry_init.rs`
2. Add regex for new pattern
3. Test with: `strip_pii_from_message("test data with PII")`
4. Redeploy

## Cost Management

### Free Tier Limits

- **Events:** 5,000/month
- **Transactions:** 10,000/month
- **Attachments:** 1GB
- **Replay:** 50 sessions/month

**Estimate for Eatsome:**

- 100 restaurants
- 10 errors/restaurant/month = 1,000 events
- **Well within free tier** ✅

### Paid Plans

If exceeding free tier:

| Plan         | Events/Month | Cost      |
| ------------ | ------------ | --------- |
| **Team**     | 50,000       | $26/month |
| **Business** | 100,000      | $80/month |

**Recommendation:** Start with free tier, upgrade if needed

## Security

### Protecting the DSN

**DSN is NOT secret** - it's safe to include in client-side apps (React).

**Why?**

- Sentry validates events server-side (can't inject fake data)
- Rate limiting prevents abuse
- IP allowlist available for extra security

**BUT:** Don't hardcode DSN in public repos (use environment variables)

### Restricting Access

Configure in Sentry dashboard → Settings → Auth:

1. **Two-Factor Auth:** Enable for all team members
2. **IP Allowlist:** Restrict access to office IPs
3. **Roles:** Grant minimum necessary permissions

## References

- [Sentry Rust SDK](https://docs.sentry.io/platforms/rust/)
- [Sentry React SDK](https://docs.sentry.io/platforms/javascript/guides/react/)
- [Sentry GDPR Compliance](https://sentry.io/security/)
- [Sentry Pricing](https://sentry.io/pricing/)
