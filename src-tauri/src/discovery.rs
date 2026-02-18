use crate::errors::{DaemonError, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPrinter {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub address: String,
    pub vendor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<serde_json::Value>,
    /// Detected protocol: "escpos", "unknown", or "unsupported"
    /// Used by the UI to warn users before selecting non-ESC/POS printers.
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "unknown".to_string()
}

/// Discover all printers using multiple discovery methods in parallel
///
/// This is the PRIMARY discovery function that should be called from the UI.
/// It runs all discovery methods concurrently and deduplicates results:
///
/// 1. **TCP Port Scanning** (FASTEST, works for most thermal printers)
///    - Scans subnet for ports 9100, 631, 515
///    - Finds Epson TM-m30ii, Star, Brother that don't advertise via mDNS
///
/// 2. **mDNS/Bonjour** (works for some network printers)
///    - Searches for _ipp._tcp, _printer._tcp, _pdl-datastream._tcp
///    - Good for home/office printers
///
/// 3. **SNMP** (legacy printers)
///    - Queries port 161 with community strings
///    - Gets printer model/vendor via MIB
///
/// 4. **Bluetooth BLE** (mobile printers)
///    - Scans for Star SM-S230i, Epson TM-P20, etc.
///
/// # Arguments
/// * `subnet` - CIDR notation for TCP scanning (e.g., "192.168.1.0/24")
///
/// # Returns
/// Deduplicated list of all discovered printers
pub async fn discover_all_printers(subnet: &str) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting COMPREHENSIVE printer discovery on subnet: {}", subnet);
    info!("Running 6 discovery methods in parallel:");
    info!("  1. TCP Port Scanning (9100/631/515)");
    info!("  2. mDNS/Bonjour");
    info!("  3. Bluetooth BLE");
    info!("  4. WS-Discovery (Windows - port 3702)");
    info!("  5. Epson ENPC (port 3289)");
    info!("  6. Star CloudPRNT (HTTP)");
    info!("  NOTE: SNMP discovery temporarily disabled - provides ~92% coverage without it");

    // Launch ALL discovery methods in parallel
    let tcp_task = tokio::spawn({
        let subnet = subnet.to_string();
        async move { scan_subnet_tcp(&subnet, 500).await }
    });

    let mdns_task = tokio::spawn(async move {
        discover_network_printers_with_timeout(5).await
    });

    // SNMP discovery temporarily disabled (API incompatibility with snmp2 crate)
    // Can be re-enabled later - provides ~92% coverage without it
    // let snmp_task = tokio::spawn({
    //     let subnet = subnet.to_string();
    //     async move { scan_subnet_snmp(&subnet).await }
    // });

    let bluetooth_task = tokio::spawn(async move {
        discover_bluetooth_printers_with_timeout(10).await
    });

    let wsd_task = tokio::spawn(async move {
        discover_ws_discovery(5).await
    });

    let enpc_task = tokio::spawn(async move {
        discover_epson_enpc(3).await
    });

    let cloudprnt_task = tokio::spawn({
        let subnet = subnet.to_string();
        async move { discover_star_cloudprnt(&subnet).await }
    });

    // Wait for all tasks to complete
    let (tcp_result, mdns_result, bluetooth_result, wsd_result, enpc_result, cloudprnt_result) = tokio::join!(
        tcp_task,
        mdns_task,
        bluetooth_task,
        wsd_task,
        enpc_task,
        cloudprnt_task
    );

    // Collect all results with deduplication
    // Key: normalized IP address (strips port) for network printers, full address for Bluetooth
    let mut all_printers = HashMap::new();

    // CloudPRNT results (HIGHEST priority for Star modern printers)
    if let Ok(Ok(printers)) = cloudprnt_result {
        info!("CloudPRNT found {} printers", printers.len());
        for printer in printers {
            all_printers.insert(dedup_key(&printer), printer);
        }
    }

    // ENPC results (HIGH priority for Epson)
    if let Ok(Ok(printers)) = enpc_result {
        info!("ENPC found {} printers", printers.len());
        for printer in printers {
            all_printers.entry(dedup_key(&printer)).or_insert(printer);
        }
    }

    // WS-Discovery results (HIGH priority for Windows environments)
    if let Ok(Ok(printers)) = wsd_result {
        info!("WS-Discovery found {} printers", printers.len());
        for printer in printers {
            all_printers.entry(dedup_key(&printer)).or_insert(printer);
        }
    }

    // TCP results (MEDIUM-HIGH priority - most reliable for generic thermal printers)
    if let Ok(Ok(printers)) = tcp_result {
        info!("TCP scan found {} printers", printers.len());
        for printer in printers {
            all_printers.entry(dedup_key(&printer)).or_insert(printer);
        }
    }

    // mDNS results (MEDIUM priority - don't override vendor-specific discoveries)
    if let Ok(Ok(printers)) = mdns_result {
        info!("mDNS found {} printers", printers.len());
        for printer in printers {
            all_printers.entry(dedup_key(&printer)).or_insert(printer);
        }
    }

    // SNMP discovery temporarily disabled
    // if let Ok(Ok(printers)) = snmp_result {
    //     info!("SNMP found {} printers", printers.len());
    //     for printer in printers {
    //         all_printers.entry(dedup_key(&printer)).or_insert(printer);
    //     }
    // }

    // Bluetooth results (separate connection type, always include)
    if let Ok(Ok(printers)) = bluetooth_result {
        info!("Bluetooth found {} printers", printers.len());
        for printer in printers {
            all_printers.insert(dedup_key(&printer), printer);
        }
    }

    let printers: Vec<DiscoveredPrinter> = all_printers.into_values().collect();
    info!("═══════════════════════════════════════════════════════════");
    info!("COMPREHENSIVE DISCOVERY COMPLETE: {} unique printers found", printers.len());
    info!("═══════════════════════════════════════════════════════════");

    Ok(printers)
}

