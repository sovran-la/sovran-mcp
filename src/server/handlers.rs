use crate::server::McpServer;
use crate::types::{
    CallTool, CallToolRequest, CallToolResponse, Implementation, Initialize, InitializeRequest,
    InitializeResponse, ListTools, ListToolsRequest, ListToolsResponse, McpCommand, McpError,
    ServerCapabilities, Shutdown, ShutdownRequest, ShutdownResponse, ToolDefinition,
    LATEST_PROTOCOL_VERSION,
};
use serde_json::json;

/// Command handler trait
pub trait CommandHandler<CMD: McpCommand, CTX>: Send + Sync {
    fn handle(
        &self,
        request: CMD::Request,
        server: &mut McpServer<CTX>,
    ) -> Result<CMD::Response, McpError>;
}

/// Default handlers
pub struct DefaultInitializeHandler {
    name: String,
    version: String,
}

impl DefaultInitializeHandler {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
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

pub struct DefaultShutdownHandler;
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

pub struct DefaultListToolsHandler;
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

pub struct DefaultCallToolHandler;
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
