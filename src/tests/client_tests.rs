#[cfg(test)]
mod tests {
    use crate::{client::transport::StdioTransport, types::*, McpClient};
    use base64::Engine;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tracing::{debug, info};
    use url::Url;

    // Helper function used by multiple tests
    fn create_test_client() -> Result<McpClient<StdioTransport>, McpError> {
        let transport =
            StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
        let mut client = McpClient::new(transport, None, None);
        client.start()?;
        Ok(client)
    }

    #[test]
    fn test_fetch_compatibility() -> Result<(), McpError> {
        let transport = StdioTransport::new("uvx", &["mcp-server-fetch"])?;
        let mut client = McpClient::new(transport, None, None);
        client.start()?;

        println!("Attempting list_tools...");
        let tools = client.list_tools()?;

        assert_eq!(tools.tools.is_empty(), false);
        client.stop()?;
        Ok(())
    }

    #[test]
    fn test_puppeteer_compatibility() -> Result<(), McpError> {
        // Handler for LLM completion requests
        struct MySamplingHandler;
        impl SamplingHandler for MySamplingHandler {
            fn handle_message(
                &self,
                _request: CreateMessageRequest,
            ) -> Result<CreateMessageResponse, McpError> {
                // Process the completion request and return response
                Ok(CreateMessageResponse {
                    content: MessageContent::Text(TextContent {
                        text: "AI response".to_string(),
                    }),
                    model: "test-model".to_string(),
                    role: Role::Assistant,
                    stop_reason: Some("complete".to_string()),
                    meta: None,
                })
            }
        }

        // Handler for server notifications
        struct MyNotificationHandler;
        impl NotificationHandler for MyNotificationHandler {
            fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
                println!("Resource updated: {}", uri);
                // Let's try to read the resource when it updates
                Ok(())
            }
            fn handle_initialized(&self) {
                println!("Server initialized!");
            }
            fn handle_log_message(&self, level: &LogLevel, data: &Value, logger: &Option<String>) {
                println!("Log message: {:?} {:?} {:?}", level, data, logger);
            }
            fn handle_progress_update(&self, token: &String, progress: &f64, total: &Option<f64>) {
                println!("Progress update: {} {} {:?}", token, progress, total);
            }
            fn handle_list_changed(&self, method: &NotificationMethod) {
                println!("List changed: {:?}", method);
            }
        }

        let transport =
            StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-puppeteer"])?;
        let mut client = McpClient::new(
            transport,
            Some(Box::new(MySamplingHandler)),
            Some(Box::new(MyNotificationHandler)),
        );

        client.start()?;

        println!("Attempting list_tools...");
        let tools = client.list_tools()?;
        println!("Tools: {}", serde_json::to_string_pretty(&tools)?);

        assert_eq!(tools.tools.is_empty(), false);

        let response = client.call_tool(
            "puppeteer_navigate".to_string(),
            Some(serde_json::json!({"url": "https://osnews.com"})),
        );
        if response.is_err() {
            println!("Error: {}", response.unwrap_err());
        } else {
            println!(
                "Response: {}",
                serde_json::to_string_pretty(&response.unwrap())?
            );
        }

        let response = client.call_tool(
            "puppeteer_screenshot".to_string(),
            Some(serde_json::json!({"name": "osnews_screenshot"})),
        );
        if response.is_err() {
            println!("Error: {}", response.unwrap_err());
        } else {
            println!(
                "Response: {}",
                serde_json::to_string_pretty(&response.unwrap())?
            );
        }

        let result = client.read_resource(&Url::parse("console://logs")?)?;
        println!("Logs: {}", serde_json::to_string_pretty(&result)?);

