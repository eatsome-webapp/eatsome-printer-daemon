use qrcode::QrCode;
use image::{DynamicImage, ImageBuffer, Luma};
use serde::{Deserialize, Serialize};

/// ESC/POS Commands (byte sequences)
const ESC: u8 = 0x1b;
const GS: u8 = 0x1d;
const LF: u8 = 0x0a;
const CR: u8 = 0x0d;

/// Paper width configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PaperWidth {
    Width58mm = 32, // 32 characters per line
    Width80mm = 48, // 48 characters per line
}

/// Text alignment
#[derive(Debug, Clone, Copy)]
pub enum Alignment {
    Left = 0,
    Center = 1,
    Right = 2,
}

/// Text size
#[derive(Debug, Clone, Copy)]
pub enum TextSize {
    Normal = 0x00,
    DoubleWidth = 0x10,
    DoubleHeight = 0x20,
    DoubleBoth = 0x30,
}

/// Barcode type
#[derive(Debug, Clone, Copy)]
pub enum BarcodeType {
    UPCA = 65,
    UPCE = 66,
    EAN13 = 67,
    EAN8 = 68,
    CODE39 = 69,
    ITF = 70,
    CODABAR = 71,
    CODE93 = 72,
    CODE128 = 73,
}

/// ESC/POS Command Builder
pub struct ESCPOSBuilder {
    buffer: Vec<u8>,
    paper_width: PaperWidth,
}

impl ESCPOSBuilder {
    pub fn new(paper_width: PaperWidth) -> Self {
        Self {
            buffer: Vec::new(),
            paper_width,
        }
    }

    /// Get the built command buffer
    pub fn build(self) -> Vec<u8> {
        self.buffer
    }

    /// Initialize printer
    pub fn initialize(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x40]);
        self
    }

    /// Add text
    pub fn text(&mut self, text: &str) -> &mut Self {
        self.buffer.extend_from_slice(text.as_bytes());
        self
    }

    /// Add line feed
    pub fn feed(&mut self, lines: u8) -> &mut Self {
        for _ in 0..lines {
            self.buffer.push(LF);
        }
        self
    }

    /// Add carriage return + line feed
    pub fn new_line(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[CR, LF]);
        self
    }

    /// Set text alignment
    pub fn align(&mut self, alignment: Alignment) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x61, alignment as u8]);
        self
    }

    /// Set text size
    pub fn size(&mut self, size: TextSize) -> &mut Self {
        self.buffer.extend_from_slice(&[GS, 0x21, size as u8]);
        self
    }

    /// Enable/disable bold
    pub fn bold(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x45, if enabled { 1 } else { 0 }]);
        self
    }

    /// Enable/disable underline
    pub fn underline(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x2d, if enabled { 1 } else { 0 }]);
        self
    }

    /// Enable/disable inverse (white text on black)
    pub fn inverse(&mut self, enabled: bool) -> &mut Self {
        self.buffer.extend_from_slice(&[GS, 0x42, if enabled { 1 } else { 0 }]);
        self
    }

    /// Draw horizontal line
    pub fn draw_line(&mut self, char: char) -> &mut Self {
        let line: String = char.to_string().repeat(self.paper_width as usize);
        self.text(&line).new_line()
    }

    /// Print barcode
    pub fn barcode(&mut self, data: &str, barcode_type: BarcodeType) -> &mut Self {
        // Set barcode height
        self.buffer.extend_from_slice(&[GS, 0x68, 80]); // 80 dots

        // Set barcode width
        self.buffer.extend_from_slice(&[GS, 0x77, 2]); // module width 2

        // Print barcode
        self.buffer.extend_from_slice(&[GS, 0x6b, barcode_type as u8, data.len() as u8]);
        self.buffer.extend_from_slice(data.as_bytes());

        self
    }

    /// Print QR code
    pub fn qr_code(&mut self, data: &str, size: u8) -> &mut Self {
        let data_bytes = data.as_bytes();
        let pl = ((data_bytes.len() + 3) % 256) as u8;
        let ph = ((data_bytes.len() + 3) / 256) as u8;

        // QR code model
        self.buffer.extend_from_slice(&[GS, 0x28, 0x6b, 0x04, 0x00, 0x31, 0x41, 0x32, 0x00]);

        // QR code size
        self.buffer.extend_from_slice(&[GS, 0x28, 0x6b, 0x03, 0x00, 0x31, 0x43, size]);

        // QR code error correction level (L=48, M=49, Q=50, H=51)
        self.buffer.extend_from_slice(&[GS, 0x28, 0x6b, 0x03, 0x00, 0x31, 0x45, 0x31]);

        // Store data
        self.buffer.extend_from_slice(&[GS, 0x28, 0x6b, pl, ph, 0x31, 0x50, 0x30]);
        self.buffer.extend_from_slice(data_bytes);

        // Print QR code
        self.buffer.extend_from_slice(&[GS, 0x28, 0x6b, 0x03, 0x00, 0x31, 0x51, 0x30]);

        self
    }

    /// Cut paper
    pub fn cut(&mut self, partial: bool) -> &mut Self {
        self.feed(3); // Feed before cut
        self.buffer.extend_from_slice(&[GS, 0x56, if partial { 1 } else { 0 }]);
        self
    }

    /// Open cash drawer (if connected)
    pub fn open_drawer(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x70, 0, 25, 250]);
        self
    }

    /// Add centered text (auto-calculated padding)
    pub fn center_text(&mut self, text: &str) -> &mut Self {
        let padding = (self.paper_width as usize - text.len()) / 2;
        let spaces = " ".repeat(padding.max(0));
        self.text(&format!("{}{}", spaces, text)).new_line()
    }

    /// Add left-right justified text
    pub fn justify_text(&mut self, left: &str, right: &str) -> &mut Self {
        let spaces = self.paper_width as usize - left.len() - right.len();
        let spacing = " ".repeat(spaces.max(1));
        self.text(&format!("{}{}{}", left, spacing, right)).new_line()
    }

    /// Add table row (multiple columns with auto-spacing)
    pub fn table_row(&mut self, columns: &[&str], widths: Option<&[usize]>) -> &mut Self {
        let default_widths;
        let widths = if let Some(w) = widths {
            w
        } else {
            let col_width = self.paper_width as usize / columns.len();
            default_widths = vec![col_width; columns.len()];
            &default_widths[..]
        };

        let mut row = String::new();
        for (i, col) in columns.iter().enumerate() {
            let width = widths.get(i).copied().unwrap_or(0);
            if col.len() > width {
                row.push_str(&col[..width]);
            } else {
                row.push_str(col);
                row.push_str(&" ".repeat(width - col.len()));
            }
        }

        let truncated = if row.len() > self.paper_width as usize {
            &row[..self.paper_width as usize]
        } else {
            &row
        };

        self.text(truncated).new_line()
    }
}