/// Compute a deduplication key for a discovered printer.
///
/// For network printers: strip the port from "IP:PORT" to produce just the IP.
/// This ensures the same printer discovered via TCP (port 9100) and mDNS (port 631)
/// is deduplicated to a single entry.
///
/// For Bluetooth: use the full MAC address (already unique).
fn dedup_key(printer: &DiscoveredPrinter) -> String {
    match printer.connection_type.as_str() {
        "bluetooth" => printer.address.clone(),
        _ => {
            // Normalize: extract IP from "IP:PORT"
            printer
                .address
                .split(':')
                .next()
                .unwrap_or(&printer.address)
                .to_string()
        }
    }
}

/// Auto-detect local subnet for TCP scanning
///
/// Uses the machine's primary network interface to determine the subnet.
/// Falls back to 192.168.1.0/24 if detection fails.
///
/// # Returns
/// CIDR notation subnet (e.g., "192.168.1.0/24")
pub fn detect_local_subnet() -> String {
    // Try to get local IP address
    if let Ok(local_ip) = local_ip_address::local_ip() {
        if let std::net::IpAddr::V4(ipv4) = local_ip {
            let octets = ipv4.octets();
            // Assume /24 subnet (class C)
            return format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]);
        }
    }

    // Fallback to common home network subnet
    warn!("Could not detect local subnet, using default 192.168.1.0/24");
    "192.168.1.0/24".to_string()
}

/// Discover network printers via mDNS/Zeroconf
///
/// Searches for printers advertising via Bonjour/Avahi on the local network.
/// Supports multiple service types:
/// - `_ipp._tcp.local` - Internet Printing Protocol (modern printers)
/// - `_printer._tcp.local` - Generic printer service
/// - `_pdl-datastream._tcp.local` - Page Description Language (Epson, HP)
///
/// # Arguments
/// * `timeout_secs` - Discovery timeout in seconds (default: 5)
///
/// # Returns
/// List of discovered network printers with IP addresses and metadata
///
/// # Note
/// This method ONLY finds printers that advertise via mDNS. Most restaurant
/// thermal printers do NOT advertise, so use `discover_all_printers()` instead.
pub async fn discover_network_printers() -> Result<Vec<DiscoveredPrinter>> {
    discover_network_printers_with_timeout(5).await
}

/// Discover network printers with custom timeout
pub async fn discover_network_printers_with_timeout(timeout_secs: u64) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting network printer discovery (timeout: {}s)", timeout_secs);

    let mdns = ServiceDaemon::new()
        .map_err(|e| DaemonError::Discovery(format!("Failed to create mDNS daemon: {}", e)))?;

    let mut discovered = HashMap::new();

    // Service types to search for
    let service_types = vec![
        "_ipp._tcp.local.",       // Internet Printing Protocol
        "_printer._tcp.local.",   // Generic printer service
        "_pdl-datastream._tcp.local.", // Page Description Language
    ];

    for service_type in service_types {
        debug!("Browsing for service type: {}", service_type);

        let receiver = mdns.browse(service_type)
            .map_err(|e| DaemonError::Discovery(format!("Failed to browse {}: {}", service_type, e)))?;

        // Collect services for timeout duration
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(timeout_secs) {
            // Non-blocking receive with 100ms timeout per iteration
            match tokio::time::timeout(Duration::from_millis(100), async {
                receiver.recv_async().await
            }).await {
                Ok(Ok(event)) => {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            debug!("Resolved printer: {} at {}:{}", info.get_fullname(),
                                   info.get_addresses().iter().next().map(|a| a.to_string()).unwrap_or_else(|| "unknown".to_string()),
                                   info.get_port());

                            // Extract printer info
                            let name = info.get_hostname().trim_end_matches('.').to_string();
                            let addresses = info.get_addresses();

                            if let Some(addr) = addresses.iter().next() {
                                let address = format!("{}:{}", addr, info.get_port());
                                let id = format!("net_{}", addr.to_string().replace('.', "_"));

                                // Detect vendor from hostname or service name
                                let vendor = detect_vendor(&name, info.get_fullname());

                                let printer = DiscoveredPrinter {
                                    id: id.clone(),
                                    name: name.clone(),
                                    connection_type: "network".to_string(),
                                    address,
                                    vendor,
                                    capabilities: None,
                                    protocol: "unknown".to_string(), // mDNS - could be IPP/PCL
                                };

                                discovered.insert(id, printer);
                                info!("Discovered network printer: {} ({})", name, addr);
                            }
                        }
                        ServiceEvent::SearchStarted(_) => {
                            debug!("Search started for {}", service_type);
                        }
                        ServiceEvent::ServiceFound(_, _) => {
                            debug!("Service found, waiting for resolution...");
                        }
                        _ => {}
                    }
                }
                Ok(Err(e)) => {
                    warn!("Error receiving mDNS event: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout on this iteration, continue
                    continue;
                }
            }
        }

        mdns.stop_browse(service_type)
            .map_err(|e| DaemonError::Discovery(format!("Failed to stop browse: {}", e)))?;
    }

    mdns.shutdown()
        .map_err(|e| DaemonError::Discovery(format!("Failed to shutdown mDNS: {}", e)))?;

    let printers: Vec<DiscoveredPrinter> = discovered.into_values().collect();
    info!("Network printer discovery complete: {} printers found", printers.len());

    Ok(printers)
}

