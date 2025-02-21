#![cfg(feature = "client")]

use serde_json::json;
use sovran_mcp::{
    client::McpClient,
    transport::StdioTransport,
    types::{McpError, ToolResponseContent},
};

#[test]
fn test_macho_integration() -> Result<(), McpError> {
    // This spawns our example and handles stdin/stdout
    let transport = StdioTransport::new("cargo", &["run", "--example", "macho-mcp", "--features", "server"])?;

    let mut client = McpClient::new(transport, None, None);
    client.start()?;

    // Now we're actually talking to our macho server!
    let response = client.call_tool(
        "elbow-drop".into(),
        Some(json!({
            "target": "Java",
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
