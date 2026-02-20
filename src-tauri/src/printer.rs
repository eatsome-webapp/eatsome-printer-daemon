use crate::config::{ConnectionType, PrinterConfig};
use crate::discovery::{self, DiscoveredPrinter};
use crate::errors::{DaemonError, Result};
use crate::escpos::{format_kitchen_receipt, format_test_print, PaperWidth};
use crate::queue::PrintJob;
use rusb::{Context, Device, DeviceDescriptor, UsbContext};
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

/// Cache TTL for discovery results (seconds)
const DISCOVERY_CACHE_TTL_SECS: u64 = 30;

pub struct PrinterManager {
    printers: Arc<Mutex<HashMap<String, PrinterConfig>>>,
    usb_context: Context,
    online_cache: Arc<Mutex<HashMap<String, (bool, std::time::Instant)>>>,
    discovery_cache: Arc<Mutex<(Vec<serde_json::Value>, Option<std::time::Instant>)>>,
}

impl PrinterManager {
    pub fn new() -> Result<Self> {
        info!("Initializing PrinterManager");
        let usb_context = Context::new().map_err(|e| {
            error!("Failed to initialize USB context: {}", e);
            DaemonError::Usb(e)
        })?;
        Ok(Self {
            printers: Arc::new(Mutex::new(HashMap::new())),
            usb_context,
            online_cache: Arc::new(Mutex::new(HashMap::new())),
            discovery_cache: Arc::new(Mutex::new((Vec::new(), None))),
        })
    }