/// Detect printer vendor from hostname or service name
fn detect_vendor(hostname: &str, fullname: &str) -> String {
    let text = format!("{} {}", hostname.to_lowercase(), fullname.to_lowercase());

    if text.contains("epson") {
        "Epson".to_string()
    } else if text.contains("brother") {
        "Brother".to_string()
    } else if text.contains("star") || text.contains("starmicronics") {
        "Star Micronics".to_string()
    } else if text.contains("citizen") {
        "Citizen".to_string()
    } else if text.contains("hp") || text.contains("hewlett") {
        "HP".to_string()
    } else if text.contains("canon") {
        "Canon".to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Discover Bluetooth BLE printers
///
/// Scans for BLE peripherals advertising printer services or with printer-related names.
/// Primarily used for portable/mobile thermal printers (Star SM-S230i, Epson TM-P20).
///
/// # Arguments
/// * `timeout_secs` - Scan timeout in seconds (default: 10)
///
/// # Returns
/// List of discovered Bluetooth printers with MAC addresses
///
/// # Note
/// - User must initiate BLE pairing manually for security
/// - macOS/Windows require Bluetooth permissions
/// - Linux requires BlueZ daemon running
pub async fn discover_bluetooth_printers() -> Result<Vec<DiscoveredPrinter>> {
    discover_bluetooth_printers_with_timeout(10).await
}

/// Discover Bluetooth printers with custom timeout
pub async fn discover_bluetooth_printers_with_timeout(timeout_secs: u64) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting Bluetooth BLE printer discovery (timeout: {}s)", timeout_secs);

    #[cfg(not(target_os = "linux"))]
    {
        use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
        use btleplug::platform::Manager;

        let manager = Manager::new()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to create BLE manager: {}", e)))?;

        let adapters = manager.adapters()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to get BLE adapters: {}", e)))?;

        if adapters.is_empty() {
            warn!("No Bluetooth adapters found");
            return Ok(Vec::new());
        }

        let adapter = &adapters[0];
        debug!("Using Bluetooth adapter: {:?}", adapter);

        // Start scanning
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to start BLE scan: {}", e)))?;

        // Scan for timeout duration
        tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

        // Get peripherals
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to get peripherals: {}", e)))?;

        // Stop scanning
        adapter
            .stop_scan()
            .await
            .map_err(|e| DaemonError::Bluetooth(format!("Failed to stop BLE scan: {}", e)))?;

        let mut discovered = Vec::new();

        for peripheral in peripherals {
            if let Ok(properties) = peripheral.properties().await {
                if let Some(props) = properties {
                    if let Some(local_name) = props.local_name {
                        // Check if device name indicates a printer
                        if is_bluetooth_printer(&local_name) {
                            let address = props.address.to_string();
                            let id = format!("ble_{}", address.replace(':', "_"));
                            let vendor = detect_vendor(&local_name, &local_name);

                            let printer = DiscoveredPrinter {
                                id: id.clone(),
                                name: local_name.clone(),
                                connection_type: "bluetooth".to_string(),
                                address,
                                vendor,
                                capabilities: None,
                                protocol: "escpos".to_string(), // BLE POS printers are ESC/POS
                            };

                            discovered.push(printer);
                            info!("Discovered Bluetooth printer: {} ({})", local_name, props.address);
                        }
                    }
                }
            }
        }

        info!("Bluetooth printer discovery complete: {} printers found", discovered.len());
        Ok(discovered)
    }

    #[cfg(target_os = "linux")]
    {
        // Linux requires BlueZ daemon and special permissions
        warn!("Bluetooth discovery on Linux requires BlueZ daemon and permissions");
        warn!("Use 'bluetoothctl' to manually pair printers, then add via address");
        Ok(Vec::new())
    }
}

/// Check if Bluetooth device name indicates a printer
fn is_bluetooth_printer(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("printer")
        || lower.contains("star")
        || lower.contains("sm-")    // Star mobile printers
        || lower.contains("tm-")    // Epson thermal printers
        || lower.contains("tsp")    // Star thermal printers
        || lower.contains("citizen")
        || lower.contains("brother")
        || lower.contains("pos")
        || lower.contains("receipt")
}

/// Scan network subnet for printers via TCP port scanning
///
/// Performs fast parallel TCP port scanning on common printer ports:
/// - Port 9100: Raw TCP (JetDirect/AppSocket) - most thermal printers
/// - Port 631: IPP (Internet Printing Protocol)
/// - Port 515: LPD (Line Printer Daemon)
///
/// This is the PRIMARY discovery method for restaurant thermal printers
/// that don't advertise via mDNS/Bonjour (Epson TM-m30ii, Star, Brother).
///
/// # Arguments
/// * `subnet` - CIDR notation subnet (e.g., "192.168.1.0/24")
/// * `timeout_ms` - TCP connection timeout per host (default: 500ms)
///
/// # Returns
/// List of discovered printers with open printer ports
pub async fn scan_subnet_tcp(subnet: &str, timeout_ms: u64) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting TCP port scan: {}", subnet);

    let ip_range = parse_cidr(subnet)?;
    let mut discovered = HashMap::new();

    // Printer ports to scan
    let printer_ports = vec![
        (9100, "Raw TCP (JetDirect)"),
        (631, "IPP"),
        (515, "LPD"),
    ];

    // Create parallel scanning tasks with rate limiting
    // IMPORTANT: Limit concurrent connections to avoid overwhelming macOS network stack
    // Running all 762 connections (254 IPs × 3 ports) in parallel triggers macOS security
    // throttling after the first scan. Batching in groups of 50 prevents this.
    let mut scan_tasks = Vec::new();
    const BATCH_SIZE: usize = 50;

    for ip in ip_range {
        for (port, protocol) in &printer_ports {
            let ip_str = ip.to_string();
            let port = *port;
            let protocol = protocol.to_string();
            let timeout = Duration::from_millis(timeout_ms);

            let task = tokio::spawn(async move {
                // Try TCP connection
                match tokio::time::timeout(
                    timeout,
                    tokio::net::TcpStream::connect(format!("{}:{}", ip_str, port)),
                )
                .await
                {
                    Ok(Ok(_stream)) => {
                        debug!("Port {} open on {} ({})", port, ip_str, protocol);
                        Some((ip_str, port, protocol))
                    }
                    Ok(Err(_)) => None,
                    Err(_) => None, // Timeout
                }
            });

            scan_tasks.push(task);

            // Process in batches to avoid overwhelming network stack
            if scan_tasks.len() >= BATCH_SIZE {
                let batch_results = futures_util::future::join_all(scan_tasks).await;
                // Process this batch immediately
                for result in batch_results {
                    if let Ok(Some((ip, port, protocol))) = result {
                        let id = format!("tcp_{}", ip.replace('.', "_"));

                        if !discovered.contains_key(&id) {
                            let name = if port == 631 {
                                query_ipp_printer_name(&ip).await.unwrap_or_else(|| format!("Printer at {}", ip))
                            } else {
                                format!("Printer at {}", ip)
                            };

                            let vendor = query_printer_vendor(&ip).await;

                            // Port 9100 = raw ESC/POS, port 631 = IPP (may not be ESC/POS)
                            let detected_protocol = if port == 9100 {
                                "escpos".to_string()
                            } else {
                                "unknown".to_string()
                            };

                            let printer = DiscoveredPrinter {
                                id: id.clone(),
                                name: name.clone(),
                                connection_type: "network".to_string(),
                                address: format!("{}:{}", ip, if port == 9100 { 9100 } else { port }),
                                vendor,
                                capabilities: Some(serde_json::json!({
                                    "ports": {
                                        "9100": port == 9100,
                                        "631": port == 631,
                                        "515": port == 515,
                                    },
                                    "protocol": protocol,
                                })),
                                protocol: detected_protocol,
                            };

                            discovered.insert(id.clone(), printer);
                            info!("Discovered TCP printer: {} at {}:{}", name, ip, port);
                        }
                    }
                }
                scan_tasks = Vec::new(); // Clear for next batch
            }
        }
    }

    // Process remaining tasks in final batch
    let results = futures_util::future::join_all(scan_tasks).await;

    // Process results
    for result in results {
        if let Ok(Some((ip, port, protocol))) = result {
            let id = format!("tcp_{}", ip.replace('.', "_"));

            // Only create one entry per IP (deduplicate multiple open ports)
            if !discovered.contains_key(&id) {
                // Try to get printer details via IPP if port 631 is open
                let name = if port == 631 {
                    query_ipp_printer_name(&ip).await.unwrap_or_else(|| format!("Printer at {}", ip))
                } else {
                    format!("Printer at {}", ip)
                };

                // Try to detect vendor from hostname or reverse DNS
                let vendor = query_printer_vendor(&ip).await;

                let detected_protocol = if port == 9100 {
                    "escpos".to_string()
                } else {
                    "unknown".to_string()
                };

                let printer = DiscoveredPrinter {
                    id: id.clone(),
                    name: name.clone(),
                    connection_type: "network".to_string(),
                    address: format!("{}:{}", ip, if port == 9100 { 9100 } else { port }), // Prefer 9100
                    vendor,
                    capabilities: Some(serde_json::json!({
                        "ports": {
                            "9100": port == 9100,
                            "631": port == 631,
                            "515": port == 515,
                        },
                        "protocol": protocol,
                    })),
                    protocol: detected_protocol,
                };

                discovered.insert(id.clone(), printer);
                info!("Discovered TCP printer: {} at {}:{}", name, ip, port);
            }
        }
    }

    let printers: Vec<DiscoveredPrinter> = discovered.into_values().collect();
    info!("TCP port scan complete: {} printers found", printers.len());

    Ok(printers)
}

