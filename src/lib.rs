// types is shared between client and server
pub mod types;
pub use types::McpError;

// client + transport only available with client feature
#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub use client::McpClient;

// server only available with server feature
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub use server::{McpServer, McpTool};

#[cfg(test)]
mod tests;