/// Format kitchen receipt
pub fn format_kitchen_receipt(
    station: &str,
    order_number: &str,
    order_type: Option<&str>,
    table_number: Option<&str>,
    customer_name: Option<&str>,
    priority: u8,
    items: &[PrintItem],
    timestamp: i64,
    paper_width: PaperWidth,
) -> Vec<u8> {
    let mut builder = ESCPOSBuilder::new(paper_width);

    builder
        .initialize()
        .align(Alignment::Center)
        .size(TextSize::DoubleBoth)
        .bold(true)
        .text(&station.to_uppercase())
        .new_line()
        .bold(false)
        .size(TextSize::Normal)
        .draw_line('=');

    // Order information
    builder
        .align(Alignment::Left)
        .size(TextSize::DoubleWidth)
        .bold(true)
        .text(&format!("ORDER {}", order_number))
        .new_line()
        .size(TextSize::Normal)
        .bold(false);

    if let Some(order_type) = order_type {
        builder.text(&format!("Type: {}", order_type.to_uppercase())).new_line();
    }

    if let Some(table) = table_number {
        builder.text(&format!("Table: {}", table)).new_line();
    }

    if let Some(customer) = customer_name {
        builder.text(&format!("Customer: {}", customer)).new_line();
    }

    // Priority indicator
    if priority == 1 {
        builder.inverse(true).bold(true).text(" URGENT ").inverse(false).bold(false).new_line();
    }

    builder.draw_line('-');

    // Items
    for item in items {
        builder
            .bold(true)
            .size(TextSize::DoubleHeight)
            .text(&format!("{}x {}", item.quantity, item.name))
            .new_line()
            .size(TextSize::Normal)
            .bold(false);

        // Modifiers
        for modifier in &item.modifiers {
            builder.text(&format!("  + {}", modifier)).new_line();
        }

        // Notes
        if let Some(notes) = &item.notes {
            builder.underline(true).text(&format!("  NOTE: {}", notes)).underline(false).new_line();
        }

        builder.feed(1);
    }

    builder.draw_line('-');

    // Timestamp
    let time_str = chrono::DateTime::from_timestamp(timestamp / 1000, 0)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| "??:??".to_string());

    builder
        .align(Alignment::Center)
        .text(&format!("Printed: {}", time_str))
        .new_line()
        .feed(2)
        .cut(false);

    builder.build()
}

/// Print item for receipts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintItem {
    pub quantity: u32,
    pub name: String,
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// Format test print
pub fn format_test_print(paper_width: PaperWidth) -> Vec<u8> {
    let mut builder = ESCPOSBuilder::new(paper_width);

    builder
        .initialize()
        .align(Alignment::Center)
        .size(TextSize::DoubleBoth)
        .bold(true)
        .text("TEST PRINT")
        .new_line()
        .size(TextSize::Normal)
        .bold(false)
        .draw_line('=')
        .align(Alignment::Left)
        .text("Printer is working correctly!")
        .new_line()
        .feed(1)
        .text(&format!(
            "Paper width: {}",
            match paper_width {
                PaperWidth::Width58mm => "58mm",
                PaperWidth::Width80mm => "80mm",
            }
        ))
        .new_line()
        .text(&format!("Timestamp: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")))
        .new_line()
        .draw_line('-')
        .align(Alignment::Center)
        .text("Text Formatting Tests:")
        .new_line()
        .feed(1)
        .bold(true)
        .text("Bold Text")
        .new_line()
        .bold(false)
        .underline(true)
        .text("Underlined Text")
        .new_line()
        .underline(false)
        .inverse(true)
        .text("Inverse Text")
        .new_line()
        .inverse(false)
        .size(TextSize::DoubleWidth)
        .text("Double Width")
        .new_line()
        .size(TextSize::DoubleHeight)
        .text("Double Height")
        .new_line()
        .size(TextSize::Normal)
        .draw_line('=')
        .feed(1)
        .qr_code("https://eatsome.nl", 5)
        .feed(1)
        .text("QR Code Test")
        .new_line()
        .feed(2)
        .cut(false);

    builder.build()
}