        Ok(())
    }

    #[test]
    fn test_prompt_operations() -> Result<(), McpError> {
        println!("Starting prompt test");
        let mut client = create_test_client()?;
        println!("Client created");

        // Test prompts/list
        let prompts = client.list_prompts()?;
        assert_eq!(prompts.prompts.len(), 2);

        // Verify simple_prompt
        let simple_prompt = prompts
            .prompts
            .iter()
            .find(|p| p.name == "simple_prompt")
            .expect("simple_prompt should exist");
        assert_eq!(
            simple_prompt.description.as_deref(),
            Some("A prompt without arguments")
        );
        assert!(
            simple_prompt.arguments.is_none()
                || simple_prompt.arguments.as_ref().unwrap().is_empty()
        );

        // Verify complex_prompt
        let complex_prompt = prompts
            .prompts
            .iter()
            .find(|p| p.name == "complex_prompt")
            .expect("complex_prompt should exist");
        assert_eq!(
            complex_prompt.description.as_deref(),
            Some("A prompt with arguments")
        );

        // Test getting simple prompt
        let simple = client.get_prompt("simple_prompt".to_string(), None)?;
        assert_eq!(simple.messages.len(), 1);
        assert!(simple.messages[0].content.is_text());
        assert_eq!(
            simple.messages[0].content.text_content(),
            Some("This is a simple prompt without arguments.")
        );
        assert_eq!(simple.messages[0].role, Role::User);

        // Test getting complex prompt with arguments
        let mut args = HashMap::new();
        args.insert("temperature".to_string(), "0.7".to_string());
        args.insert("style".to_string(), "formal".to_string());

        let complex = client.get_prompt("complex_prompt".to_string(), Some(args))?;
        assert_eq!(complex.messages.len(), 3);

        // Verify first message (User)
        assert!(complex.messages[0].content.is_text());
        assert_eq!(complex.messages[0].role, Role::User);
        assert_eq!(
            complex.messages[0].content.text_content(),
            Some("This is a complex prompt with arguments: temperature=0.7, style=formal")
        );

        // Verify second message (Assistant)
        assert!(complex.messages[1].content.is_text());
        assert_eq!(complex.messages[1].role, Role::Assistant);
        assert_eq!(
            complex.messages[1].content.text_content(),
            Some("I understand. You've provided a complex prompt with temperature and style arguments. How would you like me to proceed?")
        );

        // Verify third message (User with image)
        assert!(complex.messages[2].content.is_image());
        assert_eq!(complex.messages[2].role, Role::User);
        if let Some((_, mime)) = complex.messages[2].content.image_content() {
            assert_eq!(mime, "image/png");
        } else {
            panic!("Expected image content");
        }

        println!("Test complete, stopping client");
        client.stop()?;
        println!("Client stopped");
        println!("Prompt test complete");
        Ok(())
    }

    #[test]
    fn test_resource_operations() -> Result<(), McpError> {
        let mut client = create_test_client()?;

        // List resources
        let resources = client.list_resources()?;
        assert!(
            !resources.resources.is_empty(),
            "Should have some resources"
        );

        // Find a text resource (odd numbered) and binary resource (even numbered)
        let text_resource = resources
            .resources
            .iter()
            .find(|r| r.uri.as_str().ends_with("1"))
            .expect("Should have resource #1");

        let binary_resource = resources
            .resources
            .iter()
            .find(|r| r.uri.as_str().ends_with("2"))
            .expect("Should have resource #2");

        // Verify text resource
        assert_eq!(text_resource.mime_type.as_deref(), Some("text/plain"));
        let text_content = client.read_resource(&text_resource.uri)?;
        assert_eq!(text_content.contents.len(), 1);
        match &text_content.contents[0] {
            ResourceContent::Text(t) => {
                assert!(t.text.contains("This is a plaintext resource"));
                assert_eq!(t.uri, text_resource.uri);
                assert_eq!(t.mime_type.as_deref(), Some("text/plain"));
            }
            ResourceContent::Blob(_) => panic!("Expected text content for odd-numbered resource"),
        }

        // Verify binary resource
        assert_eq!(
            binary_resource.mime_type.as_deref(),
            Some("application/octet-stream")
        );
        let binary_content = client.read_resource(&binary_resource.uri)?;
        assert_eq!(binary_content.contents.len(), 1);
        match &binary_content.contents[0] {
            ResourceContent::Blob(b) => {
                assert_eq!(b.uri, binary_resource.uri);
                assert!(!b.blob.is_empty());
                assert_eq!(b.mime_type.as_deref(), Some("application/octet-stream"));

                // Verify base64 content
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(&b.blob)
                    .map_err(|e| McpError::Other(format!("Base64 decode error: {}", e)))?;
                let decoded_str = String::from_utf8(decoded)
                    .map_err(|e| McpError::Other(format!("UTF-8 decode error: {}", e)))?;
                assert!(decoded_str.contains("This is a base64 blob"));
            }
            ResourceContent::Text(_) => {
                panic!("Expected binary content for even-numbered resource")
            }
        }

        client.stop()?;
        Ok(())
    }

    #[test]
    fn test_initialization() -> Result<(), McpError> {
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
    fn test_capability_checks() -> Result<(), McpError> {
        let client = create_test_client()?;

        // Basic capability checks - these all exist in the response (though empty)
        assert!(client.supports_tools());
        assert!(client.supports_prompts());
        assert!(client.supports_resources());
        assert!(client.supports_logging());

        // Resources capabilities
        assert!(client.supports_resource_subscription()); // true because "subscribe": true
        assert!(!client.supports_resource_list_changed()); // false because not present

        // Prompts capabilities - none of the sub-capabilities are present
        assert!(!client.supports_prompt_list_changed()); // false because not present

        Ok(())
    }

    #[test]
    fn test_tools_operations() -> Result<(), McpError> {
        let client = create_test_client()?;

        // Test list_tools
        let tools = client.list_tools()?;
        assert!(!tools.tools.is_empty());

        // Find the echo tool specifically
        let echo_tool = tools
            .tools
            .iter()
            .find(|t| t.name == "echo")
            .expect("Echo tool should exist");

        // Test call_tool with proper parameters
        let response = client.call_tool(
            echo_tool.name.clone(),
            Some(serde_json::json!({
                "message": "Hello, MCP!"
            })),
        )?;

        // Verify the response
        assert!(!response.content.is_empty());
        if let Some(ToolResponseContent::Text { text }) = response.content.first() {
            assert_eq!(text, "Echo: Hello, MCP!"); // Server prepends "Echo: " to the message
        } else {
            panic!("Expected text response from echo tool");
        }

        // Let's also test the add tool
        let add_tool = tools
            .tools
            .iter()
            .find(|t| t.name == "add")
            .expect("Add tool should exist");

        let response = client.call_tool(
            add_tool.name.clone(),
            Some(serde_json::json!({
                "a": 2,
                "b": 3
            })),
        )?;

        // Verify the addition result
        if let Some(ToolResponseContent::Text { text }) = response.content.first() {
            assert_eq!(text, "The sum of 2 and 3 is 5."); // Server returns a formatted message
        } else {
            panic!("Expected text response from add tool");
        }

        Ok(())
    }

    #[test]
    fn test_quick_prompt_operations() -> Result<(), McpError> {
        let client = create_test_client()?;

        // Test list_prompts
        let prompts = client.list_prompts()?;
        assert!(!prompts.prompts.is_empty());

        // Test get_prompt with simple_prompt (we know it exists from previous tests)
        let simple = client.get_prompt("simple_prompt".to_string(), None)?;
        assert!(!simple.messages.is_empty());

        // Test get_prompt with arguments
        let mut args = HashMap::new();
        args.insert("temperature".to_string(), "0.7".to_string());
        let complex = client.get_prompt("complex_prompt".to_string(), Some(args))?;
        assert!(!complex.messages.is_empty());

        Ok(())
    }

    #[test]
    fn test_quick_resource_operations() -> Result<(), McpError> {
        let client = create_test_client()?;

        // Test list_resources
        let resources = client.list_resources()?;
        assert!(!resources.resources.is_empty());

        // Find a text resource and test read_resource
        if let Some(text_resource) = resources
            .resources
            .iter()
            .find(|r| r.uri.as_str().ends_with("1"))
        {
            let content = client.read_resource(&text_resource.uri)?;
            assert!(!content.contents.is_empty());

            // Verify it's text content
            match &content.contents[0] {
                ResourceContent::Text(t) => {
                    assert!(t.text.contains("This is a plaintext resource"));
                }
                _ => panic!("Expected text content"),
            }
        }

        Ok(())
    }

    #[test]
    fn test_error_handling() -> Result<(), McpError> {
        let client = create_test_client()?;

        // Test invalid prompt name
        let result = client.get_prompt("nonexistent_prompt".to_string(), None);
        assert!(result.is_err());

        // Test invalid tool name
        let result = client.call_tool("nonexistent_tool".to_string(), None);
        assert!(result.is_err());

        // Test invalid resource URI
        let invalid_url = Url::parse("file:///nonexistent")?;
        let result = client.read_resource(&invalid_url);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_resource_subscription() -> Result<(), McpError> {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();

        info!("Starting subscription test");

        // Set up notification tracking
        let updates = Arc::new(Mutex::new(Vec::new()));
        let updates_clone = updates.clone();

        struct TestNotificationHandler {
            updates: Arc<Mutex<Vec<String>>>,
        }

        impl NotificationHandler for TestNotificationHandler {
            fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
                debug!("Resource update notification received for: {}", uri);
                let mut updates = self.updates.lock().unwrap();
                updates.push(uri.to_string());
                debug!("Added notification. Current count: {}", updates.len());
                Ok(())
            }

            fn handle_log_message(
                &self,
                _level: &LogLevel,
                _data: &Value,
                _logger: &Option<String>,
            ) {
                todo!()
            }

            fn handle_progress_update(
                &self,
                _token: &String,
                _progress: &f64,
                _total: &Option<f64>,
            ) {
                todo!()
            }

            fn handle_initialized(&self) {
                todo!()
            }

            fn handle_list_changed(&self, _method: &NotificationMethod) {
                todo!()
            }
        }

        // Create client with handlers
        let notification_handler = Box::new(TestNotificationHandler {
            updates: updates_clone,
        });
        let sampling_handler = Box::new(TestSamplingHandler {});
        let mut client = McpClient::new(
            StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
            Some(sampling_handler),
            Some(notification_handler),
        );

        // Start client and verify subscription support
        client.start()?;
        assert!(client.supports_resource_subscription());

        // Get a resource to subscribe to
        let resources = client.list_resources()?;
        let test_resource = resources
            .resources
            .iter()
            .find(|r| r.uri.as_str().ends_with("/1"))
            .expect("Resource #1 should exist");

        // Subscribe to the resource
        info!("Subscribing to resource...");
        client.subscribe(&test_resource.uri)?;

        // Wait for notifications
        info!("Waiting for updates...");
        std::thread::sleep(std::time::Duration::from_secs(6));

        // Verify we got notifications
        let received_updates = updates.lock().unwrap();
        assert!(
            !received_updates.is_empty(),
            "Should have received notifications"
        );
        debug!("Received {} notifications", received_updates.len());

        // Verify we can read the resource after notification
        info!("Reading resource after notification...");
        let content = client.read_resource(&test_resource.uri)?;
        assert!(
            !content.contents.is_empty(),
            "Should be able to read resource"
        );

        // Cleanup
        info!("Unsubscribing...");
        client.unsubscribe(&test_resource.uri)?;
        client.stop()?;
        info!("Test complete");
        Ok(())
    }

    struct TestSamplingHandler;

    impl SamplingHandler for TestSamplingHandler {
        fn handle_message(
            &self,
            request: CreateMessageRequest,
        ) -> Result<CreateMessageResponse, McpError> {
            debug!(?request, "Sampling handler called");

            // Extract URI from the request text
            let uri = request
                .messages
                .first()
                .and_then(|msg| {
                    if let MessageContent::Text(t) = &msg.content {
                        t.text.split("Resource ").nth(1)?.split(" context").next()
                    } else {
                        None
                    }
                })
                .unwrap_or("unknown");

            // Return modified response
            let meta = HashMap::new();
            let response = CreateMessageResponse {
                content: MessageContent::Text(TextContent {
                    text: format!("Resource {} context noted.", uri),
                }),
                model: "test-model".to_string(),
                role: Role::Assistant,
                stop_reason: Some("complete".to_string()),
                meta: Some(meta),
            };
            debug!(?response, "Sampling handler returning response");
            Ok(response)
        }
    }
}
