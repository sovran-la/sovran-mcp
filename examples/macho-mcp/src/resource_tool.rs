use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use sovran_mcp::{McpError, McpTool};
use sovran_mcp::server::server::{McpResource, McpToolServer};
use sovran_mcp::types::{CallToolResponse, ResourceContent, TextResourceContents, ToolResponseContent};
use crate::MachoContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChampionshipBelt {
    pub name: String,
    pub weight_class: String,
    pub defense_count: u32,
    pub signature_move: String,
}


impl McpResource for ChampionshipBelt {
    fn uri(&self) -> String {
        format!("macho://belts/{}", self.name.to_lowercase().replace(' ', "-"))
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn mime_type(&self) -> String {
        "application/json".into()
    }

    fn content(&self) -> ResourceContent {
        ResourceContent::Text(TextResourceContents {
            uri: Url::parse(&*self.uri()).unwrap(),
            text: serde_json::to_string_pretty(&self).unwrap(),
            mime_type: Some(self.mime_type().to_string()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChampionshipManagerTool;

impl McpTool<MachoContext> for ChampionshipManagerTool {
    fn name(&self) -> &str {
        "championship-manager"
    }

    fn description(&self) -> &str {
        "MANAGE THE GOLD, BROTHER! THE CREAM RISES TO THE TOP! OHHH YEAHHH!"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "defend", "add", "remove"],
                    "description": "What championship action are we taking, BROTHER?!"
                },
                "belt_id": {
                    "type": "string",
                    "description": "Which piece of gold are we talking about?!"
                },
                "defense_details": {
                    "type": "object",
                    "properties": {
                        "opponent": { "type": "string" },
                        "signature_move": { "type": "string" }
                    },
                    "required": ["opponent", "signature_move"]
                }
            },
            "required": ["action"]
        })
    }

    fn execute(
        &self,
        args: Value,
        _context: &mut MachoContext,
        tool_server: &McpToolServer,
    ) -> Result<CallToolResponse, McpError> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidArguments("WHATCHA GONNA DO BROTHER?!".into()))?;

        match action {
            "list" => {
                let belts = tool_server.list_resources();

                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!(
                            "THE MACHO MAN'S COLLECTION OF {} CHAMPIONSHIPS! DIG IT!\n{}",
                            belts.len(),
                            belts.join("\n")
                        ),
                    }],
                    is_error: None,
                    meta: None,
                })
            },
            "defend" => {
                let belt_id = args.get("belt_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHICH BELT ARE WE DEFENDING?!".into()))?;

                // Get defense details
                let details = args.get("defense_details")
                    .and_then(|d| d.as_object())
                    .ok_or_else(|| McpError::InvalidArguments("HOW'D YOU DEFEND IT, BROTHER?!".into()))?;

                let opponent = details.get("opponent")
                    .and_then(|o| o.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHO'D YOU FIGHT, BROTHER?!".into()))?;

                let signature_move = details.get("signature_move")
                    .and_then(|s| s.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHAT'D YOU HIT 'EM WITH, BROTHER?!".into()))?;

                let uri = format!("macho://belts/{}", belt_id);

                // Get the belt
                let mut belt = tool_server.get_resource::<ChampionshipBelt>(&uri)?;

                // Update and save
                belt.defense_count += 1;
                belt.signature_move = signature_move.to_string();

                let defense_count = belt.defense_count;
                let belt_name = belt.name.clone();

                // Save the updated belt
                tool_server.set_resource(uri, belt)?;

                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!(
                            "OHHH YEAHHH! THE {} HAS BEEN DEFENDED FOR THE {}th TIME! \
                {} GOT DESTROYED BY THE {}! \
                THE CREAM RISES TO THE TOP! DIG IT!",
                            belt_name,
                            defense_count,
                            opponent,
                            signature_move
                        ),
                    }],
                    is_error: None,
                    meta: None,
                })
            }
            "add" => {
                let name = args.get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHAT'S THE BELT CALLED, BROTHER?!".into()))?;

                let weight_class = args.get("weight_class")
                    .and_then(|w| w.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHAT'S THE WEIGHT CLASS, BROTHER?!".into()))?;

                let signature_move = args.get("signature_move")
                    .and_then(|s| s.as_str())
                    .ok_or_else(|| McpError::InvalidArguments("WHAT'S THE FINISHING MOVE, BROTHER?!".into()))?;

                let belt = ChampionshipBelt {
                    name: name.to_string(),
                    weight_class: weight_class.to_string(),
                    defense_count: 0,
                    signature_move: signature_move.to_string(),
                };

                // Use set_resource to add the new belt
                let uri = belt.uri().to_string();
                tool_server.set_resource(uri, belt)?;

                Ok(CallToolResponse {
                    content: vec![ToolResponseContent::Text {
                        text: format!(
                            "OHHH YEAHHH! THE {} IS NOW IN PLAY! \
                        THE {} DIVISION BETTER WATCH OUT! DIG IT!",
                            name,
                            weight_class
                        ),
                    }],
                    is_error: None,
                    meta: None,
                })
            },
            _ => Err(McpError::InvalidArguments("WHATCHA TRYING TO DO, BROTHER?!".into()))
        }
    }
}