#![cfg(feature = "client")]

use serde_json::{json, Value};
use url::Url;
use sovran_mcp::{
    client::McpClient,
    client::transport::StdioTransport,
    types::{McpError, ToolResponseContent},
};
use sovran_mcp::types::{LogLevel, NotificationHandler, NotificationMethod};
use std::sync::{Arc, Mutex};

// Shared notification handler
struct MachoNotifications {
    resource_updates: Arc<Mutex<Vec<String>>>,
}

impl NotificationHandler for MachoNotifications {
    fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
        println!("Got resource update: {}", uri);
        self.resource_updates.lock().unwrap().push(uri.to_string());
        Ok(())
    }

    fn handle_log_message(&self, level: &LogLevel, data: &Value, logger: &Option<String>) {
        println!("Got log message: {:?}, level: {:?}, logger: {:?}", data, level, logger);
    }

    fn handle_progress_update(&self, token: &String, progress: &f64, total: &Option<f64>) {
        println!("Got progress update: {}/{}, token: {}", progress, total.unwrap_or(0.0), token);
    }

    fn handle_initialized(&self) {
        println!("Got initialized!");
    }

    fn handle_list_changed(&self, method: &NotificationMethod) {
        println!("Got list changed: {:?}", method);
    }
}

// Helper function to create and initialize a client
fn create_client() -> Result<(McpClient<StdioTransport>, Arc<Mutex<Vec<String>>>), McpError> {
    let transport = StdioTransport::new(
        "cargo",
        &["run", "--example", "macho-mcp", "--features", "server"],
    )?;

    let resource_updates = Arc::new(Mutex::new(Vec::<String>::new()));
    let resource_updates_clone = resource_updates.clone();

    let mut client = McpClient::new(
        transport,
        None,
        Some(Box::new(MachoNotifications { resource_updates: resource_updates_clone }))
    );
    client.start()?;

    Ok((client, resource_updates))
}

#[test]
fn test_elbow_drop() -> Result<(), McpError> {
    let (client, _) = create_client()?;

    println!("===== TESTING ELBOW DROP =====");
    let response = client.call_tool(
        "elbow-drop".into(),
        Some(json!({
            "target": "Lil' Timmy",
            "intensity": 11
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got response: {}", text);
        assert!(text.contains("DROPPED"), "Should confirm the elbow drop!");
        assert!(text.contains("110 feet"), "Should be intense!");
    } else {
        panic!("Expected text response!");
    }

    Ok(())
}

#[test]
fn test_prepare_to_rumble() -> Result<(), McpError> {
    let (client, _) = create_client()?;

    println!("===== TESTING PREPARE TO RUMBLE =====");
    let response = client.call_tool(
        "prepare-to-rumble".into(),
        Some(json!({
            "opponent": "Hulk Hogan",
            "intensity": 7,
            "_meta": {
                "progressToken": "test-preparation-123"
            }
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got response: {}", text);
        assert!(text.contains("MACHO MAN IS READY"), "Should be ready to rumble!");
        assert!(text.contains("Hulk Hogan"), "Should mention the opponent!");
        assert!(text.contains("CREAM OF THE CROP"), "Should be appropriately macho!");
    } else {
        panic!("Expected text response!");
    }

    Ok(())
}

#[test]
fn test_championship_list() -> Result<(), McpError> {
    let (client, _) = create_client()?;

    println!("===== TESTING CHAMPIONSHIP LIST =====");
    let response = client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "list"
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got response: {}", text);
        assert!(text.contains("COLLECTION"), "Should list the collection!");
        assert!(text.contains("macho://belts/wwf-world-heavyweight-championship"), "Should have WWF belt!");
        assert!(text.contains("macho://belts/intercontinental-championship"), "Should have Intercontinental belt!");
    } else {
        panic!("Expected text response!");
    }

    Ok(())
}

#[test]
fn test_add_championship() -> Result<(), McpError> {
    let (client, _) = create_client()?;

    println!("===== TESTING ADD CHAMPIONSHIP =====");
    let response = client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "add",
            "name": "WCW World Heavyweight Championship",
            "weight_class": "Heavyweight",
            "signature_move": "Flying Elbow Drop"
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got response: {}", text);
        assert!(text.contains("WCW World Heavyweight Championship"), "Should confirm the new belt!");
        assert!(text.contains("NOW IN PLAY"), "Should confirm addition!");
    } else {
        panic!("Expected text response!");
    }

    // Verify the new belt is in the list
    let response = client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "list"
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got updated belt list: {}", text);
        assert!(text.contains("macho://belts/wcw-world-heavyweight-championship"), "Should have new WCW belt!");
    } else {
        panic!("Expected text response!");
    }

    Ok(())
}

#[test]
fn test_defend_championship() -> Result<(), McpError> {
    let (client, resource_updates) = create_client()?;

    println!("===== TESTING DEFEND CHAMPIONSHIP =====");
    let response = client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "defend",
            "belt_id": "wwf-world-heavyweight-championship",
            "defense_details": {
                "opponent": "Hulk Hogan",
                "signature_move": "Savage Elbow Drop"
            }
        })),
    )?;

    if let Some(ToolResponseContent::Text { text }) = response.content.first() {
        println!("Got response: {}", text);
        assert!(text.contains("DEFENDED"), "Should confirm the defense!");
        assert!(text.contains("372"), "Should increment defense count!"); // 371 + 1
        assert!(text.contains("Hulk Hogan"), "Should mention the opponent!");
        assert!(text.contains("Savage Elbow Drop"), "Should mention the signature move!");
    } else {
        panic!("Expected text response!");
    }

    // Verify resource update was received
    let updates = resource_updates.lock().unwrap();
    assert!(updates.iter().any(|u| u.contains("wwf-world-heavyweight-championship")),
            "Should have received update for defended WWF belt!");

    Ok(())
}

#[test]
fn test_resource_modifications_and_updates() -> Result<(), McpError> {
    let (client, resource_updates) = create_client()?;

    // Add a new belt
    client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "add",
            "name": "ECW Championship",
            "weight_class": "Extreme",
            "signature_move": "Savage DDT"
        })),
    )?;

    // Defend a belt
    client.call_tool(
        "championship-manager".into(),
        Some(json!({
            "action": "defend",
            "belt_id": "wwf-world-heavyweight-championship",
            "defense_details": {
                "opponent": "Randy Orton",
                "signature_move": "Macho Splash"
            }
        })),
    )?;

    // Verify resource updates
    let updates = resource_updates.lock().unwrap();
    assert!(updates.iter().any(|u| u.contains("ecw-championship")),
            "Should have received update for new ECW belt!");
    assert!(updates.iter().any(|u| u.contains("wwf-world-heavyweight-championship")),
            "Should have received update for defended WWF belt!");

    Ok(())
}