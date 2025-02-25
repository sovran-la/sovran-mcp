use serde_json::{json, Value};
use sovran_mcp::{McpError, McpTool};
use sovran_mcp::server::server::McpToolServer;
use sovran_mcp::types::{CallToolResponse, JsonRpcNotification, LogLevel, ToolResponseContent};
use crate::MachoContext;

#[derive(Debug, Clone)]
pub struct PrepareToRumbleTool;

impl McpTool<MachoContext> for PrepareToRumbleTool {
    fn name(&self) -> &str {
        "prepare-to-rumble"
    }

    fn description(&self) -> &str {
        "SNAP INTO IT as the Macho Man gets ready for the ring! OHHH YEAHHH!"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "opponent": {
                    "type": "string",
                    "description": "WHO'S GONNA FEEL THE HEAT, BROTHER?!"
                },
                "intensity": {
                    "type": "integer",
                    "description": "HOW INTENSE IS THIS PREPARATION GONNA BE? (1-10)",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 5
                }
            },
            "required": ["opponent"]
        })
    }

    fn execute(
        &self,
        args: Value,
        _context: &mut MachoContext,
        tool_server: &McpToolServer,
    ) -> Result<CallToolResponse, McpError> {
        let opponent = args.get("opponent").and_then(|v| v.as_str()).ok_or_else(|| {
            McpError::InvalidArguments("WHO AM I PREPARING TO DESTROY, BROTHER?!".into())
        })?;

        let intensity = args.get("intensity").and_then(|v| v.as_u64()).unwrap_or(5);
        let steps = intensity as usize;

        // Get progress token if it exists
        let progress_token = args.get("_meta")
            .and_then(|m| m.get("progressToken"))
            .and_then(|t| t.as_str());

        let preparation_steps = [
            "OHHH YEAHHH! Applying the war paint!",
            "DIG IT! Lacing up the boots of destruction!",
            "SNAP INTO IT! Flexing the 24-inch pythons!",
            "THE CREAM RISES! Adjusting the championship belt!",
            "NOTHING MEANS NOTHING! Practicing elbow drops!",
            "SKY'S THE LIMIT! Warming up with chair throws!",
            "CAN YOU DIG IT? Testing the ropes!",
            "EXPECT THE UNEXPECTED! Practicing poses!",
            "ON BALANCE, OFF BALANCE! Final stretches!",
            "THE TOWER OF POWER! Ready to explode!"
        ];

        if let Some(token) = progress_token {
            for i in 1..=steps {
                // Dramatic pause between steps
                std::thread::sleep(std::time::Duration::from_secs(2));

                // Get step message (cycle through if more steps than messages)
                let step_message = preparation_steps[(i - 1) % preparation_steps.len()];

                // Log the current step
                tool_server.send_notification(JsonRpcNotification::log_message(
                    LogLevel::Info,
                    json!({
                        "action": "preparation_step",
                        "step": i,
                        "message": step_message
                    }),
                    Some("macho-mcp".to_string()),
                ))?;

                // Send progress update
                tool_server.send_progress(token, i as f64, Some(steps as f64))?;
            }
        }

        Ok(CallToolResponse {
            content: vec![ToolResponseContent::Text {
                text: format!(
                    "THE MACHO MAN IS READY TO THROW DOWN WITH {}! THE CREAM OF THE CROP IS COMING FOR YOU! DIG IT!",
                    opponent
                ),
            }],
            is_error: None,
            meta: None,
        })
    }
}