//! Client implementation for the Model Context Protocol (MCP).
//!
//! The MCP client provides a synchronous interface for interacting with MCP servers,
//! supporting operations like:
//! - Tool execution
//! - Prompt management
//! - Resource handling
//! - Server capability detection
//!
//! # Usage
//!
//! Basic usage with stdio transport:
//!
//! ```
//! use sovran_mcp::{McpClient, transport::StdioTransport};
//!
//! # fn main() -> Result<(), sovran_mcp::McpError> {
//! // Create and start client
//! let transport = StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
//! let mut client = McpClient::new(transport, None, None);
//! client.start()?;
//!
//! // Use MCP features
//! if client.supports_tools() {
//!     let tools = client.list_tools()?;
//!     println!("Available tools: {}", tools.tools.len());
//! }
//!
//! // Clean up
//! client.stop()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Error Handling
//!
//! The client uses `McpError` for error handling, which covers:
//! - Transport errors (I/O, connection issues)
//! - Protocol errors (JSON-RPC, serialization)
//! - Capability errors (unsupported features)
//! - Request timeouts
//! - Command failures
//!
//! # Thread Safety
//!
//! The client spawns a message handling thread for processing responses and notifications.
//! This thread is properly managed through the `start()` and `stop()` methods.
use crate::commands::{
    CallTool, GetPrompt, Initialize, ListPrompts, ListResources, ListTools, McpCommand,
    ReadResource, Subscribe, Unsubscribe,
};
use crate::messaging::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, MessageHandler,
};
use crate::transport::Transport;
use crate::types::*;
use crate::McpError;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Sender};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use tracing::{debug, warn};
use url::Url;

/// A client implementation of the Model Context Protocol (MCP).
///
/// The MCP client provides a synchronous interface for interacting with MCP servers,
/// allowing operations like tool execution, prompt management, and resource handling.
///
/// # Examples
///
/// ```
/// use sovran_mcp::{McpClient, transport::StdioTransport};
///
/// // Create a client using stdio transport
/// let transport = StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
/// let mut client = McpClient::new(transport, None, None);
///
/// // Start the client (initializes connection and protocol)
/// client.start()?;
///
/// // Use the client...
/// let tools = client.list_tools()?;
///
/// // Clean up when done
/// client.stop()?;
/// # Ok::<(), sovran_mcp::McpError>(())
/// ```
pub struct McpClient<T: Transport + 'static> {
    transport: Arc<T>,
    request_id: Arc<AtomicU64>,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
    listener_handle: Option<Arc<Mutex<Option<JoinHandle<()>>>>>,
    sampling_handler: Option<Arc<Box<dyn SamplingHandler + Send>>>,
    notification_handler: Option<Arc<Box<dyn NotificationHandler + Send>>>,
    stop_flag: Arc<AtomicBool>,
    server_info: Arc<Mutex<Option<InitializeResponse>>>,
}

impl<T: Transport + 'static> Drop for McpClient<T> {
    fn drop(&mut self) {
        if !self.stop_flag.load(Ordering::SeqCst) {
            // We don't want to panic in drop, so we ignore any errors
            let _ = self.stop();
        }
    }
}