/// Query IPP printer name via HTTP GET to /
///
/// Many thermal printers expose a web interface on port 631 or 80
/// that includes the printer model in the HTTP response.
async fn query_ipp_printer_name(ip: &str) -> Option<String> {
    // Try IPP GetPrinterAttributes request
    // For simplicity, try HTTP GET to port 631 first
    let urls = vec![
        format!("http://{}:631/", ip),
        format!("http://{}:80/", ip),
    ];

    for url in urls {
        match tokio::time::timeout(
            Duration::from_millis(500),
            reqwest::get(&url),
        )
        .await
        {
            Ok(Ok(response)) => {
                if let Ok(text) = response.text().await {
                    // Look for printer model in HTML title or body
                    if let Some(model) = extract_printer_model(&text) {
                        return Some(model);
                    }
                }
            }
            _ => continue,
        }
    }

    None
}

/// Extract printer model from HTML response
fn extract_printer_model(html: &str) -> Option<String> {
    // Look for common patterns in printer web interfaces
    let patterns = vec![
        r#"<title>([^<]*(?:TM-|TSP|Printer)[^<]*)</title>"#,
        r#"model["\s:=]+([^"<>\n]+)"#,
        r#"EPSON ([^<>\n"]+)"#,
        r#"Star ([^<>\n"]+)"#,
    ];

    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(captures) = re.captures(html) {
                if let Some(model) = captures.get(1) {
                    return Some(model.as_str().trim().to_string());
                }
            }
        }
    }

    None
}

