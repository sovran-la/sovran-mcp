mod stdio;
use crate::messaging::JsonRpcMessage;
use crate::McpError;
pub use stdio::*;

/// Basic transport trait for sending/receiving messages
pub trait Transport: Send + Sync {
    /// Send a message to the transport
    fn send(&self, message: &JsonRpcMessage) -> Result<(), McpError>;

    /// Receive a message from the transport
    fn receive(&self) -> Result<JsonRpcMessage, McpError>;

    /// Open the transport
    fn open(&self) -> Result<(), McpError>;

    /// Close the transport
    fn close(&self) -> Result<(), McpError>;
}
