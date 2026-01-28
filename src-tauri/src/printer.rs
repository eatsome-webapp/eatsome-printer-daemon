use crate::config::{ConnectionType, PrinterConfig};
use crate::discovery::{self, DiscoveredPrinter};
use crate::errors::{DaemonError, Result};
use crate::escpos::{format_test_print, PaperWidth};
use rusb::{Context, Device, DeviceDescriptor, UsbContext};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Known thermal printer vendor IDs
const VENDOR_IDS: &[(u16, &str)] = &[
    (0x04b8, "Epson"),
    (0x0519, "Star Micronics"),
    (0x04f9, "Brother"),
    (0x1d90, "Citizen"),
    (0x0fe6, "ICS Advent"),
    (0x154f, "Wincor Nixdorf"),
];

pub struct PrinterManager {
    printers: Arc<Mutex<HashMap<String, PrinterConfig>>>,
    usb_context: Context,
    online_cache: Arc<Mutex<HashMap<String, (bool, std::time::Instant)>>>,
}

impl PrinterManager {
    pub fn new() -> Self {
        info!("Initializing PrinterManager");
        Self {
            printers: Arc::new(Mutex::new(HashMap::new())),
            usb_context: Context::new().expect("Failed to initialize USB context"),
            online_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Discover all printers (USB + Network + Bluetooth)
    pub async fn discover_all(&self) -> Result<Vec<serde_json::Value>> {
        debug!("Starting comprehensive printer discovery");
        let mut discovered = Vec::new();

        // Discover USB printers
        let usb_printers = self.discover_usb()?;
        info!("Discovered {} USB printers", usb_printers.len());
        discovered.extend(
            usb_printers
                .into_iter()
                .map(|p| serde_json::to_value(p).unwrap()),
        );

        // Discover network printers via mDNS/Zeroconf
        match discovery::discover_network_printers().await {
            Ok(network_printers) => {
                info!("Discovered {} network printers via mDNS", network_printers.len());
                discovered.extend(
                    network_printers
                        .into_iter()
                        .map(|p| serde_json::to_value(p).unwrap()),
                );
            }
            Err(e) => {
                warn!("Network discovery failed: {}", e);
            }
        }

        // Discover Bluetooth printers (BLE scan)
        match discovery::discover_bluetooth_printers().await {
            Ok(bluetooth_printers) => {
                info!("Discovered {} Bluetooth printers", bluetooth_printers.len());
                discovered.extend(
                    bluetooth_printers
                        .into_iter()
                        .map(|p| serde_json::to_value(p).unwrap()),
                );
            }
            Err(e) => {
                warn!("Bluetooth discovery failed: {}", e);
            }
        }

        info!("Total printers discovered: {}", discovered.len());
        Ok(discovered)
    }

    /// Discover USB printers
    fn discover_usb(&self) -> Result<Vec<DiscoveredPrinter>> {
        let mut discovered = Vec::new();

        for device in self.usb_context.devices()?.iter() {
            let device_desc = device.device_descriptor()?;

            // Check if vendor ID matches known thermal printer manufacturers
            if let Some((_, vendor_name)) = VENDOR_IDS
                .iter()
                .find(|(vid, _)| *vid == device_desc.vendor_id())
            {
                let id = format!("usb_{:04x}_{:04x}", device_desc.vendor_id(), device_desc.product_id());
                let name = self.get_usb_product_name(&device, &device_desc)
                    .unwrap_or_else(|_| format!("{} Printer", vendor_name));

                let address = format!(
                    "/dev/bus/usb/{:03}/{:03}",
                    device.bus_number(),
                    device.address()
                );

                discovered.push(DiscoveredPrinter {
                    id,
                    name,
                    connection_type: "usb".to_string(),
                    address,
                    vendor: vendor_name.to_string(),
                    capabilities: Some(serde_json::json!({
                        "cutter": true,
                        "drawer": false,
                        "qrcode": true,
                        "maxWidth": 48
                    })),
                });
            }
        }

        Ok(discovered)
    }

    /// Get USB product name from device descriptor
    fn get_usb_product_name<T: UsbContext>(
        &self,
        device: &Device<T>,
        device_desc: &DeviceDescriptor,
    ) -> Result<String> {
        let handle = device.open()?;
        let timeout = Duration::from_secs(1);
        let languages = handle.read_languages(timeout)?;

        if let Some(language) = languages.first() {
            let product_string = handle
                .read_product_string(*language, device_desc, timeout)?;
            Ok(product_string)
        } else {
            Err(DaemonError::Other(anyhow::anyhow!("No language found")))
        }
    }

    /// Test print on a specific printer
    pub async fn test_print(&self, printer_id: &str) -> Result<()> {
        info!("Test print requested for printer: {}", printer_id);

        let printers = self.printers.lock().await;
        let printer = printers
            .get(printer_id)
            .ok_or_else(|| {
                error!("Printer not found: {}", printer_id);
                DaemonError::PrinterNotFound(printer_id.to_string())
            })?;

        let commands = format_test_print(PaperWidth::Width80mm);
        debug!("Generated test print commands: {} bytes", commands.len());

        let result = match printer.connection_type {
            ConnectionType::USB => {
                debug!("Printing via USB to: {}", printer.address);
                self.print_usb(&printer.address, &commands).await
            }
            ConnectionType::Network => {
                debug!("Printing via Network to: {}", printer.address);
                self.print_network(&printer.address, &commands).await
            }
            ConnectionType::Bluetooth => {
                debug!("Printing via Bluetooth to: {}", printer.address);
                self.print_bluetooth(&printer.address, &commands).await
            }
        };

        match &result {
            Ok(_) => info!("Test print completed successfully for printer: {}", printer_id),
            Err(e) => error!("Test print failed for printer {}: {}", printer_id, e),
        }

        result
    }

    /// Print via USB
    async fn print_usb(&self, address: &str, data: &[u8]) -> Result<()> {
        // Parse device path: /dev/bus/usb/001/002
        let parts: Vec<&str> = address.split('/').collect();
        if parts.len() < 6 {
            return Err(DaemonError::PrintJob("Invalid USB address".to_string()));
        }

        let bus = parts[4].parse::<u8>()
            .map_err(|_| DaemonError::PrintJob("Invalid bus number".to_string()))?;
        let addr = parts[5].parse::<u8>()
            .map_err(|_| DaemonError::PrintJob("Invalid device address".to_string()))?;

        // Find device
        for device in self.usb_context.devices()?.iter() {
            if device.bus_number() == bus && device.address() == addr {
                let mut handle = device.open()?;

                // Claim interface 0 (standard for printers)
                handle.claim_interface(0)?;

                // Write data to OUT endpoint (typically 0x01 or 0x02)
                let timeout = Duration::from_secs(5);
                handle.write_bulk(0x01, data, timeout)?;

                handle.release_interface(0)?;
                return Ok(());
            }
        }

        Err(DaemonError::PrinterNotFound(address.to_string()))
    }

    /// Print via network (raw TCP port 9100)
    async fn print_network(&self, address: &str, data: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        let mut stream = TcpStream::connect(address)
            .await
            .map_err(|e| DaemonError::Network(e.to_string()))?;

        stream
            .write_all(data)
            .await
            .map_err(|e| DaemonError::Network(e.to_string()))?;

        stream
            .flush()
            .await
            .map_err(|e| DaemonError::Network(e.to_string()))?;

        Ok(())
    }

    /// Print via Bluetooth
    async fn print_bluetooth(&self, address: &str, data: &[u8]) -> Result<()> {
        // TODO: Implement Bluetooth BLE printing using btleplug (Task #14)
        warn!("Bluetooth printing not yet implemented for address: {}", address);
        Err(DaemonError::Bluetooth("Not implemented".to_string()))
    }

    /// Add printer to managed list
    pub async fn add_printer(&self, config: PrinterConfig) {
        let mut printers = self.printers.lock().await;
        printers.insert(config.id.clone(), config);
    }

    /// Remove printer from managed list
    pub async fn remove_printer(&self, printer_id: &str) {
        let mut printers = self.printers.lock().await;
        printers.remove(printer_id);
    }

    /// Get printer by ID
    pub async fn get_printer(&self, printer_id: &str) -> Option<PrinterConfig> {
        let printers = self.printers.lock().await;
        printers.get(printer_id).cloned()
    }

    /// Check if printer is online (with 30-second cache)
    pub async fn is_online(&self, printer_id: &str) -> bool {
        use std::time::Instant;

        // Check cache first (30-second TTL)
        {
            let cache = self.online_cache.lock().await;
            if let Some((is_online, cached_at)) = cache.get(printer_id) {
                if cached_at.elapsed() < Duration::from_secs(30) {
                    debug!("Using cached online status for printer: {} = {}", printer_id, is_online);
                    return *is_online;
                }
            }
        }

        // Cache miss or expired, perform discovery
        debug!("Checking online status for printer: {}", printer_id);
        let is_online = if let Ok(discovered) = self.discover_all().await {
            discovered.iter().any(|p| {
                p.get("id")
                    .and_then(|id| id.as_str())
                    .map_or(false, |id| id == printer_id)
            })
        } else {
            error!("Failed to discover printers during online check");
            false
        };

        // Update cache
        {
            let mut cache = self.online_cache.lock().await;
            cache.insert(printer_id.to_string(), (is_online, Instant::now()));
        }

        debug!("Printer {} online status: {}", printer_id, is_online);
        is_online
    }
}
