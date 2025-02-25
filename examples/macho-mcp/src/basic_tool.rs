use serde_json::{json, Value};
use sovran_mcp::{McpError, McpTool};
use sovran_mcp::server::server::McpToolServer;
use sovran_mcp::types::{CallToolResponse, JsonRpcNotification, LogLevel, ToolResponseContent};
use crate::MachoContext;

#[derive(Debug, Clone)]
pub struct ElbowDropTool;

impl McpTool<MachoContext> for ElbowDropTool {
    fn name(&self) -> &str {
        "elbow-drop"
    }

    fn description(&self) -> &str {
        "The cream of the crop! Nothing means nothing! YEAH!"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Who's getting dropped, brother?"
                },
                "intensity": {
                    "type": "integer",
                    "description": "How many times do ya wanna snap into it? (1-10)",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 5
                }
            },
            "required": ["target"]
        })
    }

    fn execute(
        &self,
        args: Value,
        context: &mut MachoContext,
        server: &McpToolServer,
    ) -> Result<CallToolResponse, McpError> {
        let target = args.get("target").and_then(|v| v.as_str()).ok_or_else(|| {
            McpError::InvalidArguments("WHO AM I SUPPOSED TO DROP BROTHER?!".into())
        })?;

        let intensity = args.get("intensity").and_then(|v| v.as_u64()).unwrap_or(5);

        context.elbow_drops += 1;
        let catchphrase = &context.catchphrases[context.elbow_drops % context.catchphrases.len()];

        // Send a log message about this epic elbow drop!
        server.send_notification(JsonRpcNotification::log_message(
            LogLevel::Info,
            json!({
                "action": "elbow_drop",
                "target": target,
                "intensity": intensity,
                "catchphrase": catchphrase
            }),
            Some("macho-mcp".to_string()),
        ))?;

        Ok(CallToolResponse {
            content: vec![ToolResponseContent::Text {
                text: format!(
                    "{} {} just got DROPPED from {} feet! DIG IT!",
                    catchphrase,
                    target,
                    intensity * 10
                ),
            }],
            is_error: None,
            meta: None,
        })
    }
}