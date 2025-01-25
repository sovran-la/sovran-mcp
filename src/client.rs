use crate::transport::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, TransportControl};
use crate::types::*;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Sender};
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use url::Url;
use crate::commands::{CallTool, GetPrompt, Initialize, ListPrompts, ListResources, ListTools, McpCommand, ReadResource, Subscribe, Unsubscribe};
use tracing::{debug, info, warn};

pub struct Client<T: TransportControl + 'static> {
    transport: Arc<T>,
    request_id: Arc<AtomicU64>,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
    listener_handle: Option<Arc<Mutex<Option<JoinHandle<()>>>>>,
    sampling_handler: Option<Box<dyn SamplingHandler + Send>>,
    stop_flag: Arc<AtomicBool>,
    server_info: Arc<Mutex<Option<InitializeResponse>>>,
}

impl<T: TransportControl + 'static> Drop for Client<T> {
    fn drop(&mut self) {
        if !self.stop_flag.load(Ordering::SeqCst) {
            // We don't want to panic in drop, so we ignore any errors
            let _ = self.stop();
        }
    }
}

impl<T: TransportControl + 'static> Client<T> {
    pub fn new(transport: T, sampling_handler: Option<Box<dyn SamplingHandler + Send>>) -> Self {
        Self {
            transport: Arc::new(transport),
            request_id: Arc::new(AtomicU64::new(0)),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            listener_handle: None,
            sampling_handler,
            stop_flag: Arc::new(AtomicBool::new(false)),
            server_info: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&mut self) -> Result<()> {
        self.transport.open()?;

        let transport = self.transport.clone();
        let pending_requests = self.pending_requests.clone();
        let sampling_handler = self.sampling_handler.take();

        // Create a thread handle wrapper
        let handle_wrapper = Arc::new(Mutex::new(None));

        let stop_flag = self.stop_flag.clone();
        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match transport.receive() {
                    Ok(message) => {
                        debug!("Received message: {:?}", message);
                        match message {
                            JsonRpcMessage::Response(response) => {
                                let mut pending = pending_requests.lock().unwrap();
                                if let Some(sender) = pending.remove(&response.id) {
                                    let _ = sender.send(response);
                                }
                            },
                            JsonRpcMessage::Request(request) => {
                                if let Some(handler) = &sampling_handler {
                                    if request.method == "sampling/createMessage" {
                                        if let Ok(params) = serde_json::from_value(request.params.unwrap_or(serde_json::Value::Null)) {
                                            if let Ok(result) = handler.handle_message(params) {
                                                let response = JsonRpcResponse {
                                                    id: request.id,
                                                    result: Some(serde_json::to_value(result).unwrap()),
                                                    error: None,
                                                    jsonrpc: Default::default(),
                                                };
                                                let _ = transport.send(&JsonRpcMessage::Response(response));
                                            }
                                        }
                                    }
                                }
                            },
                            JsonRpcMessage::Notification(_) => {
                                // Handle notifications if needed
                            }
                        }
                    },
                    Err(_) => {
                        // Exit loop on error
                        println!("Transport error");
                        break;
                    }
                }
            }
        });

        // Store the handle
        *handle_wrapper.lock().unwrap() = Some(handle);
        self.listener_handle = Some(handle_wrapper);

        // Initialize the connection
        self.initialize()?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        // Signal the thread to stop
        self.stop_flag.store(true, Ordering::SeqCst);

        // send a dummy command to generate an error which will
        // unblock any pending receive.
        let r = self.request("__internal/server_stop", None)?;

        // Kill the transport
        self.transport.close()?;

        // Join the thread if it hasn't detached
        if let Some(wrapper) = self.listener_handle.take() {
            if let Some(handle) = wrapper.lock().unwrap().take() {
                //let _ = handle.join(); // Wait for thread termination
                drop(handle);
            }
        }

        Ok(())
    }

    fn request(&self, method: &str, params: Option<serde_json::Value>) -> Result<JsonRpcResponse> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();

        // Store the sender
        {
            let mut pending = self.pending_requests.lock().unwrap();
            pending.insert(id, tx);
        }

        // Send the request
        let request = JsonRpcRequest {
            id,
            method: method.to_string(),
            params,
            jsonrpc: Default::default(),
        };
        self.transport.send(&JsonRpcMessage::Request(request))?;

        // Wait for response
        rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive response: {}", e))
    }

    pub fn execute<C: McpCommand>(&self, request: C::Request) -> Result<C::Response> {
        let response = self.request(
            C::COMMAND,
            Some(serde_json::to_value(request)?)
        )?;

        if let Some(error) = response.error {
            return Err(anyhow::anyhow!("{} failed: {}", C::COMMAND, error.message));
        }

        let result = response.result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        Ok(serde_json::from_value(result)?)
    }

    fn initialize(&self) -> Result<&Self> {
        let request = InitializeRequest {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "mcp-simple".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let response = self.execute::<Initialize>(request)?;

        // Store the response
        *self.server_info.lock().unwrap() = Some(response);

        Ok(self)
    }

    pub fn server_capabilities(&self) -> Result<ServerCapabilities> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.capabilities.clone())
            .ok_or_else(|| anyhow::anyhow!("Client not initialized"))
    }

    pub fn server_version(&self) -> Result<String> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.protocol_version.clone())
            .ok_or_else(|| anyhow::anyhow!("Client not initialized"))
    }

    pub fn has_capability<F>(&self, check: F) -> bool
    where
        F: FnOnce(&ServerCapabilities) -> bool
    {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| check(&info.capabilities))
            .unwrap_or(false)
    }

    // Capability checking methods
    pub fn supports_resources(&self) -> bool {
        self.has_capability(|caps| caps.resources.is_some())
    }

    pub fn supports_prompts(&self) -> bool {
        self.has_capability(|caps| caps.prompts.is_some())
    }

    pub fn supports_tools(&self) -> bool {
        self.has_capability(|caps| caps.tools.is_some())
    }

    pub fn supports_logging(&self) -> bool {
        self.has_capability(|caps| caps.logging.is_some())
    }

    pub fn supports_resource_subscription(&self) -> bool {
        self.has_capability(|caps| {
            caps.resources
                .as_ref()
                .and_then(|r| r.subscribe)
                .unwrap_or(false)
        })
    }

    pub fn supports_resource_list_changed(&self) -> bool {
        self.has_capability(|caps| {
            caps.resources
                .as_ref()
                .and_then(|r| r.list_changed)
                .unwrap_or(false)
        })
    }

    pub fn supports_prompt_list_changed(&self) -> bool {
        self.has_capability(|caps| {
            caps.prompts
                .as_ref()
                .and_then(|p| p.list_changed)
                .unwrap_or(false)
        })
    }

    pub fn supports_experimental_feature(&self, feature: &str) -> bool {
        self.has_capability(|caps| {
            caps.experimental
                .as_ref()
                .and_then(|e| e.get(feature))
                .is_some()
        })
    }

    pub fn list_tools(&self) -> Result<ListToolsResponse> {
        if !self.supports_tools() {
            return Err(anyhow::anyhow!("Server does not support tools"));
        }
        let request = ListToolsRequest {
            cursor: None,
            meta: None,
        };
        self.execute::<ListTools>(request)
    }

    pub fn call_tool(&self, name: String, arguments: Option<serde_json::Value>) -> Result<CallToolResponse> {
        if !self.supports_tools() {
            return Err(anyhow::anyhow!("Server does not support tools"));
        }
        let request = CallToolRequest {
            name,
            arguments,
            meta: None,
        };
        self.execute::<CallTool>(request)
    }

    pub fn list_prompts(&self) -> Result<ListPromptsResponse> {
        if !self.supports_prompts() {
            return Err(anyhow::anyhow!("Server does not support prompts"));
        }
        let request = ListPromptsRequest {
            cursor: None,
            meta: None,
        };
        self.execute::<ListPrompts>(request)
    }

    pub fn get_prompt(&self, name: String, arguments: Option<HashMap<String, String>>) -> Result<GetPromptResponse> {
        if !self.supports_prompts() {
            return Err(anyhow::anyhow!("Server does not support prompts"));
        }
        let request = GetPromptRequest { name, arguments };
        self.execute::<GetPrompt>(request)
    }

    pub fn list_resources(&self) -> Result<ListResourcesResponse> {
        if !self.supports_resources() {
            return Err(anyhow::anyhow!("Server does not support resources"));
        }
        let request = ListResourcesRequest { cursor: None };
        self.execute::<ListResources>(request)
    }

    pub fn read_resource(&self, uri: &Url) -> Result<ReadResourceResponse> {
        if !self.supports_resources() {
            return Err(anyhow::anyhow!("Server does not support resources"));
        }
        let request = ReadResourceRequest { uri: uri.clone() };
        self.execute::<ReadResource>(request)
    }

    pub fn subscribe(&self, uri: &Url) -> Result<EmptyResult> {
        if !self.supports_resource_subscription() {
            return Err(anyhow::anyhow!("Server does not support resource subscription"));
        }
        let request = SubscribeRequest { uri: uri.clone() };
        self.execute::<Subscribe>(request)
    }

    pub fn unsubscribe(&self, uri: &Url) -> Result<EmptyResult> {
        if !self.supports_resource_subscription() {
            return Err(anyhow::anyhow!("Server does not support resource subscription"));
        }
        let request = UnsubscribeRequest { uri: uri.clone() };
        self.execute::<Unsubscribe>(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::ClientStdioTransport;

    fn create_test_client() -> Result<Client<ClientStdioTransport>> {
        let transport = ClientStdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
        let mut client = Client::new(transport, None);
        client.start()?;
        Ok(client)
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

        // Verify complex_prompt
        let complex_prompt = prompts.prompts.iter()
            .find(|p| p.name == "complex_prompt")
            .expect("complex_prompt should exist");
        assert_eq!(complex_prompt.description.as_deref(), Some("A prompt with arguments"));

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

        client.stop()?;
        Ok(())
    }

    #[test]
    fn test_resource_operations() -> Result<()> {
        let mut client = create_test_client()?;

        // List resources
        let resources = client.list_resources()?;
        assert!(!resources.resources.is_empty(), "Should have some resources");

        // Find a text resource (odd numbered) and binary resource (even numbered)
        let text_resource = resources.resources.iter()
            .find(|r| r.uri.as_str().ends_with("1"))
            .expect("Should have resource #1");

        let binary_resource = resources.resources.iter()
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
            },
            ResourceContent::Blob(_) => panic!("Expected text content for odd-numbered resource"),
        }

        // Verify binary resource
        assert_eq!(binary_resource.mime_type.as_deref(), Some("application/octet-stream"));
        let binary_content = client.read_resource(&binary_resource.uri)?;
        assert_eq!(binary_content.contents.len(), 1);
        match &binary_content.contents[0] {
            ResourceContent::Blob(b) => {
                assert_eq!(b.uri, binary_resource.uri);
                assert!(!b.blob.is_empty());
                assert_eq!(b.mime_type.as_deref(), Some("application/octet-stream"));

                // Verify base64 content
                let decoded = base64::decode(&b.blob)?;
                let decoded_str = String::from_utf8(decoded)?;
                assert!(decoded_str.contains("This is a base64 blob"));
            },
            ResourceContent::Text(_) => panic!("Expected binary content for even-numbered resource"),
        }

        client.stop()?;
        Ok(())
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
    fn test_tools_operations() -> Result<()> {
        let client = create_test_client()?;

        // Test list_tools
        let tools = client.list_tools()?;
        assert!(!tools.tools.is_empty());

        // Find the echo tool specifically
        let echo_tool = tools.tools.iter()
            .find(|t| t.name == "echo")
            .expect("Echo tool should exist");

        // Test call_tool with proper parameters
        let response = client.call_tool(
            echo_tool.name.clone(),
            Some(serde_json::json!({
            "message": "Hello, MCP!"
        }))
        )?;

        // Verify the response
        assert!(!response.content.is_empty());
        if let Some(ToolResponseContent::Text { text }) = response.content.first() {
            assert_eq!(text, "Echo: Hello, MCP!"); // Server prepends "Echo: " to the message
        } else {
            panic!("Expected text response from echo tool");
        }

        // Let's also test the add tool
        let add_tool = tools.tools.iter()
            .find(|t| t.name == "add")
            .expect("Add tool should exist");

        let response = client.call_tool(
            add_tool.name.clone(),
            Some(serde_json::json!({
            "a": 2,
            "b": 3
        }))
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
    fn test_quick_prompt_operations() -> Result<()> {
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
    fn test_quick_resource_operations() -> Result<()> {
        let client = create_test_client()?;

        // Test list_resources
        let resources = client.list_resources()?;
        assert!(!resources.resources.is_empty());

        // Find a text resource and test read_resource
        if let Some(text_resource) = resources.resources.iter()
            .find(|r| r.uri.as_str().ends_with("1"))
        {
            let content = client.read_resource(&text_resource.uri)?;
            assert!(!content.contents.is_empty());

            // Verify it's text content
            match &content.contents[0] {
                ResourceContent::Text(t) => {
                    assert!(t.text.contains("This is a plaintext resource"));
                },
                _ => panic!("Expected text content"),
            }
        }

        Ok(())
    }

    #[test]
    fn test_error_handling() -> Result<()> {
        let client = create_test_client()?;

        // Test invalid prompt name
        let result = client.get_prompt("nonexistent_prompt".to_string(), None);
        assert!(result.is_err());

        // Test invalid tool name
        let result = client.call_tool(
            "nonexistent_tool".to_string(),
            None
        );
        assert!(result.is_err());

        // Test invalid resource URI
        let invalid_url = Url::parse("file:///nonexistent")?;
        let result = client.read_resource(&invalid_url);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_resource_subscription() -> Result<()> {
        // Initialize with debug level
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();

        info!("Starting subscription test");

        // Create a basic sampling handler that just returns canned responses
        let sampling_handler = Box::new(TestSamplingHandler {});
        debug!("Creating client...");
        let mut client = Client::new(
            ClientStdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
            Some(sampling_handler)
        );

        debug!("Starting client...");
        client.start()?;
        info!("Client started");

        // First verify the server supports subscriptions
        assert!(client.supports_resource_subscription(),
                "Server should support resource subscriptions");
        debug!("Subscription support verified");

        // Get a resource to subscribe to (let's use #1 since it's odd/binary)
        debug!("Listing resources...");
        let resources = client.list_resources()?;
        let test_resource = resources.resources.iter()
            .find(|r| r.uri.as_str().ends_with("/1"))
            .expect("Resource #1 should exist");
        info!("Found test resource: {}", test_resource.uri);

        // Subscribe to the resource
        info!("Subscribing to resource...");
        let sub_result = client.subscribe(&test_resource.uri)?;
        debug!("Subscription complete");

        // Get initial content
        debug!("Reading initial content...");
        let initial_content = client.read_resource(&test_resource.uri)?;
        debug!("Initial content read");

        // Give the server a moment to potentially send an update
        info!("Waiting for update...");
        std::thread::sleep(std::time::Duration::from_secs(6));
        debug!("Wait complete");

        // Read again - content should have changed
        debug!("Reading updated content...");
        let updated_content = client.read_resource(&test_resource.uri)?;
        debug!("Updated content read");

        // Compare contents - they should be different after 5 seconds
        match (&initial_content.contents[0], &updated_content.contents[0]) {
            (ResourceContent::Blob(initial), ResourceContent::Blob(updated)) => {
                debug!("Comparing content...");
                assert_ne!(initial.blob, updated.blob,
                           "Content should have changed after update");
                debug!("Content comparison complete");
            },
            _ => panic!("Expected blob content for resource #1"),
        }

        // Unsubscribe
        info!("Unsubscribing...");
        let unsub_result = client.unsubscribe(&test_resource.uri)?;
        debug!("Unsubscribe complete");

        debug!("Stopping client...");
        client.stop()?;
        info!("Test complete");
        Ok(())
    }

    struct TestSamplingHandler;

    impl SamplingHandler for TestSamplingHandler {
        fn handle_message(&self, request: CreateMessageRequest) -> Result<CreateMessageResponse> {
            debug!(?request, "Sampling handler called");
            // Return a canned response
            let response = CreateMessageResponse {
                content: MessageContent::Text(TextContent {
                    text: "Subscription noted.".to_string()
                }),
                model: "test-model".to_string(),
                role: Role::Assistant,
                stop_reason: None,
                meta: None,
            };
            debug!(?response, "Sampling handler returning response");
            Ok(response)
        }
    }
}