impl<T: Transport + 'static> McpClient<T> {
    /// Creates a new MCP client with the specified transport and optional handlers.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport implementation to use for communication
    /// * `sampling_handler` - Optional handler for LLM completion requests from the server. This enables
    ///   the server to request AI completions through the client while maintaining security boundaries.
    /// * `notification_handler` - Optional handler for receiving one-way messages from the server,
    ///   such as resource updates.
    ///
    /// # Examples
    ///
    /// Basic client without handlers:
    /// ```
    /// use sovran_mcp::{McpClient, transport::StdioTransport};
    ///
    /// let transport = StdioTransport::new("npx", &["-y", "server-name"])?;
    /// let client = McpClient::new(transport, None, None);
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    ///
    /// Client with sampling and notification handlers:
    /// ```no_run
    /// use sovran_mcp::{McpClient, transport::StdioTransport, types::*, McpError};
    /// use std::sync::Arc;
    /// use serde_json::Value;
    /// use url::Url;    ///
    ///
    /// use sovran_mcp::messaging::{LogLevel, NotificationMethod};
    ///
    /// // Handler for LLM completion requests
    /// struct MySamplingHandler;
    /// impl SamplingHandler for MySamplingHandler {
    ///     fn handle_message(&self, request: CreateMessageRequest) -> Result<CreateMessageResponse, McpError> {
    ///         // Process the completion request and return response
    ///         Ok(CreateMessageResponse {
    ///             content: MessageContent::Text(TextContent {
    ///                 text: "AI response".to_string()
    ///             }),
    ///             model: "test-model".to_string(),
    ///             role: Role::Assistant,
    ///             stop_reason: Some("complete".to_string()),
    ///             meta: None,
    ///         })
    ///     }
    /// }
    ///
    /// // Handler for server notifications
    /// struct MyNotificationHandler;
    /// impl NotificationHandler for MyNotificationHandler {
    ///     fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
    ///         println!("Resource updated: {}", uri);
    ///         Ok(())
    ///     }
    ///     fn handle_initialized(&self) {
    ///         todo!()
    ///     }
    ///     fn handle_log_message(&self, level: &LogLevel, data: &Value, logger: &Option<String>) {
    ///         todo!()
    ///     }
    ///     fn handle_progress_update(&self, token: &String, progress: &f64, total: &Option<f64>) {
    ///         todo!()
    ///     }
    ///     fn handle_list_changed(&self, method: &NotificationMethod) {
    ///         todo!()
    ///     }
    /// }
    ///
    /// // Create client with handlers
    /// let transport = StdioTransport::new("npx", &["-y", "server-name"])?;
    /// let client = McpClient::new(
    ///     transport,
    ///     Some(Box::new(MySamplingHandler)),
    ///     Some(Box::new(MyNotificationHandler))
    /// );
    /// # Ok::<(), McpError>(())
    /// ```
    pub fn new(
        transport: T,
        sampling_handler: Option<Box<dyn SamplingHandler + Send>>,
        notification_handler: Option<Box<dyn NotificationHandler + Send>>,
    ) -> Self {
        Self {
            transport: Arc::new(transport),
            request_id: Arc::new(AtomicU64::new(0)),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            listener_handle: None,
            sampling_handler: sampling_handler.map(Arc::new),
            notification_handler: notification_handler.map(Arc::new),
            stop_flag: Arc::new(AtomicBool::new(false)),
            server_info: Arc::new(Mutex::new(None)),
        }
    }

    /// Starts the client, establishing the transport connection and initializing the MCP protocol.
    ///
    /// This method must be called before using any other client operations. It:
    /// - Opens the transport connection
    /// - Starts the message handling thread
    /// - Performs protocol initialization
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - Transport connection fails
    /// - Protocol initialization fails
    /// - Message handling thread cannot be started
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use sovran_mcp::{McpClient, transport::StdioTransport};
    ///
    /// let transport = StdioTransport::new("npx", &["-y", "server-name"])?;
    /// let mut client = McpClient::new(transport, None, None);
    ///
    /// // Start the client
    /// client.start()?;
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn start(&mut self) -> Result<(), McpError> {
        self.transport.open()?;

        let handler = MessageHandler::new(
            self.transport.clone(),
            self.pending_requests.clone(),
            self.sampling_handler.clone(),
            self.notification_handler.clone(),
        );

        let transport = self.transport.clone();
        let stop_flag = self.stop_flag.clone();

        let handle_wrapper = Arc::new(Mutex::new(None));
        let handle = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match transport.receive() {
                    Ok(message) => {
                        debug!("Received message: {:?}", message);
                        if let Err(e) = handler.handle_message(message) {
                            warn!("Error handling message: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Transport error: {}", e);
                        break;
                    }
                }
            }
            debug!("Message handling thread exited");
        });

        *handle_wrapper.lock().unwrap() = Some(handle);
        self.listener_handle = Some(handle_wrapper);

        // Phase 1: Initialize request/response
        self.initialize()?;
        debug!("Phase 1 complete: Received initialize response");

        // Phase 2: Send initialized notification
        // Phase 2: Send initialized notification
        debug!("Phase 2: Sending initialized notification");
        self.transport.send(&JsonRpcMessage::Notification(
            JsonRpcNotification::initialized(),
        ))?;
        debug!("Two-phase initialization complete");

        Ok(())
    }

    /// Stops the client, cleaning up resources and closing the transport connection.
    ///
    /// This method:
    /// - Signals the message handling thread to stop
    /// - Closes the transport connection
    /// - Cleans up any pending requests
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The transport close operation fails
    /// - The message handling thread cannot be properly stopped
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let transport = StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
    /// # let mut client = McpClient::new(transport, None, None);
    /// # client.start()?;
    /// // Use the client...
    ///
    /// // Clean up when done
    /// client.stop()?;
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn stop(&mut self) -> Result<(), McpError> {
        // Signal the thread to stop
        self.stop_flag.store(true, Ordering::SeqCst);

        // send a dummy command to generate an error which will
        // unblock any pending receive.
        _ = self.request("__internal/server_stop", None)?;

        // Kill the transport
        self.transport.close()?;

        // Join the thread if it hasn't detached
        if let Some(wrapper) = self.listener_handle.take() {
            if let Some(handle) = wrapper.lock().unwrap().take() {
                debug!("client::stop() attempting to join message handling thread");
                handle.join().map_err(|_| McpError::ThreadJoinFailed)?;
                debug!("client::stop() joined message handling thread");
            }
        }

        Ok(())
    }

    fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, McpError> {
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

        // Wait for response with timeout but DON'T remove the pending request
        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(response) => {
                // Only remove on success
                let mut pending = self.pending_requests.lock().unwrap();
                pending.remove(&id);
                Ok(response)
            }
            Err(e) => Err(McpError::RequestTimeout {
                method: method.into(),
                source: e,
            }),
        }
    }

    /// Executes a generic MCP command on the server.
    ///
    /// This is a lower-level method used internally by the specific command methods (list_tools, get_prompt, etc).
    /// It can be used to implement custom commands when extending the protocol.
    ///
    /// # Type Parameters
    ///
    /// * `C` - A type implementing the `McpCommand` trait, which defines:
    ///   - The command name (`COMMAND`)
    ///   - The request type (`Request`)
    ///   - The response type (`Response`)
    ///
    /// # Arguments
    ///
    /// * `request` - The command-specific request data
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The command execution fails on the server
    /// - The request times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use sovran_mcp::{McpClient, transport::StdioTransport, McpCommand};
    /// use serde::{Serialize, Deserialize};
    ///
    /// // Define a custom command
    /// #[derive(Debug, Clone)]
    /// pub struct MyCommand;
    ///
    /// impl McpCommand for MyCommand {
    ///     const COMMAND: &'static str = "custom/operation";
    ///     type Request = MyRequest;
    ///     type Response = MyResponse;
    /// }
    ///
    /// #[derive(Debug, Serialize)]
    /// pub struct MyRequest {
    ///     data: String,
    /// }
    ///
    /// #[derive(Debug, Deserialize)]
    /// pub struct MyResponse {
    ///     result: String,
    /// }
    ///
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@myserver/dummy_server"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// // Execute the custom command
    /// let request = MyRequest {
    ///     data: "test".to_string(),
    /// };
    ///
    /// let response = client.execute::<MyCommand>(request)?;
    /// println!("Got result: {}", response.result);
    /// # client.stop()?;
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn execute<C: McpCommand>(&self, request: C::Request) -> Result<C::Response, McpError> {
        debug!("Executing command: {}", C::COMMAND); // Add this
        let response = self.request(C::COMMAND, Some(serde_json::to_value(request)?))?;
        debug!("Got response for: {}", C::COMMAND); // Add this

        if let Some(error) = response.error {
            return Err(McpError::CommandFailed {
                command: C::COMMAND.to_string(),
                error,
            });
        }
        println!("Tool Response: {:?}", response);
        let result = response.result.ok_or_else(|| McpError::MissingResult)?;

        Ok(serde_json::from_value(result)?)
    }

    fn initialize(&self) -> Result<&Self, McpError> {
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

    /// Returns the server's capabilities as reported during initialization.
    ///
    /// # Errors
    ///
    /// Returns `McpError::ClientNotInitialized` if called before the client is started.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let capabilities = client.server_capabilities()?;
    ///
    /// // Check specific capabilities
    /// if let Some(prompts) = capabilities.prompts {
    ///     println!("Server supports prompts with list_changed: {:?}",
    ///         prompts.list_changed);
    /// }
    ///
    /// if let Some(resources) = capabilities.resources {
    ///     println!("Server supports resources with subscribe: {:?}",
    ///         resources.subscribe);
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn server_capabilities(&self) -> Result<ServerCapabilities, McpError> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.capabilities.clone())
            .ok_or_else(|| McpError::ClientNotInitialized)
    }

    /// Returns the protocol version supported by the server.
    ///
    /// # Errors
    ///
    /// Returns `McpError::ClientNotInitialized` if called before the client is started.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let version = client.server_version()?;
    /// println!("Server supports MCP version: {}", version);
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn server_version(&self) -> Result<String, McpError> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.protocol_version.clone())
            .ok_or_else(|| McpError::ClientNotInitialized)
    }

    /// Checks if the server has a specific capability using a custom predicate.
    ///
    /// This is a lower-level method used by the specific capability checks. It allows
    /// for custom capability testing logic.
    ///
    /// # Arguments
    ///
    /// * `check` - A closure that takes a reference to ServerCapabilities and returns a boolean
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// // Check if server supports prompt list change notifications
    /// let has_prompt_notifications = client.has_capability(|caps| {
    ///     caps.prompts
    ///         .as_ref()
    ///         .and_then(|p| p.list_changed)
    ///         .unwrap_or(false)
    /// });
    ///
    /// println!("Prompt notifications supported: {}", has_prompt_notifications);
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn has_capability<F>(&self, check: F) -> bool
    where
        F: FnOnce(&ServerCapabilities) -> bool,
    {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| check(&info.capabilities))
            .unwrap_or(false)
    }

    /// Checks if the server supports resource operations.
    ///
    /// Resources are server-managed content that can be read and monitored for changes.
    ///
    /// # Returns
    ///
    /// Returns `true` if the server supports resources, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_resources() {
    ///     let resources = client.list_resources()?;
    ///     println!("Available resources: {}", resources.resources.len());
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_resources(&self) -> bool {
        self.has_capability(|caps| caps.resources.is_some())
    }

    /// Checks if the server supports prompt operations.
    ///
    /// Prompts are server-managed message templates that can be retrieved and processed.
    ///
    /// # Returns
    ///
    /// Returns `true` if the server supports prompts, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_prompts() {
    ///     let prompts = client.list_prompts()?;
    ///     println!("Available prompts: {}", prompts.prompts.len());
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_prompts(&self) -> bool {
        self.has_capability(|caps| caps.prompts.is_some())
    }

    /// Checks if the server supports tool operations.
    ///
    /// Tools are server-provided functions that can be called by the client.
    ///
    /// # Returns
    ///
    /// Returns `true` if the server supports tools, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_tools() {
    ///     let tools = client.list_tools()?;
    ///     println!("Available tools: {}", tools.tools.len());
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_tools(&self) -> bool {
        self.has_capability(|caps| caps.tools.is_some())
    }

    /// Checks if the server supports logging capabilities.
    ///
    /// # Returns
    ///
    /// Returns `true` if the server supports logging, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_logging() {
    ///     println!("Server supports logging capabilities");
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_logging(&self) -> bool {
        self.has_capability(|caps| caps.logging.is_some())
    }

    /// Checks if the server supports resource subscriptions.
    ///
    /// Resource subscriptions allow the client to receive notifications when resources change.
    ///
    /// # Returns
    ///
    /// Returns `true` if the server supports resource subscriptions, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_resource_subscription() {
    ///     println!("Server supports resource change notifications");
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_resource_subscription(&self) -> bool {
        self.has_capability(|caps| {
            caps.resources
                .as_ref()
                .and_then(|r| r.subscribe)
                .unwrap_or(false)
        })
    }

    /// Checks if the server supports resource list change notifications.
    pub fn supports_resource_list_changed(&self) -> bool {
        self.has_capability(|caps| {
            caps.resources
                .as_ref()
                .and_then(|r| r.list_changed)
                .unwrap_or(false)
        })
    }

    /// Checks if the server supports prompt list change notifications.
    pub fn supports_prompt_list_changed(&self) -> bool {
        self.has_capability(|caps| {
            caps.prompts
                .as_ref()
                .and_then(|p| p.list_changed)
                .unwrap_or(false)
        })
    }

    /// Checks if the server supports a specific experimental feature.
    ///
    /// # Arguments
    ///
    /// * `feature` - The name of the experimental feature to check
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// if client.supports_experimental_feature("my_feature") {
    ///     println!("Server supports experimental feature 'my_feature'");
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn supports_experimental_feature(&self, feature: &str) -> bool {
        self.has_capability(|caps| {
            caps.experimental
                .as_ref()
                .and_then(|e| e.get(feature))
                .is_some()
        })
    }

    /// Lists all available tools provided by the server.
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support tools (`UnsupportedCapability`)
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let tools = client.list_tools()?;
    ///
    /// for tool in tools.tools {
    ///     println!("Tool: {} - {:?}", tool.name, tool.description);
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn list_tools(&self) -> Result<ListToolsResponse, McpError> {
        if !self.supports_tools() {
            return Err(McpError::UnsupportedCapability("tools"));
        }
        let request = ListToolsRequest {
            cursor: None,
            meta: None,
        };
        self.execute::<ListTools>(request)
    }

    /// Calls a tool on the server with the specified arguments.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `arguments` - Optional JSON-formatted arguments for the tool
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support tools (`UnsupportedCapability`)
    /// - The specified tool doesn't exist
    /// - The arguments are invalid for the tool
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// Simple tool call (echo):
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # use sovran_mcp::types::ToolResponseContent;
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let response = client.call_tool(
    ///     "echo".to_string(),
    ///     Some(serde_json::json!({
    ///         "message": "Hello, MCP!"
    ///     }))
    /// )?;
    ///
    /// // Handle the response
    /// if let Some(content) = response.content.first() {
    ///     match content {
    ///         ToolResponseContent::Text { text } => println!("Got response: {}", text),
    ///         _ => println!("Got non-text response"),
    ///     }
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    ///
    /// Tool with numeric arguments:
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # use sovran_mcp::types::ToolResponseContent;
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// // Call an "add" tool that sums two numbers
    /// let response = client.call_tool(
    ///     "add".to_string(),
    ///     Some(serde_json::json!({
    ///         "a": 2,
    ///         "b": 3
    ///     }))
    /// )?;
    ///
    /// // Process the response
    /// if let Some(ToolResponseContent::Text { text }) = response.content.first() {
    ///     println!("Result: {}", text); // "The sum of 2 and 3 is 5."
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn call_tool(
        &self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResponse, McpError> {
        if !self.supports_tools() {
            return Err(McpError::UnsupportedCapability("tools"));
        }
        let request = CallToolRequest {
            name,
            arguments,
            meta: None,
        };
        self.execute::<CallTool>(request)
    }

    /// Lists all available prompts from the server.
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support prompts (`UnsupportedCapability`)
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let prompts = client.list_prompts()?;
    ///
    /// for prompt in prompts.prompts {
    ///     println!("Prompt: {} - {:?}", prompt.name, prompt.description);
    ///
    ///     // Check for required arguments
    ///     if let Some(args) = prompt.arguments {
    ///         for arg in args {
    ///             println!("  Argument: {} (required: {})",
    ///                 arg.name,
    ///                 arg.required.unwrap_or(false)
    ///             );
    ///         }
    ///     }
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn list_prompts(&self) -> Result<ListPromptsResponse, McpError> {
        if !self.supports_prompts() {
            return Err(McpError::UnsupportedCapability("prompts"));
        }
        let request = ListPromptsRequest {
            cursor: None,
            meta: None,
        };
        self.execute::<ListPrompts>(request)
    }

    /// Retrieves a specific prompt from the server with optional arguments.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the prompt to retrieve
    /// * `arguments` - Optional key-value pairs of arguments for the prompt
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support prompts (`UnsupportedCapability`)
    /// - The specified prompt doesn't exist
    /// - Required arguments are missing
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// Simple prompt without arguments:
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # use sovran_mcp::types::Role;
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let response = client.get_prompt("simple_prompt".to_string(), None)?;
    ///
    /// // Process the messages
    /// for message in response.messages {
    ///     match message.role {
    ///         Role::System => println!("System: {:?}", message.content),
    ///         Role::User => println!("User: {:?}", message.content),
    ///         Role::Assistant => println!("Assistant: {:?}", message.content),
    ///     }
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    ///
    /// Prompt with arguments:
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # use std::collections::HashMap;
    /// # use sovran_mcp::types::PromptContent;
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let mut args = HashMap::new();
    /// args.insert("temperature".to_string(), "0.7".to_string());
    /// args.insert("style".to_string(), "formal".to_string());
    ///
    /// let response = client.get_prompt("complex_prompt".to_string(), Some(args))?;
    ///
    /// // Process different content types
    /// for message in response.messages {
    ///     match &message.content {
    ///         PromptContent::Text(text) => {
    ///             println!("{}: {}", message.role, text.text);
    ///         }
    ///         PromptContent::Image(img) => {
    ///             println!("{}: Image ({:?})", message.role, img.mime_type);
    ///         }
    ///         PromptContent::Resource(res) => {
    ///             println!("{}: Resource at {}", message.role, res.resource.uri);
    ///         }
    ///     }
    /// }
    /// # client.stop()?;
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn get_prompt(
        &self,
        name: String,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResponse, McpError> {
        if !self.supports_prompts() {
            return Err(McpError::UnsupportedCapability("prompts"));
        }
        let request = GetPromptRequest { name, arguments };
        self.execute::<GetPrompt>(request)
    }

    /// Lists all available resources from the server.
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support resources (`UnsupportedCapability`)
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// let resources = client.list_resources()?;
    ///
    /// for resource in resources.resources {
    ///     println!("Resource: {} ({})", resource.name, resource.uri);
    ///     if let Some(mime_type) = resource.mime_type {
    ///         println!("  Type: {}", mime_type);
    ///     }
    ///     if let Some(description) = resource.description {
    ///         println!("  Description: {}", description);
    ///     }
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn list_resources(&self) -> Result<ListResourcesResponse, McpError> {
        if !self.supports_resources() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = ListResourcesRequest { cursor: None };
        self.execute::<ListResources>(request)
    }

    /// Reads the content of a specific resource.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to read
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support resources (`UnsupportedCapability`)
    /// - The specified resource doesn't exist
    /// - The request fails or times out
    /// - The response cannot be deserialized
    ///
    /// # Examples
    ///
    /// ```
    /// # use sovran_mcp::{McpClient, transport::StdioTransport};
    /// # use sovran_mcp::types::ResourceContent;
    /// # let mut client = McpClient::new(
    /// #     StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?,
    /// #     None,
    /// #     None
    /// # );
    /// # client.start()?;
    /// # let resources = client.list_resources()?;
    /// # let resource = &resources.resources[0];
    /// // Read a resource's contents
    /// let content = client.read_resource(&resource.uri)?;
    ///
    /// // Handle different content types
    /// for item in content.contents {
    ///     match item {
    ///         ResourceContent::Text(text) => {
    ///             println!("Text content: {}", text.text);
    ///             if let Some(mime) = text.mime_type {
    ///                 println!("MIME type: {}", mime);
    ///             }
    ///         }
    ///         ResourceContent::Blob(blob) => {
    ///             println!("Binary content ({} bytes)", blob.blob.len());
    ///             if let Some(mime) = blob.mime_type {
    ///                 println!("MIME type: {}", mime);
    ///             }
    ///         }
    ///     }
    /// }
    /// # Ok::<(), sovran_mcp::McpError>(())
    /// ```
    pub fn read_resource(&self, uri: &Url) -> Result<ReadResourceResponse, McpError> {
        if !self.supports_resources() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = ReadResourceRequest { uri: uri.clone() };
        self.execute::<ReadResource>(request)
    }

    /// Subscribes to changes for a specific resource.
    ///
    /// When subscribed, the client will receive notifications through the `NotificationHandler`
    /// whenever the resource changes.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to subscribe to
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support resource subscriptions (`UnsupportedCapability`)
    /// - The specified resource doesn't exist
    /// - The request fails or times out
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use sovran_mcp::{McpClient, transport::StdioTransport, messaging::*, types::*, McpError};
    /// # use url::Url;
    /// # use std::sync::Arc;
    /// # use serde_json::Value;
    /// // Create a notification handler
    /// struct MyNotificationHandler;
    /// impl NotificationHandler for MyNotificationHandler {
    ///     fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
    ///         println!("Resource updated: {}", uri);
    ///         Ok(())
    ///     }
    ///     fn handle_log_message(&self, level: &LogLevel, data: &Value, logger: &Option<String>) {
    ///         todo!()
    ///     }
    ///     fn handle_progress_update(&self, token: &String, progress: &f64, total: &Option<f64>) {
    ///         todo!()
    ///     }
    ///     fn handle_initialized(&self) {
    ///         todo!()
    ///     }
    ///     fn handle_list_changed(&self, method: &NotificationMethod) {
    ///         todo!()
    ///     }
    /// }
    ///
    /// # let transport = StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
    /// # let mut client = McpClient::new(
    /// #     transport,
    /// #     None,
    /// #     Some(Box::new(MyNotificationHandler))
    /// # );
    /// # client.start()?;
    /// # let resources = client.list_resources()?;
    /// # let resource = &resources.resources[0];
    /// // Subscribe to a resource
    /// if client.supports_resource_subscription() {
    ///     client.subscribe(&resource.uri)?;
    ///     println!("Subscribed to {}", resource.uri);
    ///
    ///     // ... wait for notifications through handler ...
    ///
    ///     // Unsubscribe when done
    ///     client.unsubscribe(&resource.uri)?;
    /// }
    /// # Ok::<(), McpError>(())
    /// ```
    pub fn subscribe(&self, uri: &Url) -> Result<EmptyResult, McpError> {
        if !self.supports_resource_subscription() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = SubscribeRequest { uri: uri.clone() };
        self.execute::<Subscribe>(request)
    }

    /// Unsubscribes from changes for a specific resource.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to unsubscribe from
    ///
    /// # Errors
    ///
    /// Returns `McpError` if:
    /// - The server doesn't support resource subscriptions (`UnsupportedCapability`)
    /// - The specified resource doesn't exist
    /// - The request fails or times out
    ///
    /// # Examples
    ///
    /// See the `subscribe` method for a complete example including unsubscribe.
    pub fn unsubscribe(&self, uri: &Url) -> Result<EmptyResult, McpError> {
        if !self.supports_resource_subscription() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = UnsubscribeRequest { uri: uri.clone() };
        self.execute::<Unsubscribe>(request)
    }
}
