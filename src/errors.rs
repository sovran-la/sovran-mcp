use url::ParseError;
use crate::transport::{JsonRpcError, JsonRpcResponse};

#[derive(Debug)]
pub enum McpError {
    // Transport/IO related
    IoError(std::io::Error),
    TransportNotOpen,

    // Protocol related
    JsonRpc(JsonRpcError),
    SerializationError(serde_json::Error),

    // General errors
    ProcessSpawnError,
    StdinNotAvailable,
    StdoutNotAvailable,
    ClientNotInitialized,
    UnsupportedCapability(&'static str),  // e.g. "tools", "prompts", etc
    RequestTimeout {
        method: String,
        source: std::sync::mpsc::RecvTimeoutError,
    },
    CommandFailed {
        command: String,
        error: JsonRpcError,
    },
    MissingResult,
    SendError {
        id: u64,
        source: std::sync::mpsc::SendError<JsonRpcResponse>
    },
    UrlParse(ParseError),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::TransportNotOpen => write!(f, "Transport not opened"),
            Self::JsonRpc(e) => write!(f, "JSON-RPC error: {}", e.message),
            Self::SerializationError(e) => write!(f, "Serialization error: {}", e),
            Self::ProcessSpawnError => write!(f, "Failed to spawn child process"),
            Self::StdinNotAvailable => write!(f, "Child process stdin not available"),
            Self::StdoutNotAvailable => write!(f, "Child process stdout not available"),
            Self::ClientNotInitialized => write!(f, "Client not initialized"),
            Self::UnsupportedCapability(cap) => write!(f, "Server does not support {}", cap),
            Self::RequestTimeout { method, source } => {
                write!(f, "Timeout waiting for response to {}: {}", method, source)
            }
            Self::CommandFailed { command, error } => {
                write!(f, "{} failed: {}", command, error.message)
            }
            Self::MissingResult => write!(f, "No result in response"),
            Self::SendError { id, source } => {
                write!(f, "Failed to send response for id {}: {}", id, source)
            }
            Self::UrlParse(e) => write!(f, "Invalid URL: {}", e),
        }
    }
}

impl std::error::Error for McpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            Self::SerializationError(e) => Some(e),
            _ => None
        }
    }
}

// Conversion from std::io::Error
impl From<std::io::Error> for McpError {
    fn from(error: std::io::Error) -> Self {
        Self::IoError(error)
    }
}

// Conversion from serde_json::Error
impl From<serde_json::Error> for McpError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerializationError(error)
    }
}

// Conversion from Url::ParseError
impl From<ParseError> for McpError {
    fn from(error: ParseError) -> Self {
        Self::UrlParse(error)
    }
}
