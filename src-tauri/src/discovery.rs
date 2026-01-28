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
                                   info.get_addresses().iter().next().unwrap_or(&"unknown".parse().unwrap()),
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
/// should be discovered via `discover_network_printers()` instead.
pub async fn scan_subnet_snmp(subnet: &str) -> Result<Vec<DiscoveredPrinter>> {
    info!("Starting SNMP subnet scan: {}", subnet);

    // Parse subnet CIDR
    let ip_range = parse_cidr(subnet)?;
    let mut discovered = Vec::new();

    // Common SNMP community strings
    let communities = vec!["public", "private"];

    // Printer device description OID (RFC 3805 - Printer MIB v2)
    // 1.3.6.1.2.1.25.3.2.1.3.1 = hrDeviceDescr (Host Resources MIB)
    let printer_oid = vec![1, 3, 6, 1, 2, 1, 25, 3, 2, 1, 3, 1];

    for ip in ip_range {
        let ip_str = ip.to_string();

        for community in &communities {
            // Try SNMP GET request
            match tokio::time::timeout(
                Duration::from_millis(500),
                snmp_get(&ip_str, community, &printer_oid),
            )
            .await
            {
                Ok(Ok(Some(description))) => {
                    debug!("SNMP response from {}: {}", ip_str, description);

                    // Check if response indicates a printer
                    if is_printer_device(&description) {
                        let id = format!("snmp_{}", ip_str.replace('.', "_"));
                        let vendor = detect_vendor_from_description(&description);

                        let printer = DiscoveredPrinter {
                            id: id.clone(),
                            name: description.clone(),
                            connection_type: "network".to_string(),
                            address: format!("{}:9100", ip_str), // Raw TCP port 9100
                            vendor,
                            capabilities: None,
                        };

                        discovered.push(printer);
                        info!("Discovered SNMP printer: {} at {}", description, ip_str);
                        break; // Found with this community, skip others
                    }
                }
                Ok(Ok(None)) => {
                    debug!("No SNMP response from {} with community '{}'", ip_str, community);
                }
                Ok(Err(e)) => {
                    debug!("SNMP error for {} with community '{}': {}", ip_str, community, e);
                }
                Err(_) => {
                    // Timeout, try next community
                    continue;
                }
            }
        }
    }

    info!("SNMP subnet scan complete: {} printers found", discovered.len());
    Ok(discovered)
}

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

/// Perform SNMP GET request
/// TODO: Implement SNMP discovery when snmp2 crate API is stable
#[allow(unused_variables)]
async fn snmp_get(
    ip: &str,
    community: &str,
    oid: &[u32],
) -> Result<Option<String>> {
    // SNMP discovery temporarily disabled - snmp2 crate API unstable
    // Will be implemented in future version
    Ok(None)
}

/// Check if SNMP device description indicates a printer
fn is_printer_device(description: &str) -> bool {
    let lower = description.to_lowercase();
    lower.contains("printer")
        || lower.contains("epson")
        || lower.contains("star")
        || lower.contains("citizen")
        || lower.contains("brother")
        || lower.contains("tm-")
        || lower.contains("tsp")
        || lower.contains("pos")
}

/// Detect vendor from SNMP device description
fn detect_vendor_from_description(description: &str) -> String {
    let lower = description.to_lowercase();

    if lower.contains("epson") || lower.contains("tm-") {
        "Epson".to_string()
    } else if lower.contains("star") || lower.contains("tsp") {
        "Star Micronics".to_string()
    } else if lower.contains("citizen") {
        "Citizen".to_string()
    } else if lower.contains("brother") {
        "Brother".to_string()
    } else if lower.contains("hp") || lower.contains("hewlett") {
        "HP".to_string()
    } else if lower.contains("canon") {
        "Canon".to_string()
    } else {
        "Unknown".to_string()
    }
}
