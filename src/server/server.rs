use crate::types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use tracing::{debug, warn};

/// Server-side transport trait - simpler than the client transport
pub trait ServerTransport: Send + Sync {
    fn read_message(&mut self) -> Result<JsonRpcMessage, McpError>;
    fn write_message(&mut self, message: &JsonRpcMessage) -> Result<(), McpError>;
    fn close(&mut self) -> Result<(), McpError>;
}

/// Default stdio transport implementation
pub struct StdioServerTransport;

impl StdioServerTransport {
    pub fn new() -> Self {
        Self
    }
}

impl ServerTransport for StdioServerTransport {
    fn read_message(&mut self) -> Result<JsonRpcMessage, McpError> {
        let stdin = io::stdin();
        let mut line = String::new();
        stdin.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    fn write_message(&mut self, message: &JsonRpcMessage) -> Result<(), McpError> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer(&mut handle, message)?;
        writeln!(handle)?;
        handle.flush()?;
        Ok(())
    }

    fn close(&mut self) -> Result<(), McpError> {
        // do nothing
        Ok(())
    }
}

/// Core tool interface
pub trait McpTool<CTX>: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;
    fn execute(
        &self,
        args: Value,
        context: &mut CTX,
        server: &mut McpServer<CTX>,
    ) -> Result<CallToolResponse, McpError>;
}

/// Command handler trait
pub trait CommandHandler<CMD: McpCommand, CTX>: Send + Sync {
    fn handle(
        &self,
        request: CMD::Request,
        server: &mut McpServer<CTX>,
    ) -> Result<CMD::Response, McpError>;
}

/// The MCP Server implementation
pub struct McpServer<CTX> {
    name: String,
    version: String,
    transport: Box<dyn ServerTransport>,
    tools: HashMap<String, Box<dyn McpTool<CTX>>>,
    handlers: HashMap<&'static str, HandlerFn<CTX>>,
    context: CTX,
}

// Default handlers
struct DefaultInitializeHandler {
    name: String,
    version: String,
}
impl<CTX> CommandHandler<Initialize, CTX> for DefaultInitializeHandler {
    fn handle(
        &self,
        _request: InitializeRequest,
        _server: &mut McpServer<CTX>,
    ) -> Result<InitializeResponse, McpError> {
        Ok(InitializeResponse {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(json!({})),
                ..Default::default()
            },
            server_info: Implementation {
                name: self.name.clone(),
                version: self.version.clone(),
            },
        })
    }
}

struct DefaultShutdownHandler;
impl<CTX> CommandHandler<Shutdown, CTX> for DefaultShutdownHandler {
    fn handle(
        &self,
        _request: ShutdownRequest,
        _server: &mut McpServer<CTX>,
    ) -> Result<ShutdownResponse, McpError> {
        // do nothing
        Ok(ShutdownResponse { meta: None })
    }
}

struct DefaultListToolsHandler;
impl<CTX> CommandHandler<ListTools, CTX> for DefaultListToolsHandler {
    fn handle(
        &self,
        _request: ListToolsRequest,
        server: &mut McpServer<CTX>,
    ) -> Result<ListToolsResponse, McpError> {
        let tools = server
            .tools
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: Some(tool.description().to_string()),
                input_schema: tool.schema(),
            })
            .collect();

        Ok(ListToolsResponse {
            tools,
            next_cursor: None,
            meta: None,
        })
    }
}

struct DefaultCallToolHandler;
impl<CTX> CommandHandler<CallTool, CTX> for DefaultCallToolHandler {
    fn handle(
        &self,
        request: CallToolRequest,
        server: &mut McpServer<CTX>,
    ) -> Result<CallToolResponse, McpError> {
        // CERTIFIED UNSAFE ZONE™
        // We need to get around the borrow checker here because we need to:
        // 1. Look up the tool (immutable borrow)
        // 2. Then call it with mutable context and server references
        let server_ptr = server as *mut McpServer<CTX>;

        let tool = server
            .tools
            .get(&request.name)
            .ok_or_else(|| McpError::UnknownTool(request.name.clone()))?;

        // This is safe because:
        // 1. We know the server exists (we have a reference)
        // 2. We know no one else has access (we control all server access)
        // 3. The pointer can't outlive the server (scoped to this function)
        unsafe {
            let context = &mut (*server_ptr).context;
            tool.execute(
                request.arguments.unwrap_or(json!({})),
                context,
                &mut *server_ptr,
            )
        }
    }
}

impl<CTX: Send + Sync + 'static> McpServer<CTX> {
    pub fn new(name: impl Into<String>, version: impl Into<String>, context: CTX) -> Self {
        Self::with_transport(name, version, context, StdioServerTransport::new())
    }

    pub fn with_transport(
        name: impl Into<String>,
        version: impl Into<String>,
        context: CTX,
        transport: impl ServerTransport + 'static,
    ) -> Self {
        let name = name.into();
        let version = version.into();

        let mut server = Self {
            name: name.clone(),
            version: version.clone(),
            transport: Box::new(transport),
            tools: HashMap::new(),
            handlers: HashMap::new(),
            context,
        };

        // Set up default handlers
        server.set_handler::<Initialize, _>(DefaultInitializeHandler { name, version });
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
        let handler_fn = HandlerFn::new(move |params, server| {
            let request: CMD::Request = serde_json::from_value(params)?;
            let response = handler.handle(request, server)?;
            Ok(serde_json::to_value(response)?)
        });

        self.handlers.insert(CMD::COMMAND, handler_fn);
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

    pub fn context(&self) -> &CTX {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut CTX {
        &mut self.context
    }

    pub fn start(&mut self) -> ! {
        eprintln!(
            "MCP Server `{}` started, version {}",
            self.name, self.version
        );
        let result: Result<(), McpError> = loop {
            match self.transport.read_message() {
                Ok(message) => {
                    debug!("Received message: {:?}", message);
                    match message {
                        JsonRpcMessage::Request(request) => {
                            if let Err(e) = self.handle_request(request) {
                                if e.is_shutdown() {
                                    std::process::exit(0); // Clean shutdown
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

    fn handle_request(&mut self, request: JsonRpcRequest) -> Result<(), McpError> {
        let self_ptr = self as *mut McpServer<CTX>;

        let response = if let Some(handler) = self.handlers.get(request.method.as_str()) {
            match unsafe { handler.handle(request.params.unwrap_or(json!({})), &mut *self_ptr) } {
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

        self.transport
            .write_message(&JsonRpcMessage::Response(response))?;
        Ok(())
    }

    fn handle_notification(&mut self, notification: JsonRpcNotification) -> Result<(), McpError> {
        debug!("Received notification: {:?}", notification);
        Ok(())
    }

    pub fn send_notification(&mut self, notification: JsonRpcNotification) -> Result<(), McpError> {
        self.transport
            .write_message(&JsonRpcMessage::Notification(notification))
    }
}

// Handler function type for type-erased command handling
struct HandlerFn<CTX> {
    handle_fn: Box<dyn Fn(Value, &mut McpServer<CTX>) -> Result<Value, McpError> + Send + Sync>,
}

impl<CTX> HandlerFn<CTX> {
    fn new<F>(f: F) -> Self
    where
        F: Fn(Value, &mut McpServer<CTX>) -> Result<Value, McpError> + Send + Sync + 'static,
    {
        Self {
            handle_fn: Box::new(f),
        }
    }

    fn handle(&self, params: Value, server: &mut McpServer<CTX>) -> Result<Value, McpError> {
        (self.handle_fn)(params, server)
    }
}

#[cfg(test)]
mod tests {
    // Tests would go here...
}
