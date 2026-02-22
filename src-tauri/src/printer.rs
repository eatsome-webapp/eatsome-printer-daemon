use crate::config::{ConnectionType, PrinterConfig};
use crate::discovery::{self, DiscoveredPrinter};
use crate::errors::{DaemonError, Result};
use crate::escpos::{build_full_status_request, format_kitchen_receipt, format_test_print, PaperWidth};
use crate::queue::PrintJob;
use crate::status::PrinterHwStatus;
use rusb::{Context, Device, DeviceDescriptor, UsbContext};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// A persistent TCP connection to a network printer.
struct NetworkConnection {
    stream: TcpStream,
    address: String,
    connected_at: Instant,
    last_used: Instant,
    consecutive_failures: u32,
}

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
    online_cache: Arc<Mutex<HashMap<String, (bool, Instant)>>>,
    discovery_cache: Arc<Mutex<(Vec<serde_json::Value>, Option<Instant>)>>,
    /// Persistent TCP connection pool: address → NetworkConnection
    network_pool: Arc<Mutex<HashMap<String, NetworkConnection>>>,
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
            network_pool: Arc::new(Mutex::new(HashMap::new())),
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

    /// Print via network (raw TCP port 9100) with persistent connection pool.
    ///
    /// Connection pool strategy:
    /// 1. Check pool for existing connection to this address
    /// 2. If found: attempt write (reuse connection)
    /// 3. If write fails: remove from pool, create new connection, retry once
    /// 4. If not found: create new connection, add to pool after successful write
    ///
    /// Timeouts: Connect 5s, Write 20s, Flush 5s
    async fn print_network(&self, address: &str, data: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        // Try to reuse a pooled connection
        let mut pooled_stream = {
            let mut pool = self.network_pool.lock().await;
            pool.remove(address)
        };

        if let Some(mut conn) = pooled_stream.take() {
            debug!("Reusing pooled connection to {} (age: {:?})", address, conn.connected_at.elapsed());

            // Attempt write on existing connection
            let write_result = tokio::time::timeout(
                Duration::from_secs(20),
                conn.stream.write_all(data),
            ).await;

            match write_result {
                Ok(Ok(())) => {
                    // Flush
                    let flush_result = tokio::time::timeout(
                        Duration::from_secs(5),
                        conn.stream.flush(),
                    ).await;

                    match flush_result {
                        Ok(Ok(())) => {
                            // Success — return connection to pool
                            conn.last_used = Instant::now();
                            conn.consecutive_failures = 0;
                            let mut pool = self.network_pool.lock().await;
                            pool.insert(address.to_string(), conn);
                            return Ok(());
                        }
                        _ => {
                            debug!("Flush failed on pooled connection to {}, reconnecting", address);
                            // Fall through to create new connection
                        }
                    }
                }
                _ => {
                    debug!("Write failed on pooled connection to {}, reconnecting", address);
                    // Fall through to create new connection
                }
            }
        }

        // Create new connection (either no pooled connection or reuse failed)
        let mut stream = tokio::time::timeout(
            Duration::from_secs(5),
            TcpStream::connect(address),
        )
        .await
        .map_err(|_| DaemonError::Network(format!("Connection timed out to {}", address)))?
        .map_err(|e| DaemonError::Network(e.to_string()))?;

        // Set TCP keepalive on new connections
        Self::set_tcp_keepalive(&stream);

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

        // Add to pool after successful write
        let now = Instant::now();
        let conn = NetworkConnection {
            stream,
            address: address.to_string(),
            connected_at: now,
            last_used: now,
            consecutive_failures: 0,
        };
        let mut pool = self.network_pool.lock().await;
        pool.insert(address.to_string(), conn);
        debug!("Added new connection to pool for {} (pool size: {})", address, pool.len());

        Ok(())
    }

    /// Configure TCP keepalive on a tokio TcpStream to detect dead connections.
    /// Keepalive: idle 30s, interval 10s. Uses socket2 via raw fd/socket.
    #[cfg(unix)]
    fn set_tcp_keepalive(stream: &TcpStream) {
        use std::os::unix::io::{AsRawFd, FromRawFd};

        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(30))
            .with_interval(Duration::from_secs(10));

        // Borrow the raw fd without taking ownership
        let fd = stream.as_raw_fd();
        // Safety: we use from_raw_fd + forget to avoid double-close
        let socket = unsafe { socket2::Socket::from_raw_fd(fd) };

        if let Err(e) = socket.set_tcp_keepalive(&keepalive) {
            debug!("Failed to set TCP keepalive: {} (non-fatal)", e);
        }

        // Don't drop — tokio still owns the fd
        std::mem::forget(socket);
    }

    /// Configure TCP keepalive (Windows variant)
    #[cfg(windows)]
    fn set_tcp_keepalive(stream: &TcpStream) {
        use std::os::windows::io::{AsRawSocket, FromRawSocket};

        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(30))
            .with_interval(Duration::from_secs(10));

        let raw = stream.as_raw_socket();
        let socket = unsafe { socket2::Socket::from_raw_socket(raw) };

        if let Err(e) = socket.set_tcp_keepalive(&keepalive) {
            debug!("Failed to set TCP keepalive: {} (non-fatal)", e);
        }

        std::mem::forget(socket);
    }

    /// Remove stale connections from the pool (idle > max_idle_secs).
    /// Called by background health checker in main.rs.
    /// Returns `(stale_removed, active_remaining)` for telemetry.
    pub async fn cleanup_stale_connections(&self, max_idle_secs: u64) -> (usize, usize) {
        let mut pool = self.network_pool.lock().await;
        let before = pool.len();
        pool.retain(|addr, conn| {
            let idle = conn.last_used.elapsed().as_secs() > max_idle_secs;
            if idle {
                debug!("Removing stale connection to {} (idle {:?})", addr, conn.last_used.elapsed());
            }
            !idle
        });
        let removed = before - pool.len();
        let active = pool.len();
        if removed > 0 {
            info!("Cleaned up {} stale connections (pool: {} → {})", removed, before, active);
        }
        (removed, active)
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

        // 6. Write data in chunks with adaptive sizing
        // Start with 100-byte chunks (5x throughput vs 20B), fallback to 20B on error
        let mut chunk_size: usize = 100;
        let mut offset = 0;

        while offset < data.len() {
            let end = std::cmp::min(offset + chunk_size, data.len());
            let chunk = &data[offset..end];

            let write_result = tokio::time::timeout(
                Duration::from_secs(5),
                peripheral.write(&write_char, chunk, write_type),
            )
            .await;

            match write_result {
                Ok(Ok(_)) => {
                    offset = end;
                }
                Ok(Err(e)) if chunk_size > 20 => {
                    // Adaptive fallback: retry this chunk with smaller size
                    warn!("BLE write failed with {}B chunks, falling back to 20B: {}", chunk_size, e);
                    chunk_size = 20;
                    continue; // Retry same offset with smaller chunk
                }
                Ok(Err(e)) => {
                    let _ = peripheral.disconnect().await;
                    return Err(DaemonError::Bluetooth(format!("Write failed: {}", e)));
                }
                Err(_) => {
                    let _ = peripheral.disconnect().await;
                    return Err(DaemonError::Bluetooth("Write chunk timed out".to_string()));
                }
            }

            // Small inter-chunk delay to avoid overwhelming the BLE stack
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let chunks_sent = (data.len() + chunk_size - 1) / chunk_size;
        info!("BLE print complete: {} bytes sent in ~{} chunks ({}B each)", data.len(), chunks_sent, chunk_size);

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

    // =========================================================================
    // DLE EOT Real-time Status Polling
    // =========================================================================

    /// Poll printer hardware status via DLE EOT commands.
    /// Returns structured status for network and USB printers.
    /// BLE printers return a healthy default (DLE EOT not reliably supported over BLE).
    pub async fn poll_status(&self, printer: &PrinterConfig) -> Result<PrinterHwStatus> {
        match printer.connection_type {
            ConnectionType::Network => self.poll_status_network(&printer.address).await,
            ConnectionType::USB => {
                // USB I/O is synchronous (rusb) — run on blocking thread pool
                // to avoid stalling the tokio async runtime
                let usb_ctx = self.usb_context.clone();
                let address = printer.address.clone();
                tokio::task::spawn_blocking(move || {
                    poll_status_usb_blocking(&usb_ctx, &address)
                })
                .await
                .map_err(|e| DaemonError::Other(anyhow::anyhow!("USB poll task failed: {}", e)))?
            }
            ConnectionType::Bluetooth => {
                debug!("Skipping DLE EOT status poll for BLE printer {}", printer.id);
                Ok(PrinterHwStatus::healthy())
            }
        }
    }

    /// Poll status via TCP: send all 4 DLE EOT requests, read 4-byte response.
    /// Reuses persistent connection pool when available; falls back to ephemeral connection.
    async fn poll_status_network(&self, address: &str) -> Result<PrinterHwStatus> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let request = build_full_status_request();

        // Try to reuse a pooled connection first
        let mut pooled_conn = {
            let mut pool = self.network_pool.lock().await;
            pool.remove(address)
        };

        if let Some(mut conn) = pooled_conn.take() {
            debug!("Status poll reusing pooled connection to {}", address);

            let poll_result = async {
                tokio::time::timeout(Duration::from_secs(2), conn.stream.write_all(&request))
                    .await
                    .map_err(|_| DaemonError::Network(format!("Status poll write timed out to {}", address)))?
                    .map_err(|e| DaemonError::Network(e.to_string()))?;

                let mut response = [0u8; 4];
                tokio::time::timeout(Duration::from_secs(2), conn.stream.read_exact(&mut response))
                    .await
                    .map_err(|_| DaemonError::Network(format!("Status poll read timed out from {}", address)))?
                    .map_err(|e| DaemonError::Network(e.to_string()))?;

                Ok::<_, DaemonError>(response)
            }.await;

            match poll_result {
                Ok(response) => {
                    // Success — return connection to pool with updated timestamp
                    conn.last_used = Instant::now();
                    let mut pool = self.network_pool.lock().await;
                    pool.insert(address.to_string(), conn);
                    return Ok(PrinterHwStatus::from_dle_eot(
                        response[0], response[1], response[2], response[3],
                    ));
                }
                Err(e) => {
                    // Stale connection — drop it, fall through to ephemeral
                    debug!("Pooled connection to {} failed during status poll, using ephemeral: {}", address, e);
                }
            }
        }

        // No pooled connection or pooled failed — create ephemeral (don't pool status-only connections)
        let mut stream = tokio::time::timeout(
            Duration::from_secs(2),
            TcpStream::connect(address),
        )
        .await
        .map_err(|_| DaemonError::Network(format!("Status poll connect timed out to {}", address)))?
        .map_err(|e| DaemonError::Network(format!("Status poll connect failed to {}: {}", address, e)))?;

        tokio::time::timeout(Duration::from_secs(2), stream.write_all(&request))
            .await
            .map_err(|_| DaemonError::Network(format!("Status poll write timed out to {}", address)))?
            .map_err(|e| DaemonError::Network(e.to_string()))?;

        let mut response = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut response))
            .await
            .map_err(|_| DaemonError::Network(format!("Status poll read timed out from {}", address)))?
            .map_err(|e| DaemonError::Network(format!("Status poll read failed from {}: {}", address, e)))?;

        Ok(PrinterHwStatus::from_dle_eot(
            response[0], response[1], response[2], response[3],
        ))
    }

    /// Get a snapshot of all configured printers (for status polling)
    pub async fn get_all_printers(&self) -> Vec<PrinterConfig> {
        let printers = self.printers.lock().await;
        printers.values().cloned().collect()
    }
}

