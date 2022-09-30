use serde::{Deserialize, Serialize};

use crate::error::RpcError;

const JSON_RPC_VERSION: &str = "2.0";

#[derive(Debug, Deserialize, Serialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: lsp_types::NumberOrString,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

impl RpcNotification {
    pub fn new(method: String, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            method,
            params,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request(RpcRequest),
    Notification(RpcNotification),
    Response(RpcResponseMessage),
}

impl RpcMessage {
    pub fn validate(&self) -> bool {
        let jsonrpc = match self {
            Self::Request(x) => x.jsonrpc.as_str(),
            Self::Notification(x) => x.jsonrpc.as_str(),
            Self::Response(x) => match x {
                RpcResponseMessage::Result(x) => x.jsonrpc.as_str(),
                RpcResponseMessage::Error(x) => x.jsonrpc.as_str(),
            },
        };
        jsonrpc.eq(JSON_RPC_VERSION)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseMessageResult {
    pub jsonrpc: String,
    pub id: Option<lsp_types::NumberOrString>,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseMessageError {
    pub jsonrpc: String,
    pub id: Option<lsp_types::NumberOrString>,
    pub error: RpcResponseError,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcResponseMessage {
    Result(RpcResponseMessageResult),
    Error(RpcResponseMessageError),
}

impl From<RpcError> for RpcResponseError {
    fn from(err: RpcError) -> Self {
        Self {
            code: err.code,
            message: err.message.to_string(),
            data: None,
        }
    }
}

impl RpcResponseMessage {
    pub fn from_error(id: Option<lsp_types::NumberOrString>, err: RpcError) -> Self {
        Self::Error(RpcResponseMessageError {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            error: RpcResponseError::from(err),
            id,
        })
    }
    pub fn from_result<T: Serialize>(id: lsp_types::NumberOrString, res: T) -> Self {
        Self::Result(RpcResponseMessageResult {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            result: Some(serde_json::to_value(res).unwrap()),
            id: Some(id),
        })
    }
}

impl From<RpcRequest> for RpcMessage {
    fn from(val: RpcRequest) -> Self {
        Self::Request(val)
    }
}

impl From<RpcNotification> for RpcMessage {
    fn from(val: RpcNotification) -> Self {
        Self::Notification(val)
    }
}

impl From<RpcResponseMessage> for RpcMessage {
    fn from(val: RpcResponseMessage) -> Self {
        Self::Response(val)
    }
}
