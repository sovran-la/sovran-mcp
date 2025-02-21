#[cfg(feature = "client")]
use {
    crate::client::transport::Transport,
    crate::types::{McpError, NotificationHandler, SamplingHandler},
    std::collections::HashMap,
    std::sync::mpsc::Sender,
    std::sync::{Arc, Mutex},
    tracing::{debug, warn},
    url::Url,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

//
// Core JSON-RPC Types
// These types represent the basic building blocks of the JSON-RPC protocol
//

/// Request ID type
pub type RequestId = u64;

/// JSON RPC version type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct JsonRpcVersion(String);

impl Default for JsonRpcVersion {
    fn default() -> Self {
        JsonRpcVersion("2.0".to_owned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Response(JsonRpcResponse),
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JsonRpcRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub jsonrpc: JsonRpcVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct JsonRpcResponse {
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub jsonrpc: JsonRpcVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

//
// Notification System
// Types and implementations for the MCP notification system
//

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum NotificationMethod {
    #[serde(rename = "notifications/initialized")]
    Initialized,
    #[serde(rename = "notifications/progress")]
    Progress,
    #[serde(rename = "notifications/resources/updated")]
    ResourceUpdated,
    #[serde(rename = "notifications/resources/list_changed")]
    ResourceListChanged,
    #[serde(rename = "notifications/tools/list_changed")]
    ToolListChanged,
    #[serde(rename = "notifications/prompts/list_changed")]
    PromptListChanged,
    #[serde(rename = "notifications/message")]
    LogMessage,
}

impl NotificationMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initialized => "notifications/initialized",
            Self::Progress => "notifications/progress",
            Self::ResourceUpdated => "notifications/resources/updated",
            Self::ResourceListChanged => "notifications/resources/list_changed",
            Self::ToolListChanged => "notifications/tools/list_changed",
            Self::PromptListChanged => "notifications/prompts/list_changed",
            Self::LogMessage => "notifications/message",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressParams {
    pub progress_token: String,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUpdatedParams {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogMessageParams {
    pub level: LogLevel,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NotificationParams {
    Progress(ProgressParams),
    ResourceUpdated(ResourceUpdatedParams),
    LogMessage(LogMessageParams),
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct JsonRpcNotification {
    #[serde(serialize_with = "serialize_notification_method")]
    pub method: NotificationMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<NotificationParams>,
    pub jsonrpc: JsonRpcVersion,
}

fn serialize_notification_method<S>(
    method: &NotificationMethod,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(method.as_str())
}

impl JsonRpcNotification {
    pub fn new(method: NotificationMethod, params: Option<NotificationParams>) -> Self {
        Self {
            method,
            params,
            jsonrpc: JsonRpcVersion::default(),
        }
    }

    pub fn initialized() -> Self {
        Self::new(NotificationMethod::Initialized, None)
    }

    pub fn progress(token: impl Into<String>, progress: f64, total: Option<f64>) -> Self {
        Self::new(
            NotificationMethod::Progress,
            Some(NotificationParams::Progress(ProgressParams {
                progress_token: token.into(),
                progress,
                total,
            })),
        )
    }

    pub fn resource_updated(uri: impl Into<String>) -> Self {
        Self::new(
            NotificationMethod::ResourceUpdated,
            Some(NotificationParams::ResourceUpdated(ResourceUpdatedParams {
                uri: uri.into(),
            })),
        )
    }

    pub fn log_message(level: LogLevel, data: impl Into<Value>, logger: Option<String>) -> Self {
        Self::new(
            NotificationMethod::LogMessage,
            Some(NotificationParams::LogMessage(LogMessageParams {
                level,
                data: data.into(),
                logger,
            })),
        )
    }
}

//
// Message Handler
// Implementation of the MCP message handling system
//
#[cfg(feature = "client")]
pub struct MessageHandler<T: Transport + 'static> {
    transport: Arc<T>,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
    sampling_handler: Option<Arc<Box<dyn SamplingHandler + Send>>>,
    notification_handler: Option<Arc<Box<dyn NotificationHandler + Send>>>,
}

#[cfg(feature = "client")]
impl<T: Transport + 'static> MessageHandler<T> {
    pub fn new(
        transport: Arc<T>,
        pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
        sampling_handler: Option<Arc<Box<dyn SamplingHandler + Send>>>,
        notification_handler: Option<Arc<Box<dyn NotificationHandler + Send>>>,
    ) -> Self {
        Self {
            transport,
            pending_requests,
            sampling_handler,
            notification_handler,
        }
    }

    pub fn handle_message(&self, message: JsonRpcMessage) -> Result<(), McpError> {
        match message {
            JsonRpcMessage::Request(request) => self.handle_request(request),
            JsonRpcMessage::Response(response) => self.handle_response(response),
            JsonRpcMessage::Notification(notification) => self.handle_notification(notification),
        }
    }

    pub fn handle_request(&self, request: JsonRpcRequest) -> Result<(), McpError> {
        match request.method.as_str() {
            "sampling/createMessage" => {
                if let Some(handler) = &self.sampling_handler {
                    self.handle_sampling_request(handler, &request)?;
                }
                Ok(())
            }
            _ => {
                warn!("Unknown request method: {}", request.method);
                Ok(())
            }
        }
    }

    pub fn handle_sampling_request(
        &self,
        handler: &Arc<Box<dyn SamplingHandler + Send>>,
        request: &JsonRpcRequest,
    ) -> Result<(), McpError> {
        debug!("Processing sampling request...");
        let params =
            serde_json::from_value(request.clone().params.unwrap_or(serde_json::Value::Null))?;
        let result = handler.handle_message(params)?;
        let value = serde_json::to_value(result)?;

        let response = JsonRpcResponse {
            id: request.id,
            result: Some(value),
            error: None,
            jsonrpc: Default::default(),
        };

        debug!("Sending sampling response...");
        self.transport.send(&JsonRpcMessage::Response(response))?;
        debug!("Sampling response sent");
        Ok(())
    }

    pub fn handle_response(&self, response: JsonRpcResponse) -> Result<(), McpError> {
        debug!("Got response with id: {}", response.id);
        let mut pending = self.pending_requests.lock().unwrap();
        debug!(
            "Current pending request IDs: {:?}",
            pending.keys().collect::<Vec<_>>()
        );

        if let Some(sender) = pending.remove(&response.id) {
            sender
                .send(response.clone())
                .map_err(|e| McpError::SendError {
                    id: response.id,
                    source: e,
                })?;
        }
        Ok(())
    }

    pub fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        debug!("Got notification: method={:?}", notification.method);

        if let Some(handler) = &self.notification_handler {
            match notification.method {
                NotificationMethod::ResourceUpdated => {
                    if let Some(NotificationParams::ResourceUpdated(params)) = notification.params {
                        let url = Url::parse(&params.uri)?;
                        handler.handle_resource_update(&url)?;
                    }
                }
                NotificationMethod::LogMessage => {
                    if let Some(NotificationParams::LogMessage(params)) = notification.params {
                        handler.handle_log_message(&params.level, &params.data, &params.logger);
                    }
                }
                NotificationMethod::Progress => {
                    if let Some(NotificationParams::Progress(params)) = notification.params {
                        handler.handle_progress_update(
                            &params.progress_token,
                            &params.progress,
                            &params.total,
                        );
                    }
                }
                NotificationMethod::Initialized => {
                    debug!("Server initialization completed");
                    handler.handle_initialized();
                }
                NotificationMethod::ToolListChanged
                | NotificationMethod::PromptListChanged
                | NotificationMethod::ResourceListChanged => {
                    debug!("List changed notification: {:?}", notification.method);
                    handler.handle_list_changed(&notification.method);
                }
            }
        } else {
            debug!("Received notification but no handler is registered");
        }
        Ok(())
    }
}
