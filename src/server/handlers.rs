use crate::server::McpServer;
use crate::types::*;
use serde_json::json;

/// Command handler trait
pub trait CommandHandler<CMD: McpCommand, CTX>: Send + Sync {
    fn handle(
        &self,
        request: CMD::Request,
        server: &mut McpServer<CTX>,
        context: &mut CTX,
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

impl<CTX: Send + Sync + 'static> CommandHandler<Initialize, CTX> for DefaultInitializeHandler {
    fn handle(
        &self,
        _request: InitializeRequest,
        _server: &mut McpServer<CTX>,
        _context: &mut CTX,  // Unused but required by trait
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
impl<CTX: Send + Sync + 'static> CommandHandler<Shutdown, CTX> for DefaultShutdownHandler {
    fn handle(
        &self,
        _request: ShutdownRequest,
        _server: &mut McpServer<CTX>,
        _context: &mut CTX,  // Unused but required by trait
    ) -> Result<ShutdownResponse, McpError> {
        Ok(ShutdownResponse { meta: None })
    }
}

pub struct DefaultListToolsHandler;
impl<CTX: Send + Sync + 'static> CommandHandler<ListTools, CTX> for DefaultListToolsHandler {
    fn handle(
        &self,
        _request: ListToolsRequest,
        server: &mut McpServer<CTX>,
        _context: &mut CTX,  // Unused but required by trait
    ) -> Result<ListToolsResponse, McpError> {
        Ok(ListToolsResponse {
            tools: server.tool_definitions(),
            next_cursor: None,
            meta: None,
        })
    }
}

pub struct DefaultCallToolHandler;
impl<CTX: Send + Sync + 'static> CommandHandler<CallTool, CTX> for DefaultCallToolHandler {
    fn handle(
        &self,
        request: CallToolRequest,
        server: &mut McpServer<CTX>,
        context: &mut CTX
    ) -> Result<CallToolResponse, McpError> {
        server.execute_tool(
            &request.name,
            request.arguments.unwrap_or(json!({})),
            context
        )
    }
}

pub struct DefaultListResourcesHandler;
impl<CTX: Send + Sync + 'static> CommandHandler<ListResources, CTX> for DefaultListResourcesHandler {
    fn handle(
        &self,
        _request: ListResourcesRequest,
        server: &mut McpServer<CTX>,
        _context: &mut CTX
    ) -> Result<ListResourcesResponse, McpError> {
        let resources = server.list_resources();
        Ok(ListResourcesResponse {
            resources,
            next_cursor: None,
            meta: None,
        })
    }
}

pub struct DefaultReadResourceHandler;
impl<CTX: Send + Sync + 'static> CommandHandler<ReadResource, CTX> for DefaultReadResourceHandler {
    fn handle(
        &self,
        request: ReadResourceRequest,
        server: &mut McpServer<CTX>,
        _context: &mut CTX
    ) -> Result<ReadResourceResponse, McpError> {
        let resource = server.get_resource_content(request.uri.to_string())?;
        Ok(ReadResourceResponse {
            contents: resource,
            meta: None,
        })

    }
}
