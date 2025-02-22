#![cfg(feature = "client")]

use serde_json::{json, Value};
use url::Url;
use sovran_mcp::{
    client::McpClient,
    client::transport::StdioTransport,
    types::{McpError, ToolResponseContent},
};
use sovran_mcp::types::{LogLevel, NotificationHandler, NotificationMethod};

#[test]
fn test_macho_integration() -> Result<(), McpError> {
    // This spawns our example and handles stdin/stdout
    let transport = StdioTransport::new(
        "cargo",
        &["run", "--example", "macho-mcp", "--features", "server"],
    )?;

    struct MachoNotifications;
    impl NotificationHandler for MachoNotifications {
        fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
            println!("Got resource update: {}", uri);
            Ok(())
        }

        fn handle_log_message(&self, level: &LogLevel, data: &Value, logger: &Option<String>) {
            println!("Got log message: {:?}", data);
        }

        fn handle_progress_update(&self, token: &String, progress: &f64, total: &Option<f64>) {
            println!("Got progress update: {}/{}", progress, total.unwrap_or(0.0));
        }

        fn handle_initialized(&self) {
            println!("Got initialized!");
        }

        fn handle_list_changed(&self, method: &NotificationMethod) {
            println!("Got list changed: {:?}", method);
        }
    }

    let mut client = McpClient::new(transport, None, Some(Box::new(MachoNotifications)));
    client.start()?;

    // Now we're actually talking to our macho server!
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
