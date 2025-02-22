use crate::server::handlers::*;
use crate::server::transport::{ServerTransport, StdioServerTransport};
use crate::types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// Core tool interface

pub struct McpToolServer {
    transport: Arc<Mutex<Box<dyn ServerTransport>>>,
}

impl McpToolServer {
    fn new(transport: Arc<Mutex<Box<dyn ServerTransport>>>) -> Self {
        Self { transport }
    }

    pub fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let mut transport = self.transport.lock().unwrap();
        transport.write_message(&JsonRpcMessage::Notification(notification))
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

/// The MCP Server implementation
pub struct McpServer<CTX> {
    name: String,
    version: String,
    transport: Arc<Mutex<Box<dyn ServerTransport>>>,
    tools: HashMap<String, Box<dyn McpTool<CTX>>>,
    handlers: HashMap<&'static str, HandlerFn<CTX>>,
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

        let mut server = Self {
            name: name.clone(),
            version: version.clone(),
            transport: Arc::new(Mutex::new(Box::new(transport))),
            tools: HashMap::new(),
            handlers: HashMap::new(),
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

        // Create tool server with cloned Arc to transport
        let tool_server = McpToolServer::new(Arc::clone(&self.transport));

        // Execute with tool server
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
