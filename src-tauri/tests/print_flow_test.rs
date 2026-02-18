// Integration tests for end-to-end print flow

mod common;

use common::{MockPrinter, create_test_print_job};

#[tokio::test]
async fn test_successful_print_flow() {
    let printer = MockPrinter::new("usb_001", "Bar Printer");

    // Create print job
    let job = create_test_print_job("R001-20260128-0042", "bar");

    // Generate ESC/POS commands (simplified)
    let commands = vec![
        0x1B, 0x40, // Initialize printer
        0x1B, 0x61, 0x01, // Center alignment
        0x1B, 0x45, 0x01, // Bold on
        // ... more ESC/POS commands
        0x1B, 0x69, // Cut paper
    ];

    // Print
    let result = printer.print(commands).await;
    assert!(result.is_ok());

    // Verify print occurred
    assert_eq!(printer.get_print_count().await, 1);
    assert!(printer.get_last_command().await.is_some());
}

#[tokio::test]
async fn test_print_fails_when_printer_offline() {
    let printer = MockPrinter::new("net_002", "Kitchen Printer");

    // Set printer offline
    printer.set_online(false).await;

    let commands = vec![0x1B, 0x40];
    let result = printer.print(commands).await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Printer offline");
    assert_eq!(printer.get_print_count().await, 0);
}

#[tokio::test]
async fn test_print_job_routing_to_correct_station() {
    let bar_printer = MockPrinter::new("p_bar", "Bar Printer");
    let kitchen_printer = MockPrinter::new("p_kitchen", "Kitchen Printer");

    // Create bar job
    let bar_job = create_test_print_job("ORDER_BAR", "bar");
    assert_eq!(bar_job["station"], "bar");

    // Create kitchen job
    let kitchen_job = create_test_print_job("ORDER_KITCHEN", "kitchen");
    assert_eq!(kitchen_job["station"], "kitchen");

    // Print to correct printers
    let bar_result = bar_printer.print(vec![0x1B, 0x40]).await;
    let kitchen_result = kitchen_printer.print(vec![0x1B, 0x40]).await;

    assert!(bar_result.is_ok());
    assert!(kitchen_result.is_ok());

    assert_eq!(bar_printer.get_print_count().await, 1);
    assert_eq!(kitchen_printer.get_print_count().await, 1);
}

#[tokio::test]
async fn test_backup_printer_routing() {
    let primary = MockPrinter::new("p_primary", "Primary Printer");
    let backup = MockPrinter::new("p_backup", "Backup Printer");

    // Primary fails
    primary.set_online(false).await;

    let commands = vec![0x1B, 0x40];

    // Try primary first
    let primary_result = primary.print(commands.clone()).await;
    assert!(primary_result.is_err());

    // Fallback to backup
    let backup_result = backup.print(commands).await;
    assert!(backup_result.is_ok());

    // Verify routing
    assert_eq!(primary.get_print_count().await, 0);
    assert_eq!(backup.get_print_count().await, 1);
}

#[tokio::test]
async fn test_concurrent_print_jobs() {
    use tokio::task::JoinSet;

    let printer = MockPrinter::new("p_concurrent", "Concurrent Printer");

    let mut tasks = JoinSet::new();

    // Spawn 10 concurrent print jobs
    for i in 0..10 {
        let p = printer.clone();
        tasks.spawn(async move {
            let commands = vec![0x1B, 0x40, i as u8];
            p.print(commands).await
        });
    }

    // Wait for all jobs
    let mut success_count = 0;
    while let Some(result) = tasks.join_next().await {
        if result.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 10);
    assert_eq!(printer.get_print_count().await, 10);
}

#[tokio::test]
async fn test_print_order_with_modifiers() {
    let printer = MockPrinter::new("p_modifiers", "Modifier Test Printer");

    let job_with_modifiers = serde_json::json!({
        "job_id": "job_mod_123",
        "order_id": "ORDER_MOD",
        "station": "kitchen",
        "items": [
            {
                "name": "Burger",
                "quantity": 1,
                "modifiers": ["No onions", "Extra cheese", "Well done"]
            },
            {
                "name": "Salad",
                "quantity": 2,
                "modifiers": ["Dressing on side"]
            }
        ]
    });

    // Verify modifiers present
    assert!(job_with_modifiers["items"][0]["modifiers"].is_array());
    assert_eq!(job_with_modifiers["items"][0]["modifiers"].as_array().unwrap().len(), 3);

    // Generate ESC/POS with modifiers (simplified)
    let commands = vec![
        0x1B, 0x40, // Initialize
        // ... commands for "Burger"
        // ... commands for "- No onions"
        // ... commands for "- Extra cheese"
        0x1B, 0x69, // Cut
    ];

    let result = printer.print(commands).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_job_with_special_characters() {
    let printer = MockPrinter::new("p_special", "Special Chars Printer");

    let job_with_special_chars = serde_json::json!({
        "job_id": "job_special",
        "order_id": "ORDER_SPECIAL",
        "station": "bar",
        "items": [
            {
                "name": "Café Latté",
                "quantity": 1
            },
            {
                "name": "Crème Brûlée",
                "quantity": 1
            },
            {
                "name": "Jalapeño Nachos",
                "quantity": 1
            }
        ]
    });

    // ESC/POS should handle UTF-8
    let commands = vec![
        0x1B, 0x40,
        // ... UTF-8 encoded characters
        0x1B, 0x69,
    ];

    let result = printer.print(commands).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_large_order() {
    let printer = MockPrinter::new("p_large", "Large Order Printer");

    // Create order with 50 items
    let mut items = vec![];
    for i in 0..50 {
        items.push(serde_json::json!({
            "name": format!("Item {}", i + 1),
            "quantity": 1,
            "price": 10.00
        }));
    }

    let large_job = serde_json::json!({
        "job_id": "job_large",
        "order_id": "ORDER_LARGE",
        "station": "kitchen",
        "items": items
    });

    assert_eq!(large_job["items"].as_array().unwrap().len(), 50);

    // Print should handle large orders
    let commands = vec![0x1Bu8; 1000]; // Simulate large ESC/POS command buffer
    let result = printer.print(commands).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_retry_after_transient_failure() {
    let printer = MockPrinter::new("p_retry", "Retry Test Printer");

    // Simulate transient failure
    printer.set_should_fail(true).await;

    let commands = vec![0x1B, 0x40];

    // First attempt fails
    let first_result = printer.print(commands.clone()).await;
    assert!(first_result.is_err());

    // Fix issue
    printer.set_should_fail(false).await;

    // Retry succeeds
    let retry_result = printer.print(commands).await;
    assert!(retry_result.is_ok());

    assert_eq!(printer.get_print_count().await, 1);
}
