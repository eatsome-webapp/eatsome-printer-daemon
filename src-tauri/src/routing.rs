use crate::config::PrinterConfig;
use crate::errors::{DaemonError, Result};
use crate::queue::PrintJob;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Routing group (station) in the kitchen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingGroup {
    pub id: String,
    pub name: String,           // 'bar', 'grill', 'kitchen'
    pub display_name: String,    // 'Bar', 'Grill Station', 'Main Kitchen'
    pub color: Option<String>,
    pub sort_order: i32,
}

/// Printer assignment to a routing group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterAssignment {
    pub routing_group_id: String,
    pub printer_id: String,
    pub is_primary: bool,
    pub is_backup: bool,
}

/// Menu item routing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItemRouting {
    pub menu_item_id: String,
    pub routing_group_ids: Vec<String>, // Can route to multiple stations
}

/// Route decision for a print job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub station: String,
    pub printer_ids: Vec<String>, // Primary + backups
    pub items: Vec<RouteItem>,
}

/// Item included in a route
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteItem {
    pub menu_item_id: String,
    pub quantity: u32,
    pub name: String,
    pub modifiers: Vec<String>,
    pub notes: Option<String>,
}

/// Kitchen router for managing print job routing
pub struct KitchenRouter {
    /// Routing groups (stations)
    routing_groups: Arc<RwLock<HashMap<String, RoutingGroup>>>,
    /// Printer assignments (which printers handle which stations)
    printer_assignments: Arc<RwLock<Vec<PrinterAssignment>>>,
    /// Menu item routing (which items go to which stations)
    menu_item_routing: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Configured printers
    printers: Arc<RwLock<HashMap<String, PrinterConfig>>>,
}

