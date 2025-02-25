use crate::server::handlers::*;
use crate::server::transport::{ServerTransport, StdioServerTransport};
use crate::types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sovran_typemap::{StoreError, TypeStore};
use tracing::{debug, warn};

impl From<StoreError> for McpError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::KeyNotFound(key) => McpError::Other(format!("Resource not found, key: {}", key).into()),
            StoreError::TypeMismatch => McpError::Other("Resource type mismatch".into()),
            StoreError::LockError => McpError::Other("Failed to acquire lock on resource store".into()),
        }
    }
}

/// Core tool interface

pub struct McpToolServer {
    transport: Arc<Mutex<Box<dyn ServerTransport>>>,
    resources: TypeStore<String>,
}

impl McpToolServer {
    pub(crate) fn new(
        transport: Arc<Mutex<Box<dyn ServerTransport>>>,
        resources: TypeStore<String>,
    ) -> Self {
        Self { transport, resources }
    }

    pub fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let mut transport = self.transport.lock().unwrap();
        transport.write_message(&JsonRpcMessage::Notification(notification))
    }

    pub fn send_progress(
        &self,
        token: impl Into<String>,
        progress: f64,
        total: Option<f64>,
    ) -> Result<(), McpError> {
        self.send_notification(JsonRpcNotification::progress(
            token.into(),
            progress,
            total,
        ))
    }

    pub fn get_resource<T: McpResource + 'static>(&self, uri: &str) -> Result<T, McpError>
    where T: Clone
    {
        // Use TypeStore's get method which returns a clone
        self.resources.get(&uri.to_string())
            .map_err(|e| McpError::Other(format!("Resource error: {}", e)))
    }

    pub fn set_resource<T: McpResource + 'static>(&self, uri: String, resource: T) -> Result<(), McpError> {
        // Store the resource
        self.resources.set(uri.clone(), resource)
            .map_err(|e| McpError::Other(format!("Failed to store resource: {}", e)))?;

        // Send notification about the new/updated resource
        self.send_notification(JsonRpcNotification::resource_updated(&uri))?;

        Ok(())
    }

    pub fn list_resources(&self) -> Vec<String> {
        self.resources.keys().unwrap_or_default()
    }

    pub fn with_resource<T: McpResource + 'static, R, F: FnOnce(&T) -> R>(&self, uri: &str, f: F) -> Result<R, McpError> {
        self.resources.with(&uri.to_string(), f)
            .map_err(|e| McpError::Other(format!("Resource error: {}", e)))
    }

    pub fn with_resource_mut<T: McpResource + 'static, F: FnOnce(&mut T)>(&self, uri: &str, f: F) -> Result<(), McpError> {
        self.resources.with_mut(&uri.to_string(), f)
            .map_err(|e| McpError::Other(format!("Resource error: {}", e)))?;

        // Send notification that resource was modified
        self.send_notification(JsonRpcNotification::resource_updated(uri))?;

        Ok(())
    }
}

pub trait McpTool<CTX>: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn execute(
        &self,
        args: Value,
        context: &mut CTX,
        tool_server: &McpToolServer,
    ) -> Result<CallToolResponse, McpError>;
}

pub trait McpResource: Send + Sync {
    fn uri(&self) -> String;
    fn name(&self) -> String;
    fn mime_type(&self) -> String;
    fn content(&self) -> ResourceContent;
}

/// The MCP Server implementation
pub struct McpServer<CTX> {
    name: String,
    version: String,
    transport: Arc<Mutex<Box<dyn ServerTransport>>>,
    tools: HashMap<String, Box<dyn McpTool<CTX>>>,
    handlers: HashMap<&'static str, HandlerFn<CTX>>,
    resources: TypeStore<String>,
}

