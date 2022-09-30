use std::io;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: &'static str,
}

pub struct RpcErrors {}

impl RpcErrors {
    pub const PARSE_ERROR: RpcError = RpcError {
        code: -32700,
        message: "Parse error",
    };
    pub const INVALID_REQUEST: RpcError = RpcError {
        code: -32600,
        message: "Invalid request",
    };
    pub const METHOD_NOT_FOUND: RpcError = RpcError {
        code: -32601,
        message: "Method not found",
    };
    pub const INVALID_PARAMS: RpcError = RpcError {
        code: -32602,
        message: "Invalid params",
    };
    pub const INTERNAL_ERROR: RpcError = RpcError {
        code: -32603,
        message: "Internal error",
    };
    pub const SERVER_NOT_INITIALIZED: RpcError = RpcError {
        code: -32002,
        message: "Server not initialized",
    };
    pub const UNKNOWN_ERROR_CODE: RpcError = RpcError {
        code: -32001,
        message: "Unknown error code",
    };
    pub const REQUEST_FAILED: RpcError = RpcError {
        code: -32803,
        message: "Request failed",
    };
    pub const SERVER_CANCELLED: RpcError = RpcError {
        code: -32802,
        message: "Server cancelled",
    };
    pub const CONTENT_MODIFIED: RpcError = RpcError {
        code: lsp_types::error_codes::CONTENT_MODIFIED,
        message: "Content modified",
    };
    pub const REQUEST_CANCELLED: RpcError = RpcError {
        code: lsp_types::error_codes::REQUEST_CANCELLED,
        message: "Request cancelled",
    };
}

#[derive(Error, Debug)]
pub enum AggAvlTreeError {
    #[error("Index out of bounds")]
    IndexOutOfBounds,
}

#[derive(Error, Debug)]
pub enum RopeError {
    #[error("Index out of bounds")]
    IndexOutOfBounds,
}

#[derive(Error, Debug)]
pub enum DocumentError {
    #[error("Index out of bounds")]
    IndexOutOfBounds,
    #[error("Row out of bounds")]
    RowOutOfBounds,
    #[error("Column out of bounds")]
    ColOutOfBounds,
    #[error(transparent)]
    AggAvlTreeError(#[from] AggAvlTreeError),
    #[error(transparent)]
    RopeError(#[from] RopeError),
}

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Unknown encoding {0}")]
    UnknownEncoding(String),
    #[error("DocumentError: {0}")]
    DocumentError(#[from] DocumentError),
    #[error("Uri: '{0}' not open")]
    EditUnopenedDocument(lsp_types::Url),
    #[error("Unexpected None")]
    UnexpectedNone,
    #[error("Internal Error: {0}")]
    InternalError(#[from] anyhow::Error),
    #[error("Cannot convert uri to path: {0}")]
    UriToPathError(lsp_types::Url),
}

impl From<io::Error> for RpcError {
    fn from(_: io::Error) -> Self {
        RpcErrors::INTERNAL_ERROR
    }
}

impl From<serde_json::Error> for RpcError {
    fn from(_: serde_json::Error) -> Self {
        RpcErrors::PARSE_ERROR
    }
}

impl From<RuntimeError> for RpcError {
    fn from(err: RuntimeError) -> Self {
        // tmp logging for runtime errors
        dbg!(err);
        RpcErrors::INTERNAL_ERROR
    }
}

pub type RpcResult<T> = Result<T, RpcError>;
