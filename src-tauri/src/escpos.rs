use image::DynamicImage;
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

/// Font selection
#[derive(Debug, Clone, Copy)]
pub enum Font {
    A = 0, // Standard (12x24)
    B = 1, // Compressed (9x17)
}

/// Character code page for international characters
#[derive(Debug, Clone, Copy)]
pub enum CodePage {
    PC437USA = 0,
    Katakana = 1,
    PC850Multilingual = 2,
    PC860Portuguese = 3,
    PC863CanadianFrench = 4,
    PC865Nordic = 5,
    WPC1252Latin1 = 16,
    PC866Cyrillic = 17,
    PC852Latin2 = 18,
    PC858Euro = 19,
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

    /// Open cash drawer on specific pin with custom timing
    ///
    /// # Arguments
    /// * `pin` - Connector pin (2 or 5)
    /// * `on_time_ms` - Pulse on-time in milliseconds (rounded to 2ms units)
    /// * `off_time_ms` - Pulse off-time in milliseconds (rounded to 2ms units)
    pub fn open_drawer_pin(&mut self, pin: u8, on_time_ms: u8, off_time_ms: u8) -> &mut Self {
        let pin_val = if pin == 5 { 1 } else { 0 };
        let t1 = on_time_ms / 2;
        let t2 = off_time_ms / 2;
        self.buffer.extend_from_slice(&[ESC, 0x70, pin_val, t1, t2]);
        self
    }

    /// Select font (Font A = standard 12x24, Font B = compressed 9x17)
    pub fn font(&mut self, font: Font) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x4d, font as u8]);
        self
    }

    /// Set custom line spacing (n units, printer-dependent: typically n/360 or n/180 inch)
    pub fn line_spacing(&mut self, n: u8) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x33, n]);
        self
    }

    /// Reset line spacing to default (1/6 inch)
    pub fn default_line_spacing(&mut self) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x32]);
        self
    }

    /// Set right-side character spacing (0-255, in half-dot units)
    pub fn char_spacing(&mut self, n: u8) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x20, n]);
        self
    }

    /// Select character code page for international character support
    ///
    /// Use `CodePage::WPC1252Latin1` for Western European (€, ü, é, etc.)
    /// Use `CodePage::PC858Euro` for Euro symbol on older printers
    pub fn code_page(&mut self, page: CodePage) -> &mut Self {
        self.buffer.extend_from_slice(&[ESC, 0x74, page as u8]);
        self
    }

    /// Set width and height multiplier independently (1-8 each)
    ///
    /// More flexible than `size()` which uses predefined combinations.
    /// width=2, height=3 means double-width, triple-height.
    pub fn size_wh(&mut self, width: u8, height: u8) -> &mut Self {
        let w = (width.clamp(1, 8) - 1) << 4;
        let h = height.clamp(1, 8) - 1;
        self.buffer.extend_from_slice(&[GS, 0x21, w | h]);
        self
    }

    /// Print raster bit image (monochrome bitmap)
    ///
    /// Converts DynamicImage to 1-bit bitmap and sends via GS v 0.
    /// Automatically resizes to fit paper width.
    pub fn raster_image(&mut self, img: &DynamicImage, max_width: u32) -> &mut Self {
        let gray = img.to_luma8();
        let (orig_width, orig_height) = gray.dimensions();

        // Resize if wider than max
        let (width, height) = if orig_width > max_width {
            let scale = max_width as f32 / orig_width as f32;
            (max_width, (orig_height as f32 * scale) as u32)
        } else {
            (orig_width, orig_height)
        };

        let resized = if width != orig_width {
            img.resize(width, height, image::imageops::FilterType::Lanczos3)
                .to_luma8()
        } else {
            gray
        };

        // Width in bytes (8 pixels per byte)
        let byte_width = ((width + 7) / 8) as u16;

        // GS v 0 - Print raster bit image
        // m=0 (normal size)
        self.buffer.extend_from_slice(&[GS, 0x76, 0x30, 0x00]);
        self.buffer.push(byte_width as u8);          // xL
        self.buffer.push((byte_width >> 8) as u8);   // xH
        self.buffer.push(height as u8);               // yL
        self.buffer.push((height >> 8) as u8);        // yH

        // Convert pixels to 1-bit bitmap (black < 128 threshold)
        for y in 0..height {
            for bx in 0..byte_width {
                let mut byte_val = 0u8;
                for bit in 0..8u32 {
                    let x = bx as u32 * 8 + bit;
                    if x < width {
                        let pixel = resized.get_pixel(x, y)[0];
                        if pixel < 128 {
                            byte_val |= 1 << (7 - bit);
                        }
                    }
                }
                self.buffer.push(byte_val);
            }
        }

        self
    }

    /// Write raw ESC/POS bytes (for commands not yet in the builder)
    pub fn raw(&mut self, data: &[u8]) -> &mut Self {
        self.buffer.extend_from_slice(data);
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

// ============================================================================
// ESC/POS Binary Parser (for print preview)
// ============================================================================

/// Text alignment for parsed elements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

/// Text style state for parsed elements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextStyle {
    pub bold: bool,
    pub underline: bool,
    pub double_width: bool,
    pub double_height: bool,
    pub inverted: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            bold: false,
            underline: false,
            double_width: false,
            double_height: false,
            inverted: false,
        }
    }
}