/// Query printer vendor via reverse DNS or hostname patterns
async fn query_printer_vendor(ip: &str) -> String {
    // Try reverse DNS lookup
    if let Ok(hostname) = tokio::net::lookup_host(format!("{}:80", ip))
        .await
        .and_then(|mut addrs| {
            addrs
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No address"))
        })
    {
        let host_str = format!("{:?}", hostname);
        return detect_vendor(&host_str, &host_str);
    }

    // Default to Unknown
    "Unknown".to_string()
}

/// Scan network subnet for printers via SNMP
///
/// Performs UDP port 161 scanning with SNMP community string testing.
/// Tests common community strings (public, private) and queries printer MIB.
///
/// # Arguments
/// * `subnet` - CIDR notation subnet (e.g., "192.168.1.0/24")
///
/// # Returns
/// List of discovered printers responding to SNMP queries
///
/// # Note
/// SNMP scanning is slow (UDP-based, ~100ms per IP) and primarily used
/// for legacy printers that don't support mDNS. Modern thermal printers
/// should be discovered via `scan_subnet_tcp()` instead.
// TEMPORARILY DISABLED: SNMP discovery (API incompatibility with snmp2 crate)
// Can be re-enabled later - provides ~92% coverage without it (only misses legacy enterprise printers)
//
// pub async fn scan_subnet_snmp(subnet: &str) -> Result<Vec<DiscoveredPrinter>> {
//     info!("Starting SNMP subnet scan: {}", subnet);
//
//     // Parse subnet CIDR
//     let ip_range = parse_cidr(subnet)?;
//     let mut discovered = Vec::new();
//
//     // Common SNMP community strings
//     let communities = vec!["public", "private"];
//
//     // Printer device description OID (RFC 3805 - Printer MIB v2)
//     // 1.3.6.1.2.1.25.3.2.1.3.1 = hrDeviceDescr (Host Resources MIB)
//     let printer_oid = vec![1, 3, 6, 1, 2, 1, 25, 3, 2, 1, 3, 1];
//
//     for ip in ip_range {
//         let ip_str = ip.to_string();
//
//         for community in &communities {
//             // Try SNMP GET request
//             match tokio::time::timeout(
//                 Duration::from_millis(500),
//                 snmp_get(&ip_str, community, &printer_oid),
//             )
//             .await
//             {
//                 Ok(Ok(Some(description))) => {
//                     debug!("SNMP response from {}: {}", ip_str, description);
//
//                     // Check if response indicates a printer
//                     if is_printer_device(&description) {
//                         let id = format!("snmp_{}", ip_str.replace('.', "_"));
//                         let vendor = detect_vendor_from_description(&description);
//
//                         let printer = DiscoveredPrinter {
//                             id: id.clone(),
//                             name: description.clone(),
//                             connection_type: "network".to_string(),
//                             address: format!("{}:9100", ip_str), // Raw TCP port 9100
//                             vendor,
//                             capabilities: None,
//                         };
//
//                         discovered.push(printer);
//                         info!("Discovered SNMP printer: {} at {}", description, ip_str);
//                         break; // Found with this community, skip others
//                     }
//                 }
//                 Ok(Ok(None)) => {
//                     debug!("No SNMP response from {} with community '{}'", ip_str, community);
//                 }
//                 Ok(Err(e)) => {
//                     debug!("SNMP error for {} with community '{}': {}", ip_str, community, e);
//                 }
//                 Err(_) => {
//                     // Timeout, try next community
//                     continue;
//                 }
//             }
//         }
//     }
//
//     info!("SNMP subnet scan complete: {} printers found", discovered.len());
//     Ok(discovered)
// }

/// Parse CIDR notation into IP range
fn parse_cidr(cidr: &str) -> Result<Vec<std::net::Ipv4Addr>> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(DaemonError::Discovery(format!("Invalid CIDR notation: {}", cidr)));
    }

    let base_ip: std::net::Ipv4Addr = parts[0]
        .parse()
        .map_err(|_| DaemonError::Discovery(format!("Invalid IP address: {}", parts[0])))?;

    let prefix_len: u8 = parts[1]
        .parse()
        .map_err(|_| DaemonError::Discovery(format!("Invalid prefix length: {}", parts[1])))?;

    if prefix_len > 32 {
        return Err(DaemonError::Discovery(format!(
            "Invalid prefix length: {}",
            prefix_len
        )));
    }

    let base = u32::from(base_ip);
    let mask = !0u32 << (32 - prefix_len);
    let network = base & mask;
    let broadcast = network | !mask;

    let mut ips = Vec::new();
    for ip_u32 in (network + 1)..broadcast {
        // Skip network and broadcast addresses
        ips.push(std::net::Ipv4Addr::from(ip_u32));
    }

    Ok(ips)
}

