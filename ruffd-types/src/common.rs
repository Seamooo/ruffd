use serde::{Deserialize, Serialize};

use crate::RpcError;

#[derive(Debug, Deserialize)]
pub struct RpcMessage {
    pub jsonrpc: String,
    pub id: Option<lsp_types::NumberOrString>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcResponseError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    jsonrpc: String,
    id: Option<lsp_types::NumberOrString>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcResponseError>,
}

impl Default for RpcResponse {
    fn default() -> Self {
        Self {
            jsonrpc: String::from("2.0"),
            id: None,
            result: None,
            error: None,
        }
    }
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

impl RpcResponse {
    pub fn from_error(id: Option<lsp_types::NumberOrString>, err: RpcError) -> Self {
        Self {
            error: Some(RpcResponseError::from(err)),
            id,
            ..Default::default()
        }
    }
    pub fn from_result<T: Serialize>(id: lsp_types::NumberOrString, res: T) -> Self {
        Self {
            result: Some(serde_json::to_value(res).unwrap()),
            id: Some(id),
            ..Default::default()
        }
    }
}