impl<CTX: Send + Sync + 'static> McpServer<CTX> {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self::with_transport(name, version, StdioServerTransport::new())
    }

    pub fn with_transport(
        name: impl Into<String>,
        version: impl Into<String>,
        transport: impl ServerTransport + 'static,
    ) -> Self {
        let name = name.into();
        let version = version.into();
        let transport = Arc::new(Mutex::new(Box::new(transport) as Box<dyn ServerTransport>));
        let resources = TypeStore::new();

        let mut server = Self {
            name: name.clone(),
            version: version.clone(),
            transport,
            tools: HashMap::new(),
            handlers: HashMap::new(),
            resources,
        };

        // Set up default handlers
        server.set_handler::<Initialize, _>(DefaultInitializeHandler::new(name, version));
        server.set_handler::<Shutdown, _>(DefaultShutdownHandler);
        server.set_handler::<ListTools, _>(DefaultListToolsHandler);
        server.set_handler::<CallTool, _>(DefaultCallToolHandler);

        server
    }

    pub fn set_handler<CMD, H>(&mut self, handler: H)
    where
        CMD: McpCommand,
        CMD::Request: DeserializeOwned,
        CMD::Response: Serialize,
        H: CommandHandler<CMD, CTX> + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        let handler_fn = HandlerFn::new(move |params, server, context| {
            let request: CMD::Request = serde_json::from_value(params)?;
            let response = handler.handle(request, server, context)?;
            Ok(serde_json::to_value(response)?)
        });

        self.handlers.insert(CMD::COMMAND, handler_fn);
    }

    pub fn execute_tool(&mut self, name: &str, args: Value, context: &mut CTX) -> Result<CallToolResponse, McpError> {
        let tool = self.tools
            .get(name)
            .ok_or_else(|| McpError::UnknownTool(name.to_string()))?;

        // Create tool server with the transport and resources
        // No need to clone the Arc for resources anymore since TypeStore is already thread-safe
        let tool_server = McpToolServer::new(
            Arc::clone(&self.transport),
            self.resources.clone()  // TypeStore implements Clone
        );

        tool.execute(args, context, &tool_server)
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: Some(tool.description().to_string()),
                input_schema: tool.schema(),
            })
            .collect()
    }

    pub fn add_tool<Tool>(&mut self, tool: Tool) -> Result<(), McpError>
    where
        Tool: McpTool<CTX> + 'static,
    {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            return Err(McpError::DuplicateTool(name));
        }

        self.tools.insert(name, Box::new(tool));
        Ok(())
    }

    pub fn add_resource<R>(&mut self, resource: R) -> Result<(), McpError>
    where
        R: McpResource + 'static,
    {
        let uri = resource.uri().to_string();
        if self.resources.contains_key(&uri)? {
            return Err(McpError::Other(format!("Resource already exists: {}", uri)));
        }

        self.resources.set(uri.clone(), resource)?;

        Ok(())
    }

    pub fn start(&mut self, mut context: CTX) -> ! {
        eprintln!(
            "MCP Server `{}` started, version {}",
            self.name, self.version
        );
        let result: Result<(), McpError> = loop {
            // Lock transport for reading
            let message = {
                let mut transport = self.transport.lock().unwrap();
                transport.read_message()
            };

            match message {
                Ok(message) => {
                    debug!("Received message: {:?}", message);
                    match message {
                        JsonRpcMessage::Request(request) => {
                            if let Err(e) = self.handle_request(request, &mut context) {
                                if e.is_shutdown() {
                                    std::process::exit(0);
                                }
                                break Err(e);
                            }
                        }
                        JsonRpcMessage::Notification(notification) => {
                            if let Err(e) = self.handle_notification(notification) {
                                break Err(e);
                            }
                        }
                        _ => {
                            warn!("Unexpected message type");
                        }
                    }
                }
                Err(e) => {
                    warn!("Transport error: {}", e);
                    break Err(e);
                }
            }
        };

        // Handle any error that broke the loop
        if let Err(e) = result {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        }

        std::process::exit(0);
    }

    fn handle_request(&mut self, request: JsonRpcRequest, context: &mut CTX) -> Result<(), McpError> {
        // Clone the handler function if it exists
        let handler = (&self.handlers
            .get(request.method.as_str()))
            .cloned();  // Clone the Arc<HandlerFn>

        let response = if let Some(handler) = handler {
            match handler.handle(request.params.unwrap_or(json!({})), self, context) {
                Ok(result) => JsonRpcResponse {
                    id: request.id,
                    result: Some(result),
                    error: None,
                    jsonrpc: request.jsonrpc,
                },
                Err(e) => JsonRpcResponse {
                    id: request.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32000,
                        message: e.to_string(),
                        data: None,
                    }),
                    jsonrpc: request.jsonrpc,
                },
            }
        } else {
            JsonRpcResponse {
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
                jsonrpc: request.jsonrpc,
            }
        };

        let mut transport = self.transport.lock().unwrap();
        transport.write_message(&JsonRpcMessage::Response(response))?;
        Ok(())
    }

    fn handle_notification(&mut self, notification: JsonRpcNotification) -> Result<(), McpError> {
        debug!("Received notification: {:?}", notification);
        Ok(())
    }

    pub fn send_notification(&mut self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let mut transport = self.transport.lock().unwrap();
        transport.write_message(&JsonRpcMessage::Notification(notification))
    }
}

// Handler function type for type-erased command handling
struct HandlerFn<CTX> {
    handle_fn: Arc<dyn Fn(Value, &mut McpServer<CTX>, &mut CTX) -> Result<Value, McpError> + Send + Sync>,
}

impl<CTX> HandlerFn<CTX> {
    fn new<F>(f: F) -> Self
    where
        F: Fn(Value, &mut McpServer<CTX>, &mut CTX) -> Result<Value, McpError> + Send + Sync + 'static,
    {
        Self {
            handle_fn: Arc::new(f),
        }
    }

    fn handle(&self, params: Value, server: &mut McpServer<CTX>, context: &mut CTX) -> Result<Value, McpError> {
        (self.handle_fn)(params, server, context)
    }
}

impl<CTX> Clone for HandlerFn<CTX> {
    fn clone(&self) -> Self {
        Self {
            handle_fn: Arc::clone(&self.handle_fn),
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here...
}