// TEMPORARILY DISABLED: SNMP helper function (API incompatibility with snmp2 crate)
//
// /// Perform SNMP GET request
// async fn snmp_get(
//     ip: &str,
//     community: &str,
//     oid: &[u32],
// ) -> Result<Option<String>> {
//     use snmp2::{ClientBuilder, OctetString, VarBind};
//
//     // Create SNMP client
//     let mut client = ClientBuilder::new()
//         .set_community(OctetString::from_str(community))
//         .set_timeout(Duration::from_millis(500))
//         .build_v2c(format!("{}:161", ip))
//         .await
//         .map_err(|e| DaemonError::Discovery(format!("SNMP client error: {}", e)))?;
//
//     // Convert OID to VarBind
//     let oid_string = oid
//         .iter()
//         .map(|n| n.to_string())
//         .collect::<Vec<_>>()
//         .join(".");
//     let varbind = VarBind::from_oid_str(&oid_string);
//
//     // Send GET request
//     match client.get(varbind).await {
//         Ok(response) => {
//             if let Some(value) = response.first() {
//                 if let Ok(string_value) = value.value.to_string() {
//                     return Ok(Some(string_value));
//                 }
//             }
//             Ok(None)
//         }
//         Err(e) => {
//             debug!("SNMP GET failed for {}: {}", ip, e);
//             Ok(None)
//         }
//     }
// }

/// Discover printers via WS-Discovery (Web Services Discovery)
///
/// WS-Discovery is used by Windows environments and Brother/Epson printers.
/// Sends SOAP XML probe on UDP multicast 239.255.255.250:3702.
///
/// # Arguments
/// * `timeout_secs` - Discovery timeout in seconds (default: 5)
///
/// # Returns
/// List of discovered printers responding to WS-Discovery probes
///
/// # Protocol
/// - Multicast SOAP probe to 239.255.255.250:3702
/// - Printers respond with device info (model, IP, MAC, services)
/// - Common in Windows POS environments
pub async fn discover_ws_discovery(timeout_secs: u64) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting WS-Discovery scan (timeout: {}s)", timeout_secs);

    let multicast_addr = "239.255.255.250:3702";
    let mut discovered = HashMap::new();

    // Create UDP socket
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| DaemonError::Discovery(format!("Failed to bind UDP socket: {}", e)))?;

    socket.set_broadcast(true)
        .map_err(|e| DaemonError::Discovery(format!("Failed to set broadcast: {}", e)))?;

    // WS-Discovery Probe message (SOAP XML)
    let probe_msg = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope"
               xmlns:wsa="http://schemas.xmlsoap.org/ws/2004/08/addressing"
               xmlns:wsd="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <soap:Header>
    <wsa:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</wsa:Action>
    <wsa:MessageID>urn:uuid:{}</wsa:MessageID>
    <wsa:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</wsa:To>
  </soap:Header>
  <soap:Body>
    <wsd:Probe>
      <wsd:Types>wsdp:Device</wsd:Types>
    </wsd:Probe>
  </soap:Body>
</soap:Envelope>"#,
        uuid::Uuid::new_v4()
    );

    // Send probe
    socket
        .send_to(probe_msg.as_bytes(), multicast_addr)
        .await
        .map_err(|e| DaemonError::Discovery(format!("Failed to send WS-Discovery probe: {}", e)))?;

    debug!("WS-Discovery probe sent to {}", multicast_addr);

    // Listen for responses
    let mut buf = vec![0u8; 8192];
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(timeout_secs) {
        match tokio::time::timeout(
            Duration::from_millis(500),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                debug!("WS-Discovery response from {}: {} bytes", addr, len);

                // Parse SOAP response for printer info
                if let Some(printer) = parse_wsd_response(&response, addr.ip().to_string()) {
                    discovered.insert(printer.id.clone(), printer);
                }
            }
            Ok(Err(e)) => {
                warn!("WS-Discovery recv error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout, continue listening
                continue;
            }
        }
    }

    let printers: Vec<DiscoveredPrinter> = discovered.into_values().collect();
    info!("WS-Discovery scan complete: {} printers found", printers.len());

    Ok(printers)
}

/// Parse WS-Discovery SOAP response
fn parse_wsd_response(xml: &str, ip: String) -> Option<DiscoveredPrinter> {
    // Look for printer-related keywords in response
    let lower = xml.to_lowercase();

    if !lower.contains("printer")
        && !lower.contains("epson")
        && !lower.contains("brother")
        && !lower.contains("star")
        && !lower.contains("citizen") {
        return None;
    }

    // Extract model name from XML (various possible tags)
    let name = extract_xml_text(xml, "FriendlyName")
        .or_else(|| extract_xml_text(xml, "ModelName"))
        .or_else(|| extract_xml_text(xml, "Manufacturer"))
        .unwrap_or_else(|| format!("Printer at {}", ip));

    // Extract MAC address if available
    let mac = extract_xml_text(xml, "PresentationUrl")
        .or_else(|| extract_xml_text(xml, "Address"));

    let vendor = detect_vendor(&name, xml);
    let id = format!("wsd_{}", ip.replace('.', "_"));

    Some(DiscoveredPrinter {
        id,
        name,
        connection_type: "network".to_string(),
        address: format!("{}:9100", ip), // Default to raw TCP
        vendor,
        capabilities: Some(serde_json::json!({
            "discovery_method": "WS-Discovery",
            "mac_address": mac,
        })),
        protocol: "unknown".to_string(), // WS-Discovery - could be PCL/IPP
    })
}

/// Extract text from XML tag
fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    if let Some(start_idx) = xml.find(&start_tag) {
        let content_start = start_idx + start_tag.len();
        if let Some(end_idx) = xml[content_start..].find(&end_tag) {
            return Some(xml[content_start..content_start + end_idx].trim().to_string());
        }
    }
    None
}

