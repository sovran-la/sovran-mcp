// commands.rs
use crate::types::{
    CallToolRequest, CallToolResponse, GetPromptRequest, GetPromptResponse, InitializeRequest,
    InitializeResponse, ListPromptsRequest, ListPromptsResponse, ListResourcesRequest,
    ListResourcesResponse, ListToolsRequest, ListToolsResponse, ReadResourceRequest,
    ReadResourceResponse, SubscribeRequest, UnsubscribeRequest,
};
use crate::types::{CreateMessageRequest, CreateMessageResponse, EmptyResult};
use serde::{Deserialize, Serialize};

pub trait McpCommand {
    const COMMAND: &'static str;
    type Request: Serialize;
    type Response: for<'de> Deserialize<'de>;
}

#[derive(Debug, Clone)]
pub struct Initialize;
impl McpCommand for Initialize {
    const COMMAND: &'static str = "initialize";
    type Request = InitializeRequest;
    type Response = InitializeResponse;
}

#[derive(Debug, Clone)]
pub struct ListTools;
impl McpCommand for ListTools {
    const COMMAND: &'static str = "tools/list";
    type Request = ListToolsRequest;
    type Response = ListToolsResponse;
}

#[derive(Debug, Clone)]
pub struct CallTool;
impl McpCommand for CallTool {
    const COMMAND: &'static str = "tools/call";
    type Request = CallToolRequest;
    type Response = CallToolResponse;
}

#[derive(Debug, Clone)]
pub struct ListPrompts;
impl McpCommand for ListPrompts {
    const COMMAND: &'static str = "prompts/list";
    type Request = ListPromptsRequest;
    type Response = ListPromptsResponse;
}

#[derive(Debug, Clone)]
pub struct GetPrompt;
impl McpCommand for GetPrompt {
    const COMMAND: &'static str = "prompts/get";
    type Request = GetPromptRequest;
    type Response = GetPromptResponse;
}

#[derive(Debug, Clone)]
pub struct ListResources;
impl McpCommand for ListResources {
    const COMMAND: &'static str = "resources/list";
    type Request = ListResourcesRequest;
    type Response = ListResourcesResponse;
}

#[derive(Debug, Clone)]
pub struct ReadResource;
impl McpCommand for ReadResource {
    const COMMAND: &'static str = "resources/read";
    type Request = ReadResourceRequest;
    type Response = ReadResourceResponse;
}

#[derive(Debug, Clone)]
pub struct Subscribe;
impl McpCommand for Subscribe {
    const COMMAND: &'static str = "resources/subscribe";
    type Request = SubscribeRequest;
    type Response = EmptyResult; // Just returns {}
}

#[derive(Debug, Clone)]
pub struct Unsubscribe;
impl McpCommand for Unsubscribe {
    const COMMAND: &'static str = "resources/unsubscribe";
    type Request = UnsubscribeRequest;
    type Response = EmptyResult;
}

#[derive(Debug, Clone)]
pub struct CreateMessage;
impl McpCommand for CreateMessage {
    const COMMAND: &'static str = "sampling/createMessage";
    type Request = CreateMessageRequest;
    type Response = CreateMessageResponse;
}