    /// Discover all printers (USB + Network + Bluetooth) with caching
    ///
    /// Returns cached results if the last scan was within the TTL window (30s).
    /// This prevents redundant full-network scans during the setup wizard flow
    /// where discovery may be triggered multiple times.
    #[tracing::instrument(skip(self))]
    pub async fn discover_all(&self, force: bool) -> Result<Vec<serde_json::Value>> {
        // Check cache first (skip if force=true)
        if !force {
            let cache = self.discovery_cache.lock().await;
            if let Some(last_scan) = cache.1 {
                if last_scan.elapsed() < Duration::from_secs(DISCOVERY_CACHE_TTL_SECS) {
                    info!(
                        "Returning {} cached discovery results (age: {:.1}s)",
                        cache.0.len(),
                        last_scan.elapsed().as_secs_f32()
                    );
                    return Ok(cache.0.clone());
                }
            }
        }

        debug!("Starting comprehensive printer discovery");
        let mut discovered = Vec::new();

        // Discover USB printers
        let usb_printers = self.discover_usb()?;
        info!("Discovered {} USB printers", usb_printers.len());
        discovered.extend(
            usb_printers
                .into_iter()
                .filter_map(|p| serde_json::to_value(p).ok()),
        );

        // Discover network printers via comprehensive multi-method discovery
        let subnet = discovery::detect_local_subnet();
        info!("Auto-detected subnet for scanning: {}", subnet);

        match discovery::discover_all_printers(&subnet).await {
            Ok(network_printers) => {
                info!("Discovered {} network/bluetooth printers via comprehensive scan", network_printers.len());
                discovered.extend(
                    network_printers
                        .into_iter()
                        .filter_map(|p| serde_json::to_value(p).ok()),
                );
            }
            Err(e) => {
                warn!("Comprehensive network discovery failed: {}", e);
            }
        }

        info!("Total printers discovered: {}", discovered.len());

        // Update cache
        {
            let mut cache = self.discovery_cache.lock().await;
            cache.0 = discovered.clone();
            cache.1 = Some(std::time::Instant::now());
        }

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
                    protocol: "escpos".to_string(), // Known vendor IDs = ESC/POS
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
    #[tracing::instrument(skip(self), fields(printer_id))]
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

    /// Test print directly to an address without requiring printer to be registered
    pub async fn test_print_direct(&self, address: &str, connection_type: &str) -> Result<()> {
        info!("Direct test print requested for: {} ({})", address, connection_type);

        let commands = format_test_print(PaperWidth::Width80mm);
        debug!("Generated test print commands: {} bytes", commands.len());

        let result = match connection_type {
            "usb" => {
                debug!("Printing via USB to: {}", address);
                self.print_usb(address, &commands).await
            }
            "network" => {
                debug!("Printing via Network to: {}", address);
                self.print_network(address, &commands).await
            }
            "bluetooth" => {
                debug!("Printing via Bluetooth to: {}", address);
                self.print_bluetooth(address, &commands).await
            }
            _ => {
                error!("Unknown connection type: {}", connection_type);
                Err(DaemonError::PrintJob(format!("Unknown connection type: {}", connection_type)))
            }
        };

        match &result {
            Ok(_) => info!("Direct test print completed successfully for: {}", address),
            Err(e) => error!("Direct test print failed for {}: {}", address, e),
        }

        result
    }

    /// Print a job to a specific printer
    ///
    /// Generates ESC/POS kitchen receipt from the job's items and sends to the printer.
    #[tracing::instrument(skip(self, job), fields(printer_id, job_id = %job.id, order = %job.order_number))]
    pub async fn print_to_printer(&self, printer_id: &str, job: &PrintJob) -> Result<()> {
        info!("Printing job {} to printer {}", job.id, printer_id);

        let printers = self.printers.lock().await;
        let printer = printers
            .get(printer_id)
            .ok_or_else(|| DaemonError::PrinterNotFound(printer_id.to_string()))?;

        let commands = format_kitchen_receipt(
            &job.station,
            &job.order_number,
            job.order_type.as_deref(),
            job.table_number.as_deref(),
            job.customer_name.as_deref(),
            job.priority,
            &job.items,
            job.timestamp,
            PaperWidth::Width80mm,
        );

        match printer.connection_type {
            ConnectionType::USB => self.print_usb(&printer.address, &commands).await,
            ConnectionType::Network => self.print_network(&printer.address, &commands).await,
            ConnectionType::Bluetooth => self.print_bluetooth(&printer.address, &commands).await,
        }
    }

    /// Print via USB
    ///
    /// Handles macOS-specific USB permission errors with user-friendly messages.
    /// On macOS, USB access requires entitlements in the app bundle.
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
                let handle = device.open().map_err(|e| {
                    // Provide user-friendly error for permission issues
                    if e == rusb::Error::Access {
                        warn!("USB access denied for device at {}. On macOS, ensure the app has USB entitlements.", address);
                        DaemonError::PrintJob(format!(
                            "USB permission denied for {}. Please grant USB access in System Settings > Privacy & Security.",
                            address
                        ))
                    } else {
                        DaemonError::Usb(e)
                    }
                })?;

                // Claim interface 0 (standard for printers)
                handle.claim_interface(0).map_err(|e| {
                    if e == rusb::Error::Access || e == rusb::Error::Busy {
                        warn!("Cannot claim USB interface: {} (another driver may be active)", e);
                        DaemonError::PrintJob(format!(
                            "USB interface busy or locked: {}. Close any other printer software and retry.",
                            e
                        ))
                    } else {
                        DaemonError::Usb(e)
                    }
                })?;

                // Write data to OUT endpoint (typically 0x01 or 0x02)
                let timeout = Duration::from_secs(5);
                if let Err(e) = handle.write_bulk(0x01, data, timeout) {
                    handle.release_interface(0).ok();
                    return Err(DaemonError::PrintJob(format!("USB write failed: {}", e)));
                }

                handle.release_interface(0).ok();
                return Ok(());
            }
        }

        Err(DaemonError::PrinterNotFound(address.to_string()))
    }

    /// Print via network (raw TCP port 9100) with timeouts
    ///
    /// Applies per-operation timeouts to prevent hanging when the network drops:
    /// - Connect: 5 seconds
    /// - Write: 20 seconds (large receipts may take time)
    /// - Flush: 5 seconds
    async fn print_network(&self, address: &str, data: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpStream;

        // Connect with 5s timeout
        let mut stream = tokio::time::timeout(
            Duration::from_secs(5),
            TcpStream::connect(address),
        )
        .await
        .map_err(|_| DaemonError::Network(format!("Connection timed out to {}", address)))?
        .map_err(|e| DaemonError::Network(e.to_string()))?;

        // Write with 20s timeout
        tokio::time::timeout(
            Duration::from_secs(20),
            stream.write_all(data),
        )
        .await
        .map_err(|_| DaemonError::Network(format!("Write timed out to {} ({} bytes)", address, data.len())))?
        .map_err(|e| DaemonError::Network(e.to_string()))?;

        // Flush with 5s timeout
        tokio::time::timeout(
            Duration::from_secs(5),
            stream.flush(),
        )
        .await
        .map_err(|_| DaemonError::Network(format!("Flush timed out to {}", address)))?
        .map_err(|e| DaemonError::Network(e.to_string()))?;

        Ok(())
    }

    /// Print via Bluetooth BLE
    ///
    /// Discovers the BLE peripheral by address, connects, finds a writable
    /// GATT characteristic, and sends data in 20-byte chunks (safe BLE MTU minimum).
    ///
    /// Known printer service/characteristic UUIDs are tried first (Star Micronics,
    /// generic BLE printer). Falls back to first characteristic with WRITE_WITHOUT_RESPONSE
    /// or WRITE property.
    async fn print_bluetooth(&self, address: &str, data: &[u8]) -> Result<()> {
        use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
        use btleplug::platform::Manager;
        use uuid::Uuid;

        // Known BLE printer GATT characteristic UUIDs
        const GENERIC_WRITE: Uuid = Uuid::from_u128(0x00002AF1_0000_1000_8000_00805F9B34FB);
        const STAR_SERVICE: Uuid = Uuid::from_u128(0x49535343_FE7D_4AE5_8FA9_9FAFD205E455);
        const STAR_WRITE: Uuid = Uuid::from_u128(0x49535343_8841_43F4_A8D4_ECBE34729BB3);

        info!("BLE print requested for address: {} ({} bytes)", address, data.len());

        // 1. Get BLE manager and adapter
        let manager = Manager::new()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to create BLE manager: {}", e)))?;

        let adapters = manager.adapters()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to get BLE adapters: {}", e)))?;

        let adapter = adapters
            .first()
            .ok_or_else(|| DaemonError::Bluetooth("No Bluetooth adapters found".to_string()))?;

        // 2. Brief scan to ensure peripheral is discoverable (macOS CoreBluetooth needs this)
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to start BLE scan: {}", e)))?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        adapter.stop_scan().await.ok(); // best-effort stop

        // 3. Find peripheral by address
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to list peripherals: {}", e)))?;

        let peripheral = {
            let mut found = None;
            for p in &peripherals {
                if let Ok(Some(props)) = p.properties().await {
                    if props.address.to_string() == address {
                        found = Some(p);
                        break;
                    }
                }
            }
            found.ok_or_else(|| {
                DaemonError::Bluetooth(format!("Peripheral not found: {}", address))
            })?
        };

        // 4. Connect with timeout
        tokio::time::timeout(Duration::from_secs(10), peripheral.connect())
            .await
            .map_err(|_| DaemonError::Bluetooth(format!("Connection timed out to {}", address)))?
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to connect: {}", e)))?;

        info!("Connected to BLE peripheral: {}", address);

        // 5. Discover services and find writable characteristic
        peripheral
            .discover_services()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Service discovery failed: {}", e)))?;

        let characteristics = peripheral.characteristics();

        // Try known UUIDs first, then fallback to any writable characteristic
        let write_char = characteristics
            .iter()
            .find(|c| c.uuid == STAR_WRITE || c.uuid == GENERIC_WRITE)
            .or_else(|| {
                // Check for Star service membership
                characteristics.iter().find(|c| {
                    c.service_uuid == STAR_SERVICE
                        && c.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
                })
            })
            .or_else(|| {
                characteristics
                    .iter()
                    .find(|c| c.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE))
            })
            .or_else(|| {
                characteristics
                    .iter()
                    .find(|c| c.properties.contains(CharPropFlags::WRITE))
            })
            .cloned();

        let write_char = match write_char {
            Some(c) => c,
            None => {
                let _ = peripheral.disconnect().await;
                return Err(DaemonError::Bluetooth(
                    "No writable characteristic found on printer".to_string(),
                ));
            }
        };

        let write_type = if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            WriteType::WithResponse
        };

        info!(
            "Using BLE characteristic {} (service: {}, type: {:?})",
            write_char.uuid, write_char.service_uuid, write_type
        );

        // 6. Write data in 20-byte chunks (safe BLE MTU minimum)
        const BLE_CHUNK_SIZE: usize = 20;
        for chunk in data.chunks(BLE_CHUNK_SIZE) {
            tokio::time::timeout(
                Duration::from_secs(5),
                peripheral.write(&write_char, chunk, write_type),
            )
            .await
            .map_err(|_| DaemonError::Bluetooth("Write chunk timed out".to_string()))?
            .map_err(|e| DaemonError::Bluetooth(format!("Write failed: {}", e)))?;

            // Small inter-chunk delay to avoid overwhelming the BLE stack
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        info!("BLE print complete: {} bytes sent in {} chunks", data.len(), (data.len() + BLE_CHUNK_SIZE - 1) / BLE_CHUNK_SIZE);

        // 7. Disconnect (best-effort)
        if let Err(e) = peripheral.disconnect().await {
            warn!("Failed to disconnect from BLE peripheral: {}", e);
        }

        Ok(())
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
    #[allow(dead_code)] // Public API for future callers
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
        let is_online = if let Ok(discovered) = self.discover_all(false).await {
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
