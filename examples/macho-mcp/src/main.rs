
mod basic_tool;
mod longrunning_tool;
mod resource_tool;

use crate::basic_tool::ElbowDropTool;
use sovran_mcp::server::McpServer;
use sovran_mcp::types::McpError;
use crate::longrunning_tool::PrepareToRumbleTool;
use crate::resource_tool::{ChampionshipBelt, ChampionshipManagerTool};

struct MachoContext {
    catchphrases: Vec<String>,
    elbow_drops: usize,
    belts: Vec<ChampionshipBelt>,
}

impl Default for MachoContext {
    fn default() -> Self {
        let mut belts = Vec::new();
        belts.push(
            ChampionshipBelt {
                name: "WWF World Heavyweight Championship".into(),
                weight_class: "Heavyweight".into(),
                defense_count: 371,
                signature_move: "Flying Elbow Drop".into(),
            }
        );
        belts.push(
            ChampionshipBelt {
                name: "Intercontinental Championship".into(),
                weight_class: "Heavyweight".into(),
                defense_count: 183,
                signature_move: "Savage Slam".into(),
            }
        );

        Self {
            catchphrases: vec![
                "OHHH YEAHHH!".into(),
                "SNAP INTO IT!".into(),
                "DIG IT!".into(),
                "THE CREAM RISES TO THE TOP!".into(),
                "ON BALANCE, OFF BALANCE, DOESN'T MATTER!".into(),
            ],
            elbow_drops: 0,
            belts,
        }
    }
}

fn main() -> Result<(), McpError> {
    eprintln!("MACHO MCP SERVER IS READY TO SNAP INTO IT! OHHH YEAHHH!");

    let context = MachoContext::default();
    let mut server = McpServer::new("macho-mcp", "1.0.0");

    server.add_tool(ElbowDropTool)?;
    server.add_tool(PrepareToRumbleTool)?;
    server.add_tool(ChampionshipManagerTool)?;

    // Add championship belt resources
    for belt in context.belts.iter() {
        server.add_resource(belt.clone())?
    }

    // Start the server - this handles all the stdin/stdout stuff for us!
    server.start(context)
}
