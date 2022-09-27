use crate::common::RpcResponse;
use crate::state::{ServerState, ServerStateHandles, ServerStateLocks};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

type RequestExec = fn(
    state: ServerStateHandles<'_>,
    id: Option<lsp_types::NumberOrString>,
    params: Option<serde_json::Value>,
) -> Pin<Box<dyn Send + Future<Output = RpcResponse> + '_>>;

type NotificationExec = fn(
    state: ServerStateHandles<'_>,
    id: Option<lsp_types::NumberOrString>,
    params: Option<serde_json::Value>,
) -> Pin<Box<dyn Send + Future<Output = Option<RpcResponse>> + '_>>;

type CreateLocks =
    fn(state: Arc<Mutex<ServerState>>) -> Pin<Box<dyn Send + Future<Output = ServerStateLocks>>>;

pub struct Request {
    pub exec: RequestExec,
    pub create_locks: CreateLocks,
}

pub struct Notification {
    pub exec: NotificationExec,
    pub create_locks: CreateLocks,
}
