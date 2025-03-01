use crate::types::{JsonRpcError, JsonRpcResponse};
use std::error::Error as StdError;
use std::fmt;
use thiserror::Error;
use url::ParseError;

#[derive(Debug, Error)]
pub enum McpError {
    // Transport Errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Transport not open")]
    TransportNotOpen,
    #[error("Failed to spawn process")]
    ProcessSpawnError,
    #[error("Stdin not available")]
    StdinNotAvailable,
    #[error("Stdout not available")]
    StdoutNotAvailable,
    #[error("Stderr not available")]
    StderrNotAvailable,

    // Protocol Errors
    #[error("JSON-RPC error: {0}")]
    JsonRpc(#[from] JsonRpcError),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("URL parse error: {0}")]
    UrlParse(#[from] ParseError),
    #[error("Missing result in response")]
    MissingResult,

    // Client Errors
    #[error("Client not initialized")]
    ClientNotInitialized,
    #[error("Unsupported capability: {0}")]
    UnsupportedCapability(&'static str),
    #[error("Request timeout for method '{method}': {source}")]
    RequestTimeout {
        method: String,
        #[source]
        source: std::sync::mpsc::RecvTimeoutError,
    },
    #[error("Command '{command}' failed: {error}")]
    CommandFailed {
        command: String,
        error: JsonRpcError,
    },
    #[error("Failed to send response for request {id}: {source}")]
    SendError {
        id: u64,
        #[source]
        source: std::sync::mpsc::SendError<JsonRpcResponse>,
    },
    #[error("Terminating the client thread failed.")]
    ThreadJoinFailed,

    // Server errors
    #[error("Unknown tool: {0}")]
    UnknownTool(String),
    #[error("Duplicate tool: {0}")]
    DuplicateTool(String),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("Invalid resource: {0}")]
    InvalidResource(String),

    // Other Errors
    #[error("{0}")]
    Other(String),

    #[error("MCP Server Shutdown")]
    ServerShutdown,
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl StdError for JsonRpcError {}

impl McpError {
    pub fn is_shutdown(&self) -> bool {
        matches!(self, McpError::ServerShutdown)
    }
}