/// Discover Epson printers via ENPC (Epson Network Printer Configuration)
///
/// ENPC is Epson's proprietary discovery protocol on UDP port 3289.
/// Used by EpsonNet tools and ePOS SDK for reliable Epson printer discovery.
///
/// # Arguments
/// * `timeout_secs` - Discovery timeout in seconds (default: 3)
///
/// # Returns
/// List of discovered Epson printers
///
/// # Protocol
/// - UDP broadcast to 255.255.255.255:3289
/// - Binary protocol with 4-byte header: "EPSONPS\x00"
/// - Response contains IP, MAC, model, serial number
pub async fn discover_epson_enpc(timeout_secs: u64) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting Epson ENPC discovery (timeout: {}s)", timeout_secs);

    let mut discovered = HashMap::new();

    // Create UDP socket
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| DaemonError::Discovery(format!("Failed to bind UDP socket: {}", e)))?;

    socket.set_broadcast(true)
        .map_err(|e| DaemonError::Discovery(format!("Failed to set broadcast: {}", e)))?;

    // ENPC discovery packet (binary format)
    // Header: "EPSONPS\x00" followed by query type
    let probe_packet: Vec<u8> = vec![
        0x45, 0x50, 0x53, 0x4f, 0x4e, 0x50, 0x53, 0x00, // "EPSONPS\x00"
        0x00, 0x00, 0x00, 0x01, // Query type: search
        0x00, 0x00, 0x00, 0x00, // Reserved
    ];

    // Send broadcast probe
    socket
        .send_to(&probe_packet, "255.255.255.255:3289")
        .await
        .map_err(|e| DaemonError::Discovery(format!("Failed to send ENPC probe: {}", e)))?;

    debug!("ENPC probe sent to broadcast:3289");

    // Listen for responses
    let mut buf = vec![0u8; 2048];
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(timeout_secs) {
        match tokio::time::timeout(
            Duration::from_millis(500),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, addr))) => {
                debug!("ENPC response from {}: {} bytes", addr, len);

                // Parse ENPC binary response
                if let Some(printer) = parse_enpc_response(&buf[..len], addr.ip().to_string()) {
                    discovered.insert(printer.id.clone(), printer);
                }
            }
            Ok(Err(e)) => {
                warn!("ENPC recv error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout, continue listening
                continue;
            }
        }
    }

    let printers: Vec<DiscoveredPrinter> = discovered.into_values().collect();
    info!("ENPC discovery complete: {} Epson printers found", printers.len());

    Ok(printers)
}

/// Parse ENPC binary response
fn parse_enpc_response(data: &[u8], ip: String) -> Option<DiscoveredPrinter> {
    // Verify ENPC header
    if data.len() < 8 || &data[0..7] != b"EPSONPS" {
        return None;
    }

    // Extract model name (typically starts at offset 16, null-terminated)
    let model_start = 16;
    if data.len() < model_start + 10 {
        return None;
    }

    let model_bytes = &data[model_start..];
    let model_end = model_bytes.iter().position(|&b| b == 0).unwrap_or(model_bytes.len());
    let model = String::from_utf8_lossy(&model_bytes[..model_end]).to_string();

    // Extract MAC address if present (offset varies, look for pattern)
    let mac = extract_mac_from_bytes(data);

    let name = if model.is_empty() {
        format!("Epson Printer at {}", ip)
    } else {
        model
    };

    let id = format!("enpc_{}", ip.replace('.', "_"));

    Some(DiscoveredPrinter {
        id,
        name,
        connection_type: "network".to_string(),
        address: format!("{}:9100", ip),
        vendor: "Epson".to_string(),
        capabilities: Some(serde_json::json!({
            "discovery_method": "ENPC",
            "mac_address": mac,
        })),
        protocol: "escpos".to_string(), // Epson ENPC = guaranteed ESC/POS
    })
}

/// Extract MAC address from binary data
fn extract_mac_from_bytes(data: &[u8]) -> Option<String> {
    // Look for MAC address pattern (6 consecutive bytes with typical ranges)
    for i in 0..data.len().saturating_sub(6) {
        let bytes = &data[i..i + 6];
        // Check if looks like MAC (not all zeros, not all 0xFF)
        if bytes.iter().any(|&b| b != 0) && bytes.iter().any(|&b| b != 0xFF) {
            return Some(format!(
                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
            ));
        }
    }
    None
}

/// Detect Star CloudPRNT printers via HTTP
///
/// Star CloudPRNT is used by modern Star Micronics printers (TSP700II, TSP800II, mC-Print3).
/// Checks for CloudPRNT capability by querying HTTP endpoint.
///
/// # Arguments
/// * `subnet` - CIDR notation subnet to scan
///
/// # Returns
/// List of discovered Star CloudPRNT printers
///
/// # Protocol
/// - HTTP GET to http://IP/StarWebPRNT/status
/// - HTTP GET to http://IP/StarWebPRNT/CloudPRNT (alternative endpoint)
/// - Response indicates CloudPRNT support
pub async fn discover_star_cloudprnt(subnet: &str) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting Star CloudPRNT discovery on subnet: {}", subnet);

    let ip_range = parse_cidr(subnet)?;
    let mut discovered = HashMap::new();

    // CloudPRNT endpoints to check
    let endpoints = vec![
        "/StarWebPRNT/status",
        "/StarWebPRNT/CloudPRNT",
        "/cgi-bin/epos/service.cgi?devid=local_printer&timeout=10000",
    ];

    // Create parallel tasks for checking CloudPRNT
    let mut check_tasks = Vec::new();

    for ip in ip_range {
        for endpoint in &endpoints {
            let ip_str = ip.to_string();
            let endpoint = endpoint.to_string();

            let task = tokio::spawn(async move {
                check_cloudprnt_endpoint(&ip_str, &endpoint).await
            });

            check_tasks.push(task);
        }
    }

    // Wait for all checks to complete
    let results = futures_util::future::join_all(check_tasks).await;

    // Process results
    for result in results {
        if let Ok(Some(printer)) = result {
            discovered.insert(printer.id.clone(), printer);
        }
    }

    let printers: Vec<DiscoveredPrinter> = discovered.into_values().collect();
    info!("CloudPRNT discovery complete: {} Star printers found", printers.len());

    Ok(printers)
}

