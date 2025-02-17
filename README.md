
# sovran_mcp

A synchronous Rust client for the Model Context Protocol (MCP).

## Overview

`sovran-mcp` provides a clean, synchronous interface for interacting with MCP servers. It handles:
- Tool execution and discovery
- Prompt management
- Resource handling and subscriptions
- Server capability detection

## Usage

Add to your `Cargo.toml`:
```toml
[dependencies]
sovran-mcp = "0.3.1"
```

Basic example:
```rust
use sovran_mcp::{McpClient, transport::StdioTransport};

fn main() -> Result<(), sovran_mcp::McpError> {
    // Create and start client
    let transport = StdioTransport::new("npx", &["-y", "@modelcontextprotocol/server-everything"])?;
    let mut client = McpClient::new(transport, None, None);
    client.start()?;

    // List available tools
    if client.supports_tools() {
        let tools = client.list_tools()?;
        println!("Available tools: {:?}", tools);
        
        // Call a tool
        let response = client.call_tool(
            "echo".to_string(),
            Some(serde_json::json!({
                "message": "Hello, MCP!"
            }))
        )?;
    }

    // Clean up
    client.stop()?;
    Ok(())
}
```

## Features

### Tool Operations
- List available tools
- Execute tools with arguments
- Handle tool responses (text, images, resources)

### Prompt Management
- List available prompts
- Retrieve prompts with arguments
- Handle multi-part prompt messages

### Resource Handling
- List available resources
- Read resource contents
- Subscribe to resource changes
- Handle resource update notifications

### Server Capabilities
- Protocol version detection
- Feature availability checking
- Experimental feature support

## Handler Support

### Sampling Handler
Support for server-initiated LLM completions:
```rust
use sovran_mcp::types::*;

struct MySamplingHandler;
impl SamplingHandler for MySamplingHandler {
    fn handle_message(&self, request: CreateMessageRequest) -> Result<CreateMessageResponse, McpError> {
        // Process completion request
        Ok(CreateMessageResponse {
            content: MessageContent::Text(TextContent { 
                text: "Response".to_string() 
            }),
            model: "test-model".to_string(),
            role: Role::Assistant,
            stop_reason: Some("complete".to_string()),
            meta: None,
        })
    }
}
```

### Notification Handler
Support for resource update notifications:
```rust
use sovran_mcp::types::*;
use url::Url;

struct MyNotificationHandler;
impl NotificationHandler for MyNotificationHandler {
    fn handle_resource_update(&self, uri: &Url) -> Result<(), McpError> {
        println!("Resource updated: {}", uri);
        Ok(())
    }
}
```

## Error Handling

The crate uses `McpError` for comprehensive error handling, covering:
- Transport errors (I/O, connection issues)
- Protocol errors (JSON-RPC, serialization)
- Capability errors (unsupported features)
- Request timeouts
- Command failures

## License

MIT License

## Contributing

Contributions welcome! Please feel free to submit a Pull Request.