/// Poll printer status via USB (standalone, runs on blocking thread pool).
/// Extracted from PrinterManager so it can be called from spawn_blocking.
fn poll_status_usb_blocking(usb_context: &Context, address: &str) -> Result<PrinterHwStatus> {
    let request = build_full_status_request();

    // Parse vendor:product from address (e.g., "usb_04b8_0e15")
    let parts: Vec<&str> = address.split('_').collect();
    if parts.len() < 3 {
        return Err(DaemonError::PrinterNotFound(format!(
            "Invalid USB address format for status poll: {}", address
        )));
    }

    let vendor_id = u16::from_str_radix(parts[1], 16)
        .map_err(|_| DaemonError::PrinterNotFound(format!("Invalid vendor ID: {}", parts[1])))?;
    let product_id = u16::from_str_radix(parts[2], 16)
        .map_err(|_| DaemonError::PrinterNotFound(format!("Invalid product ID: {}", parts[2])))?;

    let devices = usb_context.devices()
        .map_err(DaemonError::Usb)?;

    for device in devices.iter() {
        if let Ok(desc) = device.device_descriptor() {
            if desc.vendor_id() == vendor_id && desc.product_id() == product_id {
                let handle = device.open()
                    .map_err(DaemonError::Usb)?;

                // Find bulk OUT and IN endpoints
                let config = device.active_config_descriptor()
                    .map_err(DaemonError::Usb)?;

                let mut out_ep = None;
                let mut in_ep = None;

                for interface in config.interfaces() {
                    for iface_desc in interface.descriptors() {
                        for ep in iface_desc.endpoint_descriptors() {
                            match ep.direction() {
                                rusb::Direction::Out if out_ep.is_none() => {
                                    out_ep = Some(ep.address());
                                }
                                rusb::Direction::In if in_ep.is_none() => {
                                    in_ep = Some(ep.address());
                                }
                                _ => {}
                            }
                        }
                    }
                }

                let out_ep = out_ep.ok_or_else(|| {
                    DaemonError::PrintJob("No USB OUT endpoint found for status poll".to_string())
                })?;
                let in_ep = in_ep.ok_or_else(|| {
                    DaemonError::PrintJob("No USB IN endpoint found for status poll".to_string())
                })?;

                // Claim interface 0
                let _ = handle.set_auto_detach_kernel_driver(true);
                handle.claim_interface(0)
                    .map_err(DaemonError::Usb)?;

                // Write DLE EOT requests
                handle.write_bulk(out_ep, &request, Duration::from_secs(2))
                    .map_err(DaemonError::Usb)?;

                // Read response
                let mut response = [0u8; 4];
                handle.read_bulk(in_ep, &mut response, Duration::from_secs(2))
                    .map_err(DaemonError::Usb)?;

                handle.release_interface(0)
                    .map_err(DaemonError::Usb)?;

                return Ok(PrinterHwStatus::from_dle_eot(
                    response[0],
                    response[1],
                    response[2],
                    response[3],
                ));
            }
        }
    }

    Err(DaemonError::PrinterNotFound(format!(
        "USB device not found for status poll: {}", address
    )))
}
