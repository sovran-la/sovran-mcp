use crate::transport::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, Transport};
use crate::messaging::MessageHandler;
use crate::types::*;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Sender};
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use url::Url;
use crate::commands::{CallTool, GetPrompt, Initialize, ListPrompts, ListResources, ListTools, McpCommand, ReadResource, Subscribe, Unsubscribe};
use tracing::{debug, warn};
use crate::McpError;

pub struct Client<T: Transport + 'static> {
    transport: Arc<T>,
    request_id: Arc<AtomicU64>,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
    listener_handle: Option<Arc<Mutex<Option<JoinHandle<()>>>>>,
    sampling_handler: Option<Arc<Box<dyn SamplingHandler + Send>>>,
    notification_handler: Option<Arc<Box<dyn NotificationHandler + Send>>>,
    stop_flag: Arc<AtomicBool>,
    server_info: Arc<Mutex<Option<InitializeResponse>>>,
}

impl<T: Transport + 'static> Drop for Client<T> {
    fn drop(&mut self) {
        if !self.stop_flag.load(Ordering::SeqCst) {
            // We don't want to panic in drop, so we ignore any errors
            let _ = self.stop();
        }
    }
}

impl<T: Transport + 'static> Client<T> {
    pub fn new(transport: T,
               sampling_handler: Option<Box<dyn SamplingHandler + Send>>,
               notification_handler: Option<Box<dyn NotificationHandler + Send>>
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
                    },
                    Err(e) => {
                        warn!("Transport error: {}", e);
                        break;
                    }
                }
            }
        });

        *handle_wrapper.lock().unwrap() = Some(handle);
        self.listener_handle = Some(handle_wrapper);

        self.initialize()?;
        Ok(())
    }

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
                //let _ = handle.join(); // Wait for thread termination
                drop(handle);
            }
        }

        Ok(())
    }

    fn request(&self, method: &str, params: Option<serde_json::Value>) -> Result<JsonRpcResponse, McpError> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        debug!("Setting up pending request id: {}", id);
        let (tx, rx) = channel();

        // Store the sender
        {
            let mut pending = self.pending_requests.lock().unwrap();
            pending.insert(id, tx);
            debug!("Added pending request. Current IDs: {:?}", pending.keys().collect::<Vec<_>>());
        }

        // Send the request
        let request = JsonRpcRequest {
            id,
            method: method.to_string(),
            params,
            jsonrpc: Default::default(),
        };
        debug!("Sending request: {:?}", request);
        self.transport.send(&JsonRpcMessage::Request(request))?;

        // Wait for response with timeout
        debug!("Waiting for response to id: {}", id);
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(response) => Ok(response),
            Err(e) => {
                // Remove from pending requests
                let mut pending = self.pending_requests.lock().unwrap();
                pending.remove(&id);
                Err(McpError::RequestTimeout {
                    method: method.into(), // Store method name
                    source: e             // Store underlying error
                })
            }
        }
    }


    pub fn execute<C: McpCommand>(&self, request: C::Request) -> Result<C::Response, McpError> {
        debug!("Executing command: {}", C::COMMAND);  // Add this
        let response = self.request(
            C::COMMAND,
            Some(serde_json::to_value(request)?)
        )?;
        debug!("Got response for: {}", C::COMMAND);   // Add this

        if let Some(error) = response.error {
            return Err(McpError::CommandFailed {
                command: C::COMMAND.to_string(),
                error,
            });
        }

        let result = response.result
            .ok_or_else(|| McpError::MissingResult)?;

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

    pub fn server_capabilities(&self) -> Result<ServerCapabilities, McpError> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.capabilities.clone())
            .ok_or_else(|| McpError::ClientNotInitialized)
    }

    pub fn server_version(&self) -> Result<String, McpError> {
        self.server_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| info.protocol_version.clone())
            .ok_or_else(|| McpError::ClientNotInitialized)
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

    pub fn call_tool(&self, name: String, arguments: Option<serde_json::Value>) -> Result<CallToolResponse, McpError> {
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

    pub fn get_prompt(&self, name: String, arguments: Option<HashMap<String, String>>) -> Result<GetPromptResponse, McpError> {
        if !self.supports_prompts() {
            return Err(McpError::UnsupportedCapability("prompts"));
        }
        let request = GetPromptRequest { name, arguments };
        self.execute::<GetPrompt>(request)
    }

    pub fn list_resources(&self) -> Result<ListResourcesResponse, McpError> {
        if !self.supports_resources() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = ListResourcesRequest { cursor: None };
        self.execute::<ListResources>(request)
    }

    pub fn read_resource(&self, uri: &Url) -> Result<ReadResourceResponse, McpError> {
        if !self.supports_resources() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = ReadResourceRequest { uri: uri.clone() };
        self.execute::<ReadResource>(request)
    }

    pub fn subscribe(&self, uri: &Url) -> Result<EmptyResult, McpError> {
        if !self.supports_resource_subscription() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = SubscribeRequest { uri: uri.clone() };
        self.execute::<Subscribe>(request)
    }

    pub fn unsubscribe(&self, uri: &Url) -> Result<EmptyResult, McpError> {
        if !self.supports_resource_subscription() {
            return Err(McpError::UnsupportedCapability("resources"));
        }
        let request = UnsubscribeRequest { uri: uri.clone() };
        self.execute::<Unsubscribe>(request)
    }
}
