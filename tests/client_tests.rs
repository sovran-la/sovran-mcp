use mcp_sync::{
    Client,
    transport::ClientStdioTransport,
    types::*,
};

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use url::Url;
use tracing::{debug, info};

// Helper function used by multiple tests
fn create_test_client() -> Result<Client<ClientStdioTransport>> {
    let transport = ClientStdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
    let mut client = Client::new(transport, None, None);
    client.start()?;
    Ok(client)
}

#[test]
fn test_initialization() -> Result<()> {
    let client = create_test_client()?;

    // Check that we got server info
    let capabilities = client.server_capabilities()?;
    assert!(capabilities.tools.is_some());
    assert!(capabilities.prompts.is_some());
    assert!(capabilities.resources.is_some());

    // Verify version
    let version = client.server_version()?;
    assert!(!version.is_empty());

    Ok(())
}

#[test]
fn test_capability_checks() -> Result<()> {
    let client = create_test_client()?;

    // Basic capability checks
    assert!(client.supports_tools());
    assert!(client.supports_prompts());
    assert!(client.supports_resources());
    assert!(client.supports_logging());

    // Resources capabilities
    assert!(client.supports_resource_subscription());
    assert!(!client.supports_resource_list_changed());

    // Prompts capabilities
    assert!(!client.supports_prompt_list_changed());

    Ok(())
}

#[test]
fn test_prompt_operations() -> Result<()> {
    let mut client = create_test_client()?;

    // Test prompts/list
    let prompts = client.list_prompts()?;
    assert_eq!(prompts.prompts.len(), 2);

    // Verify simple_prompt
    let simple_prompt = prompts.prompts.iter()
        .find(|p| p.name == "simple_prompt")
        .expect("simple_prompt should exist");
    assert_eq!(simple_prompt.description.as_deref(), Some("A prompt without arguments"));
    assert!(simple_prompt.arguments.is_none() || simple_prompt.arguments.as_ref().unwrap().is_empty());

    // ... rest of the prompt tests ...
    client.stop()?;
    Ok(())
}

#[test]
fn test_quick_prompt_operations() -> Result<()> {
    let client = create_test_client()?;

    let prompts = client.list_prompts()?;
    assert!(!prompts.prompts.is_empty());

    let simple = client.get_prompt("simple_prompt".to_string(), None)?;
    assert!(!simple.messages.is_empty());

    let mut args = HashMap::new();
    args.insert("temperature".to_string(), "0.7".to_string());
    let complex = client.get_prompt("complex_prompt".to_string(), Some(args))?;
    assert!(!complex.messages.is_empty());

    Ok(())
}

#[test]
fn test_resource_operations() -> Result<()> {
    let mut client = create_test_client()?;
    // ... resource tests ...
    client.stop()?;
    Ok(())
}

#[test]
fn test_quick_resource_operations() -> Result<()> {
    let client = create_test_client()?;
    // ... quick resource tests ...
    Ok(())
}

#[test]
fn test_resource_subscription() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // ... subscription test ...
    Ok(())
}

#[test]
fn test_tools_operations() -> Result<()> {
    let client = create_test_client()?;
    // ... tools tests ...
    Ok(())
}

#[test]
fn test_error_handling() -> Result<()> {
    let client = create_test_client()?;
    // ... error handling tests ...
    Ok(())
}

// Test handlers at the bottom
struct TestNotificationHandler {
    updates: Arc<Mutex<Vec<String>>>,
}

impl NotificationHandler for TestNotificationHandler {
    fn handle_resource_update(&self, uri: &Url) -> Result<()> {
        debug!("Resource update notification received for: {}", uri);
        let mut updates = self.updates.lock().unwrap();
        updates.push(uri.to_string());
        debug!("Added notification. Current count: {}", updates.len());
        Ok(())
    }
}

struct TestSamplingHandler;

impl SamplingHandler for TestSamplingHandler {
    fn handle_message(&self, request: CreateMessageRequest) -> Result<CreateMessageResponse> {
        // ... sampling handler implementation ...
        Ok(CreateMessageResponse {
            content: MessageContent::Text(TextContent {
                text: format!("Resource {} context noted.", "test")
            }),
            model: "test-model".to_string(),
            role: Role::Assistant,
            stop_reason: Some("complete".to_string()),
            meta: Some(HashMap::new()),
        })
    }
}
