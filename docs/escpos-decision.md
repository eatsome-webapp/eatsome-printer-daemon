# ESC/POS Implementation Decision

**Date:** 2026-01-28
**Status:** Verified ✅

## Research Summary

Evaluated three options for ESC/POS thermal printing:

### 1. tauri-plugin-lnxdxtf-thermal-printer

- **Status:** Incomplete, minimal documentation
- **Pros:** Tauri-native plugin
- **Cons:** Limited features, unclear feature parity, no production releases

### 2. tauri-plugin-escpos

- **Status:** BLE-only support
- **Pros:** Uses eco_print library
- **Cons:** Only supports Bluetooth, missing USB/Network protocols

### 3. escpos Rust crate (v0.17.0)

- **Status:** Mature, well-documented
- **Pros:**
  - Full ESC/POS implementation
  - Graphics processing with dithering
  - Multiple transport layers (USB, Network, Serial)
  - Barcode/QR code support
  - Comprehensive status commands
- **Cons:** Not Tauri-specific (but works perfectly with our architecture)

## Decision

**Use custom ESC/POS implementation (src/escpos.rs) as primary, with escpos crate as optional dependency.**

### Rationale:

1. **Custom implementation is complete** - Has all required features:
   - Text formatting (bold, underline, inverse, size, alignment)
   - QR codes via qrcode crate
   - Barcodes (EAN13, Code39, Code128, etc.)
   - Paper cutting (full/partial)
   - Cash drawer kick
   - Table formatting
   - Kitchen receipt templates

2. **Control over command sequences** - Custom builder gives exact control over ESC/POS byte sequences for troubleshooting

3. **escpos crate as enhancement** - Added with `features = ["graphics", "codes_2d", "barcode", "usb_rusb", "serial"]` for:
   - Advanced image dithering
   - Built-in printer status checks
   - Fallback if custom implementation has issues

4. **No tauri plugin limitations** - Direct Rust implementation works seamlessly with Tauri architecture

## Feature Comparison

| Feature          | Custom (escpos.rs) | escpos crate         | tauri-plugin |
| ---------------- | ------------------ | -------------------- | ------------ |
| Text formatting  | ✅                 | ✅                   | ❓           |
| QR codes         | ✅                 | ✅ (codes_2d)        | ❓           |
| Barcodes         | ✅                 | ✅ (barcode)         | ❓           |
| Paper cut        | ✅                 | ✅                   | ❓           |
| Drawer kick      | ✅                 | ✅                   | ❓           |
| Images           | Basic              | Advanced (dithering) | ❓           |
| USB printing     | ✅ (via rusb)      | ✅ (usb_rusb)        | ⚠️ Limited   |
| Network printing | ✅ (TCP)           | ✅                   | ⚠️ TCP only  |
| Status checks    | Basic              | Comprehensive        | ❌           |
| Documentation    | ✅                 | ✅                   | ❌           |

## Implementation Strategy

1. **Primary:** Use custom ESCPOSBuilder for all standard receipt printing
2. **Fallback:** Use escpos crate if advanced image processing needed
3. **Future:** Consider migrating to escpos crate entirely if custom implementation proves insufficient

## Verification

Tested custom implementation supports:

- ✅ Kitchen receipt formatting (stations, items, modifiers)
- ✅ Test print generation
- ✅ Multi-line text with alignment
- ✅ Table formatting with dynamic column widths
- ✅ QR code generation (using qrcode crate)
- ✅ Barcode generation (EAN13, Code39, etc.)
- ✅ Paper cutting commands
- ✅ Cash drawer pulse

**Conclusion:** Custom implementation is production-ready. ESC/POS plugin verification PASSED ✅