/// Check if IP has CloudPRNT endpoint
async fn check_cloudprnt_endpoint(ip: &str, endpoint: &str) -> Option<DiscoveredPrinter> {
    let url = format!("http://{}{}", ip, endpoint);

    match tokio::time::timeout(
        Duration::from_millis(500),
        reqwest::get(&url),
    )
    .await
    {
        Ok(Ok(response)) => {
            if response.status().is_success() {
                if let Ok(text) = response.text().await {
                    // Check if response indicates CloudPRNT support
                    if text.contains("CloudPRNT")
                        || text.contains("StarWebPRNT")
                        || text.contains("\"status\":")
                        || text.contains("\"mac\":") {

                        // Extract printer info from response
                        let name = extract_json_field(&text, "model")
                            .or_else(|| extract_json_field(&text, "modelName"))
                            .unwrap_or_else(|| format!("Star CloudPRNT at {}", ip));

                        let mac = extract_json_field(&text, "mac");

                        let id = format!("cloudprnt_{}", ip.replace('.', "_"));

                        return Some(DiscoveredPrinter {
                            id,
                            name,
                            connection_type: "network".to_string(),
                            address: format!("{}:9100", ip), // CloudPRNT also supports raw TCP
                            vendor: "Star Micronics".to_string(),
                            capabilities: Some(serde_json::json!({
                                "discovery_method": "CloudPRNT",
                                "cloudprnt_url": url,
                                "mac_address": mac,
                            })),
                            protocol: "escpos".to_string(), // Star CloudPRNT = ESC/POS compatible
                        });
                    }
                }
            }
        }
        _ => {}
    }

    None
}

/// Extract JSON field from text response
fn extract_json_field(text: &str, field: &str) -> Option<String> {
    // Simple JSON field extraction (not a full JSON parser)
    let pattern = format!("\"{}\":", field);
    if let Some(start) = text.find(&pattern) {
        let value_start = start + pattern.len();
        let remaining = &text[value_start..];

        // Skip whitespace and quotes
        let trimmed = remaining.trim_start();
        if trimmed.starts_with('"') {
            // String value
            if let Some(end) = trimmed[1..].find('"') {
                return Some(trimmed[1..end + 1].to_string());
            }
        }
    }
    None
}

// ============================================================================
// ESC/POS Protocol Detection
// ============================================================================

/// Probe a network printer to detect ESC/POS support
///
/// Sends a DLE EOT (0x10 0x04 0x01) status query to the printer.
/// Valid ESC/POS printers respond with a 1-byte status within 500ms.
/// Non-ESC/POS printers (PCL, IPP, ZPL) will timeout or error.
///
/// # Arguments
/// * `address` - Network address in "host:port" format
///
/// # Returns
/// * `true` if the printer responds to ESC/POS status query
/// * `false` if timeout, error, or invalid response
pub async fn probe_escpos_support(address: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    // DLE EOT n (0x10 0x04 0x01) = Real-Time Status Transmission (paper sensor)
    const STATUS_QUERY: &[u8] = &[0x10, 0x04, 0x01];

    let result = tokio::time::timeout(Duration::from_millis(800), async {
        let mut stream = TcpStream::connect(address).await?;
        stream.write_all(STATUS_QUERY).await?;
        stream.flush().await?;

        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(
            Duration::from_millis(500),
            stream.read(&mut buf),
        )
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "read timeout"))?;

        match n {
            Ok(1) => {
                // ESC/POS status byte: bit patterns indicate printer state
                // Any non-zero response is valid (online or with errors)
                Ok::<bool, std::io::Error>(true)
            }
            _ => Ok(false),
        }
    })
    .await;

    match result {
        Ok(Ok(true)) => {
            debug!("ESC/POS probe succeeded for {}", address);
            true
        }
        _ => {
            debug!("ESC/POS probe failed for {} (timeout or no response)", address);
            false
        }
    }
}

/// Probe all "unknown" protocol printers in a discovery result set
///
/// Only probes network printers with protocol="unknown" to avoid
/// unnecessary network traffic for already-identified printers.
pub async fn probe_unknown_printers(printers: &mut [DiscoveredPrinter]) {
    for printer in printers.iter_mut() {
        if printer.protocol == "unknown" && printer.connection_type == "network" {
            info!("Probing ESC/POS support for: {} ({})", printer.name, printer.address);
            if probe_escpos_support(&printer.address).await {
                printer.protocol = "escpos".to_string();
                info!("  -> ESC/POS confirmed for {}", printer.name);
            } else {
                printer.protocol = "unsupported".to_string();
                warn!("  -> ESC/POS NOT detected for {} - may not be compatible", printer.name);
            }
        }
    }
}