/// A single element in a parsed receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReceiptElement {
    Text {
        content: String,
        style: TextStyle,
        alignment: TextAlignment,
    },
    Feed {
        lines: u8,
    },
    Cut {
        partial: bool,
    },
}

/// Complete parsed receipt structure for frontend rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedReceipt {
    pub elements: Vec<ReceiptElement>,
    pub paper_width_mm: u16,
    pub char_width: u8,
}

/// Parse ESC/POS binary buffer into structured receipt data
///
/// Interprets ESC/POS commands (ESC @, ESC E, ESC a, GS !, etc.)
/// and converts them to a JSON-friendly structure that React can render
/// with monospace fonts to simulate thermal printer output.
pub fn parse_escpos(buffer: &[u8], paper_width: PaperWidth) -> ParsedReceipt {
    let char_width = paper_width as u8;
    let paper_width_mm = match paper_width {
        PaperWidth::Width58mm => 58,
        PaperWidth::Width80mm => 80,
    };

    let mut elements = Vec::new();
    let mut style = TextStyle::default();
    let mut alignment = TextAlignment::Left;
    let mut text_buf = String::new();
    let mut i = 0;

    while i < buffer.len() {
        match buffer[i] {
            // ESC commands (0x1B)
            0x1B if i + 1 < buffer.len() => {
                // Flush text buffer before processing command
                if !text_buf.is_empty() {
                    elements.push(ReceiptElement::Text {
                        content: text_buf.clone(),
                        style: style.clone(),
                        alignment: alignment.clone(),
                    });
                    text_buf.clear();
                }

                match buffer[i + 1] {
                    0x40 => {
                        // ESC @ - Initialize (reset)
                        style = TextStyle::default();
                        alignment = TextAlignment::Left;
                        i += 2;
                    }
                    0x45 if i + 2 < buffer.len() => {
                        // ESC E n - Bold on/off
                        style.bold = buffer[i + 2] != 0;
                        i += 3;
                    }
                    0x61 if i + 2 < buffer.len() => {
                        // ESC a n - Alignment
                        alignment = match buffer[i + 2] {
                            0 => TextAlignment::Left,
                            1 => TextAlignment::Center,
                            2 => TextAlignment::Right,
                            _ => TextAlignment::Left,
                        };
                        i += 3;
                    }
                    0x2D if i + 2 < buffer.len() => {
                        // ESC - n - Underline on/off
                        style.underline = buffer[i + 2] != 0;
                        i += 3;
                    }
                    0x4D if i + 2 < buffer.len() => {
                        // ESC M n - Font select (skip)
                        i += 3;
                    }
                    0x33 if i + 2 < buffer.len() => {
                        // ESC 3 n - Line spacing (skip)
                        i += 3;
                    }
                    0x32 => {
                        // ESC 2 - Default line spacing (skip)
                        i += 2;
                    }
                    0x20 if i + 2 < buffer.len() => {
                        // ESC SP n - Character spacing (skip)
                        i += 3;
                    }
                    0x74 if i + 2 < buffer.len() => {
                        // ESC t n - Code page (skip)
                        i += 3;
                    }
                    0x70 if i + 4 < buffer.len() => {
                        // ESC p - Cash drawer (skip 5 bytes)
                        i += 5;
                    }
                    _ => {
                        // Unknown ESC command, skip 2 bytes
                        i += 2;
                    }
                }
            }
            // GS commands (0x1D)
            0x1D if i + 1 < buffer.len() => {
                if !text_buf.is_empty() {
                    elements.push(ReceiptElement::Text {
                        content: text_buf.clone(),
                        style: style.clone(),
                        alignment: alignment.clone(),
                    });
                    text_buf.clear();
                }

                match buffer[i + 1] {
                    0x21 if i + 2 < buffer.len() => {
                        // GS ! n - Character size
                        let n = buffer[i + 2];
                        style.double_width = (n & 0x10) != 0;
                        style.double_height = (n & 0x01) != 0 || (n & 0x20) != 0;
                        i += 3;
                    }
                    0x42 if i + 2 < buffer.len() => {
                        // GS B n - Inverse on/off
                        style.inverted = buffer[i + 2] != 0;
                        i += 3;
                    }
                    0x56 if i + 2 < buffer.len() => {
                        // GS V n - Cut paper
                        let partial = buffer[i + 2] != 0;
                        elements.push(ReceiptElement::Cut { partial });
                        i += 3;
                    }
                    0x28 if i + 2 < buffer.len() && buffer[i + 2] == 0x6B => {
                        // GS ( k - QR code command (variable length, skip)
                        if i + 4 < buffer.len() {
                            let pl = buffer[i + 3] as usize;
                            let ph = buffer[i + 4] as usize;
                            let data_len = pl + (ph << 8);
                            i += 5 + data_len.min(buffer.len() - i - 5);
                        } else {
                            i += 3;
                        }
                    }
                    0x68 if i + 2 < buffer.len() => {
                        // GS h n - Barcode height (skip)
                        i += 3;
                    }
                    0x77 if i + 2 < buffer.len() => {
                        // GS w n - Barcode width (skip)
                        i += 3;
                    }
                    0x6B if i + 3 < buffer.len() => {
                        // GS k - Barcode (variable length, skip)
                        let data_len = buffer[i + 3] as usize;
                        i += 4 + data_len.min(buffer.len() - i - 4);
                    }
                    0x76 if i + 7 < buffer.len() => {
                        // GS v 0 - Raster image (skip entire image data)
                        let xl = buffer[i + 4] as usize;
                        let xh = buffer[i + 5] as usize;
                        let yl = buffer[i + 6] as usize;
                        let yh = buffer[i + 7] as usize;
                        let byte_width = xl + (xh << 8);
                        let height = yl + (yh << 8);
                        i += 8 + (byte_width * height).min(buffer.len() - i - 8);
                    }
                    _ => {
                        i += 2;
                    }
                }
            }
            // LF (Line Feed)
            0x0A => {
                if !text_buf.is_empty() {
                    elements.push(ReceiptElement::Text {
                        content: text_buf.clone(),
                        style: style.clone(),
                        alignment: alignment.clone(),
                    });
                    text_buf.clear();
                }
                elements.push(ReceiptElement::Feed { lines: 1 });
                i += 1;
            }
            // CR (Carriage Return) - skip, usually paired with LF
            0x0D => {
                i += 1;
            }
            // Regular printable text
            byte => {
                if byte >= 0x20 {
                    text_buf.push(byte as char);
                }
                i += 1;
            }
        }
    }

    // Flush remaining text
    if !text_buf.is_empty() {
        elements.push(ReceiptElement::Text {
            content: text_buf,
            style: style.clone(),
            alignment: alignment.clone(),
        });
    }

    ParsedReceipt {
        elements,
        paper_width_mm,
        char_width,
    }
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
