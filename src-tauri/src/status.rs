/// Print job status constants — single source of truth.
/// Used in both local SQLite and remote Supabase.
/// Must match Supabase CHECK constraint: ('pending', 'printing', 'completed', 'failed')
pub const PENDING: &str = "pending";
pub const PRINTING: &str = "printing";
pub const COMPLETED: &str = "completed";
pub const FAILED: &str = "failed";
