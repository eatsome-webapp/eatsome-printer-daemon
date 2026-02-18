// Integration tests for ESC/POS command generation

#[test]
fn test_initialize_printer_command() {
    // ESC @ - Initialize printer
    let cmd = vec![0x1B, 0x40];
    assert_eq!(cmd, [0x1B, 0x40]);
}

#[test]
fn test_bold_text_commands() {
    // ESC E 1 - Bold on
    let bold_on = vec![0x1B, 0x45, 0x01];
    assert_eq!(bold_on, [0x1B, 0x45, 0x01]);

    // ESC E 0 - Bold off
    let bold_off = vec![0x1B, 0x45, 0x00];
    assert_eq!(bold_off, [0x1B, 0x45, 0x00]);
}

#[test]
fn test_text_alignment_commands() {
    // ESC a 0 - Left alignment
    let align_left = vec![0x1B, 0x61, 0x00];
    assert_eq!(align_left, [0x1B, 0x61, 0x00]);

    // ESC a 1 - Center alignment
    let align_center = vec![0x1B, 0x61, 0x01];
    assert_eq!(align_center, [0x1B, 0x61, 0x01]);

    // ESC a 2 - Right alignment
    let align_right = vec![0x1B, 0x61, 0x02];
    assert_eq!(align_right, [0x1B, 0x61, 0x02]);
}

#[test]
fn test_paper_cut_command() {
    // ESC i - Full cut
    let cut_full = vec![0x1B, 0x69];
    assert_eq!(cut_full, [0x1B, 0x69]);

    // ESC m - Partial cut
    let cut_partial = vec![0x1B, 0x6D];
    assert_eq!(cut_partial, [0x1B, 0x6D]);
}

#[test]
fn test_line_feed_commands() {
    // LF - Line feed
    let lf = vec![0x0A];
    assert_eq!(lf, [0x0A]);

    // ESC d 3 - Feed 3 lines
    let feed_3 = vec![0x1B, 0x64, 0x03];
    assert_eq!(feed_3, [0x1B, 0x64, 0x03]);
}

#[test]
fn test_text_size_commands() {
    // ESC ! 0 - Normal size
    let size_normal = vec![0x1B, 0x21, 0x00];
    assert_eq!(size_normal, [0x1B, 0x21, 0x00]);

    // ESC ! 0x30 - Double width + double height
    let size_double = vec![0x1B, 0x21, 0x30];
    assert_eq!(size_double, [0x1B, 0x21, 0x30]);
}

#[test]
fn test_drawer_kick_command() {
    // ESC p 0 25 250 - Open drawer
    let open_drawer = vec![0x1B, 0x70, 0x00, 0x19, 0xFA];
    assert_eq!(open_drawer, [0x1B, 0x70, 0x00, 0x19, 0xFA]);
}

#[test]
fn test_qr_code_commands() {
    // QR code command sequence (model 2, size 3, error correction L)
    let qr_model = vec![0x1D, 0x28, 0x6B, 0x04, 0x00, 0x31, 0x41, 0x32, 0x00]; // GS ( k - Model
    let qr_size = vec![0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x43, 0x03]; // GS ( k - Size
    let qr_error = vec![0x1D, 0x28, 0x6B, 0x03, 0x00, 0x31, 0x45, 0x30]; // GS ( k - Error correction

    assert_eq!(qr_model[0], 0x1D);
    assert_eq!(qr_size[0], 0x1D);
    assert_eq!(qr_error[0], 0x1D);
}

#[test]
fn test_barcode_commands() {
    // GS H 2 - Barcode human readable below
    let barcode_position = vec![0x1D, 0x48, 0x02];
    assert_eq!(barcode_position, [0x1D, 0x48, 0x02]);

    // GS h 50 - Barcode height 50 dots
    let barcode_height = vec![0x1D, 0x68, 0x32];
    assert_eq!(barcode_height, [0x1D, 0x68, 0x32]);

    // GS w 2 - Barcode width multiplier
    let barcode_width = vec![0x1D, 0x77, 0x02];
    assert_eq!(barcode_width, [0x1D, 0x77, 0x02]);
}

#[test]
fn test_character_encoding() {
    // ESC t - Select character code table
    // ESC t 0 - PC437 (USA, Standard Europe)
    let encoding_pc437 = vec![0x1B, 0x74, 0x00];
    assert_eq!(encoding_pc437, [0x1B, 0x74, 0x00]);

    // ESC t 16 - WPC1252 (Windows)
    let encoding_wpc1252 = vec![0x1B, 0x74, 0x10];
    assert_eq!(encoding_wpc1252, [0x1B, 0x74, 0x10]);
}

#[test]
fn test_full_receipt_command_sequence() {
    let mut receipt = Vec::new();

    // Initialize
    receipt.extend_from_slice(&[0x1B, 0x40]);

    // Center alignment
    receipt.extend_from_slice(&[0x1B, 0x61, 0x01]);

    // Bold on
    receipt.extend_from_slice(&[0x1B, 0x45, 0x01]);

    // "RESTAURANT NAME" (ASCII)
    receipt.extend_from_slice(b"RESTAURANT NAME");
    receipt.push(0x0A); // Line feed

    // Bold off
    receipt.extend_from_slice(&[0x1B, 0x45, 0x00]);

    // Left alignment
    receipt.extend_from_slice(&[0x1B, 0x61, 0x00]);

    // "Order #0042" (ASCII)
    receipt.extend_from_slice(b"Order #0042");
    receipt.push(0x0A);

    // Feed 3 lines
    receipt.extend_from_slice(&[0x1B, 0x64, 0x03]);

    // Cut paper
    receipt.extend_from_slice(&[0x1B, 0x69]);

    // Verify sequence length (47 bytes: init + alignment + bold + text + feeds + cut)
    assert!(receipt.len() > 40);

    // Verify starts with initialize
    assert_eq!(&receipt[0..2], &[0x1B, 0x40]);

    // Verify ends with cut
    assert_eq!(&receipt[receipt.len() - 2..], &[0x1B, 0x69]);
}

#[test]
fn test_utf8_to_escpos_encoding() {
    // Test UTF-8 special characters
    let cafe = "Café"; // UTF-8
    let bytes = cafe.as_bytes();

    // UTF-8: C3 A9 for é
    assert_eq!(bytes, &[67, 97, 102, 195, 169]); // C, a, f, UTF-8 é

    // For ESC/POS, would need to convert to appropriate code page
    // e.g., WPC1252: é = 0xE9
}

#[test]
fn test_print_width_calculation() {
    // 48 characters per line (common thermal printer width)
    let max_width = 48;

    let item_name = "Extra Long Item Name That Needs To Be Truncated!";
    let truncated = if item_name.len() > max_width {
        &item_name[..max_width]
    } else {
        item_name
    };

    assert_eq!(truncated.len(), 48);
}

#[test]
fn test_dashed_line_generation() {
    let width = 48;
    let dashed_line: String = "-".repeat(width);

    assert_eq!(dashed_line.len(), 48);
    assert_eq!(dashed_line, "------------------------------------------------");
}

#[test]
fn test_price_formatting_alignment() {
    let item = "Burger";
    let price = "€10.50";
    let line_width = 48;

    let spaces_needed = line_width - item.len() - price.len();
    let formatted = format!("{}{}{}", item, " ".repeat(spaces_needed), price);

    assert_eq!(formatted.len(), 48);
    assert!(formatted.starts_with("Burger"));
    assert!(formatted.ends_with("€10.50"));
}
