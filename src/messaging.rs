use crate::transport::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, Transport};
use crate::types::*;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use url::Url;
use tracing::{debug, warn};
use crate::McpError;

pub struct MessageHandler<T: Transport + 'static> {
    transport: Arc<T>,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<JsonRpcResponse>>>>,
    sampling_handler: Option<Arc<Box<dyn SamplingHandler + Send>>>,
    notification_handler: Option<Arc<Box<dyn NotificationHandler + Send>>>,
}

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
        let params = serde_json::from_value(request.clone().params.unwrap_or(serde_json::Value::Null))?;
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
        debug!("Current pending request IDs: {:?}", pending.keys().collect::<Vec<_>>());

        if let Some(sender) = pending.remove(&response.id) {
            sender.send(response.clone())
                .map_err(|e| McpError::SendError {
                    id: response.id,
                    source: e
                })?;
        }
        Ok(())
    }

    pub fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        debug!("Got notification: method={}", notification.method);
        match notification.method.as_str() {
            "notifications/resources/updated" => self.handle_resource_update(notification),
            _ => {
                warn!("Unknown notification method: {}", notification.method);
                Ok(())
            }
        }
    }

    pub fn handle_resource_update(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(handler) = &self.notification_handler {
            if let Some(params) = notification.params {
                if let Some(uri) = params.get("uri").and_then(|v| v.as_str()) {
                    let url = Url::parse(uri)?;
                    handler.handle_resource_update(&url)?;
                }
            }
        }
        Ok(())
    }
}