impl KitchenRouter {
    /// Create new kitchen router
    pub fn new() -> Self {
        Self {
            routing_groups: Arc::new(RwLock::new(HashMap::new())),
            printer_assignments: Arc::new(RwLock::new(Vec::new())),
            menu_item_routing: Arc::new(RwLock::new(HashMap::new())),
            printers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add routing group (station)
    pub async fn add_routing_group(&self, group: RoutingGroup) {
        let mut groups = self.routing_groups.write().await;
        info!("Adding routing group: {} ({})", group.name, group.display_name);
        groups.insert(group.id.clone(), group);
    }

    /// Add printer assignment
    pub async fn add_printer_assignment(&self, assignment: PrinterAssignment) {
        let mut assignments = self.printer_assignments.write().await;
        debug!(
            "Adding printer assignment: {} -> {} (primary: {}, backup: {})",
            assignment.printer_id, assignment.routing_group_id, assignment.is_primary, assignment.is_backup
        );
        assignments.push(assignment);
    }

    /// Set menu item routing
    pub async fn set_menu_item_routing(&self, menu_item_id: String, routing_group_ids: Vec<String>) {
        let mut routing = self.menu_item_routing.write().await;
        debug!("Setting menu item routing: {} -> {:?}", menu_item_id, routing_group_ids);
        routing.insert(menu_item_id, routing_group_ids);
    }

    /// Update printers configuration
    pub async fn update_printers(&self, printers: Vec<PrinterConfig>) {
        let mut printer_map = self.printers.write().await;
        printer_map.clear();
        for printer in printers {
            printer_map.insert(printer.id.clone(), printer);
        }
        info!("Updated printer configuration: {} printers", printer_map.len());
    }

    /// Route a print job to appropriate stations
    pub async fn route_job(&self, job: &PrintJob) -> Result<Vec<RouteDecision>> {
        let routing_map = self.menu_item_routing.read().await;
        let groups = self.routing_groups.read().await;

        // Group items by routing groups
        let mut station_items: HashMap<String, Vec<RouteItem>> = HashMap::new();

        for item in &job.items {
            // Get routing groups for this menu item
            let routing_group_ids = routing_map
                .get(&item.name) // Using name as menu_item_id for now (TODO: use actual menu_item_id)
                .cloned()
                .unwrap_or_else(|| {
                    // Default routing: use station from job if specified
                    if let Some(station_group) = groups.values().find(|g| g.name == job.station) {
                        vec![station_group.id.clone()]
                    } else {
                        warn!("No routing found for item '{}', using default station '{}'", item.name, job.station);
                        vec![job.station.clone()]
                    }
                });

            // Add item to each routing group
            for group_id in routing_group_ids {
                let group_name = groups
                    .get(&group_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| group_id.clone());

                station_items.entry(group_name).or_insert_with(Vec::new).push(RouteItem {
                    menu_item_id: item.name.clone(), // TODO: use actual menu_item_id
                    quantity: item.quantity,
                    name: item.name.clone(),
                    modifiers: item.modifiers.clone(),
                    notes: item.notes.clone(),
                });
            }
        }

        // Build route decisions for each station
        let mut decisions = Vec::new();
        for (station, items) in station_items {
            let printer_ids = self.get_printers_for_station(&station).await?;

            if printer_ids.is_empty() {
                error!("No printers configured for station: {}", station);
                return Err(DaemonError::Config(format!(
                    "No printers configured for station: {}",
                    station
                )));
            }

            decisions.push(RouteDecision {
                station: station.clone(),
                printer_ids,
                items,
            });
        }

        if decisions.is_empty() {
            error!("No routing decisions generated for job: {}", job.id);
            return Err(DaemonError::Queue("No routing decisions generated".to_string()));
        }

        info!(
            "Routed job {} to {} stations",
            job.order_number,
            decisions.len()
        );

        Ok(decisions)
    }

    /// Get printers for a specific station (primary + backups)
    async fn get_printers_for_station(&self, station: &str) -> Result<Vec<String>> {
        let assignments = self.printer_assignments.read().await;
        let groups = self.routing_groups.read().await;

        // Find routing group by name
        let group_id = groups
            .values()
            .find(|g| g.name == station)
            .map(|g| g.id.clone());

        if let Some(group_id) = group_id {
            // Get all printers assigned to this group
            let mut printer_ids: Vec<String> = assignments
                .iter()
                .filter(|a| a.routing_group_id == group_id)
                .map(|a| (a.printer_id.clone(), a.is_primary))
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|(id, is_primary)| {
                    // Sort: primary first, then backups
                    Some(id)
                })
                .collect();

            // Sort: primary printers first
            let primary_assignments: Vec<_> = assignments
                .iter()
                .filter(|a| a.routing_group_id == group_id && a.is_primary)
                .map(|a| a.printer_id.clone())
                .collect();

            let backup_assignments: Vec<_> = assignments
                .iter()
                .filter(|a| a.routing_group_id == group_id && !a.is_primary)
                .map(|a| a.printer_id.clone())
                .collect();

            printer_ids = primary_assignments;
            printer_ids.extend(backup_assignments);

            Ok(printer_ids)
        } else {
            warn!("Routing group not found for station: {}", station);
            Ok(Vec::new())
        }
    }

    /// Get all routing groups
    pub async fn get_routing_groups(&self) -> Vec<RoutingGroup> {
        let groups = self.routing_groups.read().await;
        let mut sorted: Vec<_> = groups.values().cloned().collect();
        sorted.sort_by_key(|g| g.sort_order);
        sorted
    }

    /// Get printer assignments for a routing group
    pub async fn get_assignments_for_group(&self, group_id: &str) -> Vec<PrinterAssignment> {
        let assignments = self.printer_assignments.read().await;
        assignments
            .iter()
            .filter(|a| a.routing_group_id == group_id)
            .cloned()
            .collect()
    }

    /// Clear all routing configuration (for tests/reset)
    pub async fn clear_all(&self) {
        let mut groups = self.routing_groups.write().await;
        let mut assignments = self.printer_assignments.write().await;
        let mut routing = self.menu_item_routing.write().await;

        groups.clear();
        assignments.clear();
        routing.clear();

        info!("Cleared all routing configuration");
    }
}

impl Default for KitchenRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnectionType, PrinterCapabilities};
    use crate::escpos::PrintItem;

    fn create_test_printer(id: &str, station: Option<String>) -> PrinterConfig {
        PrinterConfig {
            id: id.to_string(),
            name: format!("Test Printer {}", id),
            connection_type: ConnectionType::USB,
            address: "/dev/usb/lp0".to_string(),
            protocol: "escpos".to_string(),
            station,
            is_primary: true,
            capabilities: PrinterCapabilities {
                cutter: true,
                drawer: false,
                qrcode: true,
                max_width: 48,
            },
        }
    }

    #[tokio::test]
    async fn test_route_job_to_single_station() {
        let router = KitchenRouter::new();

        // Setup routing group
        router
            .add_routing_group(RoutingGroup {
                id: "group_bar".to_string(),
                name: "bar".to_string(),
                display_name: "Bar".to_string(),
                color: None,
                sort_order: 0,
            })
            .await;

        // Setup printer assignment
        router
            .add_printer_assignment(PrinterAssignment {
                routing_group_id: "group_bar".to_string(),
                printer_id: "printer_1".to_string(),
                is_primary: true,
                is_backup: false,
            })
            .await;

        // Setup menu item routing
        router.set_menu_item_routing("Beer".to_string(), vec!["group_bar".to_string()]).await;

        // Create test job
        let job = PrintJob {
            id: "job_1".to_string(),
            restaurant_id: "rest_123".to_string(),
            order_id: "order_1".to_string(),
            order_number: "R001-20260127-0001".to_string(),
            station: "bar".to_string(),
            printer_id: None,
            items: vec![PrintItem {
                quantity: 2,
                name: "Beer".to_string(),
                modifiers: vec![],
                notes: None,
            }],
            table_number: Some("5".to_string()),
            customer_name: None,
            order_type: Some("dine-in".to_string()),
            priority: 3,
            timestamp: 1738041600000,
            status: "pending".to_string(),
            retry_count: 0,
            error_message: None,
        };

        // Route job
        let decisions = router.route_job(&job).await.unwrap();

        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].station, "bar");
        assert_eq!(decisions[0].printer_ids.len(), 1);
        assert_eq!(decisions[0].printer_ids[0], "printer_1");
        assert_eq!(decisions[0].items.len(), 1);
        assert_eq!(decisions[0].items[0].name, "Beer");
        assert_eq!(decisions[0].items[0].quantity, 2);
    }

    #[tokio::test]
    async fn test_route_job_to_multiple_stations() {
        let router = KitchenRouter::new();

        // Setup routing groups
        router
            .add_routing_group(RoutingGroup {
                id: "group_grill".to_string(),
                name: "grill".to_string(),
                display_name: "Grill".to_string(),
                color: None,
                sort_order: 0,
            })
            .await;

        router
            .add_routing_group(RoutingGroup {
                id: "group_kitchen".to_string(),
                name: "kitchen".to_string(),
                display_name: "Kitchen".to_string(),
                color: None,
                sort_order: 1,
            })
            .await;

        // Setup printer assignments
        router
            .add_printer_assignment(PrinterAssignment {
                routing_group_id: "group_grill".to_string(),
                printer_id: "printer_grill".to_string(),
                is_primary: true,
                is_backup: false,
            })
            .await;

        router
            .add_printer_assignment(PrinterAssignment {
                routing_group_id: "group_kitchen".to_string(),
                printer_id: "printer_kitchen".to_string(),
                is_primary: true,
                is_backup: false,
            })
            .await;

        // Setup menu item routing (one item goes to both stations)
        router.set_menu_item_routing("Grilled Salad".to_string(), vec!["group_grill".to_string(), "group_kitchen".to_string()]).await;

        // Create test job
        let job = PrintJob {
            id: "job_2".to_string(),
            restaurant_id: "rest_123".to_string(),
            order_id: "order_2".to_string(),
            order_number: "R001-20260127-0002".to_string(),
            station: "grill".to_string(),
            printer_id: None,
            items: vec![PrintItem {
                quantity: 1,
                name: "Grilled Salad".to_string(),
                modifiers: vec!["Extra Dressing".to_string()],
                notes: Some("No onions".to_string()),
            }],
            table_number: Some("3".to_string()),
            customer_name: None,
            order_type: Some("dine-in".to_string()),
            priority: 2,
            timestamp: 1738041600000,
            status: "pending".to_string(),
            retry_count: 0,
            error_message: None,
        };

        // Route job
        let decisions = router.route_job(&job).await.unwrap();

        // Should route to both grill and kitchen
        assert_eq!(decisions.len(), 2);

        let stations: Vec<_> = decisions.iter().map(|d| d.station.as_str()).collect();
        assert!(stations.contains(&"grill"));
        assert!(stations.contains(&"kitchen"));
    }
}
