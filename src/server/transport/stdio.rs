use crate::server::transport::ServerTransport;
use crate::types::JsonRpcMessage;
use crate::McpError;
use std::io;
use std::io::Write;

// Default stdio transport implementation
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
