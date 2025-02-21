// types is shared between client and server
pub mod types;

// client + transport only available with client feature
#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub mod transport;
#[cfg(feature = "client")]
pub use {client::McpClient, types::McpError};

// server only available with server feature
#[cfg(feature = "server")]
pub mod server;

#[cfg(test)]
mod tests;
