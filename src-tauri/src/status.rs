/// Print job status constants — single source of truth.
/// Used in both local SQLite and remote Supabase.
/// Must match Supabase CHECK constraint: ('pending', 'printing', 'completed', 'failed')
pub const PENDING: &str = "pending";
pub const PRINTING: &str = "printing";
pub const COMPLETED: &str = "completed";
pub const FAILED: &str = "failed";

// =============================================================================
// Hardware Status (DLE EOT response parsing)
// =============================================================================

use serde::{Deserialize, Serialize};

/// Real-time hardware status parsed from ESC/POS DLE EOT response bytes.
///
/// ESC/POS DLE EOT response format (each response is 1 byte):
///   n=1 (Printer): bit 3 = offline
///   n=2 (Offline cause): bit 2 = cover open, bit 3 = feed button, bit 5 = error
///   n=3 (Error cause): bit 2 = auto-cutter error, bit 5 = unrecoverable
///   n=4 (Paper sensor): bit 2+3 = paper near-end, bit 5+6 = paper end
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrinterHwStatus {
    pub online: bool,
    pub cover_open: bool,
    pub paper_present: bool,
    pub paper_near_end: bool,
    pub error: bool,
    pub cutter_error: bool,
}

impl PrinterHwStatus {
    /// Parse 4 DLE EOT response bytes into a structured status.
    ///
    /// Each response byte has bit 0 fixed to 0 and bit 7 fixed to 0 (per spec).
    /// The relevant status bits are documented inline.
    pub fn from_dle_eot(printer: u8, offline_cause: u8, error_cause: u8, paper: u8) -> Self {
        Self {
            // n=1: bit 3 set = offline
            online: (printer & 0x08) == 0,
            // n=2: bit 2 set = cover open
            cover_open: (offline_cause & 0x04) != 0,
            // n=4: bit 5 OR bit 6 set = paper end (no paper)
            paper_present: (paper & 0x60) == 0,
            // n=4: bit 2 OR bit 3 set = paper near-end
            paper_near_end: (paper & 0x0C) != 0,
            // n=2: bit 5 set = error condition
            error: (offline_cause & 0x20) != 0,
            // n=3: bit 2 set = auto-cutter error
            cutter_error: (error_cause & 0x04) != 0,
        }
    }

    /// Map hardware status to a Supabase-compatible status string.
    /// Must match the CHECK constraint: ('online', 'offline', 'error', 'paper_low', 'paper_out')
    pub fn to_status_string(&self) -> &str {
        if !self.online {
            "offline"
        } else if !self.paper_present {
            "paper_out"
        } else if self.paper_near_end {
            "paper_low"
        } else if self.error || self.cutter_error {
            "error"
        } else {
            "online"
        }
    }

    /// Returns a healthy "all clear" status (used as default/fallback)
    pub fn healthy() -> Self {
        Self {
            online: true,
            cover_open: false,
            paper_present: true,
            paper_near_end: false,
            error: false,
            cutter_error: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthy_printer() {
        // All zeros = printer online, no errors, paper present
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x00, 0x00, 0x00);
        assert!(status.online);
        assert!(!status.cover_open);
        assert!(status.paper_present);
        assert!(!status.paper_near_end);
        assert!(!status.error);
        assert!(!status.cutter_error);
        assert_eq!(status.to_status_string(), "online");
    }

    #[test]
    fn test_printer_offline() {
        // Bit 3 of printer byte = offline
        let status = PrinterHwStatus::from_dle_eot(0x08, 0x00, 0x00, 0x00);
        assert!(!status.online);
        assert_eq!(status.to_status_string(), "offline");
    }

    #[test]
    fn test_cover_open() {
        // Bit 2 of offline_cause = cover open, bit 5 = error
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x24, 0x00, 0x00);
        assert!(status.cover_open);
        assert!(status.error);
        assert_eq!(status.to_status_string(), "error");
    }

    #[test]
    fn test_paper_near_end() {
        // Bit 2+3 of paper byte = near-end
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x00, 0x00, 0x0C);
        assert!(status.paper_near_end);
        assert!(status.paper_present); // Still has paper
        assert_eq!(status.to_status_string(), "paper_low");
    }

    #[test]
    fn test_paper_out() {
        // Bit 5+6 of paper byte = paper end
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x00, 0x00, 0x60);
        assert!(!status.paper_present);
        assert_eq!(status.to_status_string(), "paper_out");
    }

    #[test]
    fn test_cutter_error() {
        // Bit 2 of error_cause = auto-cutter error
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x00, 0x04, 0x00);
        assert!(status.cutter_error);
        assert_eq!(status.to_status_string(), "error");
    }

    #[test]
    fn test_multiple_issues_priority() {
        // Offline + paper out — offline takes priority
        let status = PrinterHwStatus::from_dle_eot(0x08, 0x00, 0x00, 0x60);
        assert_eq!(status.to_status_string(), "offline");
    }

    #[test]
    fn test_paper_out_over_near_end() {
        // Both near-end and paper-end bits set — paper_out wins
        let status = PrinterHwStatus::from_dle_eot(0x00, 0x00, 0x00, 0x6C);
        assert!(!status.paper_present);
        assert!(status.paper_near_end);
        assert_eq!(status.to_status_string(), "paper_out");
    }

    #[test]
    fn test_healthy_helper() {
        let status = PrinterHwStatus::healthy();
        assert_eq!(status.to_status_string(), "online");
    }
}
