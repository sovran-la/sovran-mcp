use crate::types::{JsonRpcMessage, McpError};

/// Server-side transport trait - simpler than the client transport
pub trait ServerTransport: Send + Sync {
    fn read_message(&mut self) -> Result<JsonRpcMessage, McpError>;
    fn write_message(&mut self, message: &JsonRpcMessage) -> Result<(), McpError>;
    fn close(&mut self) -> Result<(), McpError>;
}

mod stdio;
pub use stdio::StdioServerTransport;
