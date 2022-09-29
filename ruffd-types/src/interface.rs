use crate::common::RpcResponseMessage;
use crate::state::{ServerState, ServerStateHandles, ServerStateLocks};
use crate::RpcMessage;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

type RequestExec = fn(
    state: ServerStateHandles<'_>,
    scheduler_channel: Sender<ScheduledTask>,
    id: lsp_types::NumberOrString,
    params: Option<serde_json::Value>,
) -> Pin<Box<dyn Send + Future<Output = RpcResponseMessage> + '_>>;

type NotificationExec = fn(
    state: ServerStateHandles<'_>,
    scheduler_channel: Sender<ScheduledTask>,
    params: Option<serde_json::Value>,
)
    -> Pin<Box<dyn Send + Future<Output = Option<RpcResponseMessage>> + '_>>;

type CreateLocks =
    fn(state: Arc<Mutex<ServerState>>) -> Pin<Box<dyn Send + Future<Output = ServerStateLocks>>>;

pub type ServerNotificationExec = Box<
    dyn FnOnce(
            ServerStateHandles<'_>,
            Sender<ScheduledTask>,
        ) -> Pin<Box<dyn Send + Future<Output = RpcMessage> + '_>>
        + Send,
>;

pub type ServerRequestExec = Box<
    dyn FnOnce(
            ServerStateHandles<'_>,
            Sender<ScheduledTask>,
        ) -> Pin<Box<dyn Send + Future<Output = RpcMessage> + '_>>
        + Send,
>;

pub type ServerWorkExec = Box<
    dyn FnOnce(
            ServerStateHandles<'_>,
            Sender<ScheduledTask>,
        ) -> Pin<Box<dyn Send + Future<Output = ()> + '_>>
        + Send,
>;

pub type CreateLocksFn = Box<
    dyn FnOnce(Arc<Mutex<ServerState>>) -> Pin<Box<dyn Send + Future<Output = ServerStateLocks>>>
        + Send,
>;

pub struct Request {
    pub exec: RequestExec,
    pub create_locks: CreateLocks,
}

pub struct Notification {
    pub exec: NotificationExec,
    pub create_locks: CreateLocks,
}

pub struct ServerNotification {
    pub exec: ServerNotificationExec,
    pub create_locks: CreateLocksFn,
}

pub struct ServerRequest {
    pub exec: ServerRequestExec,
    pub create_locks: CreateLocksFn,
}

pub struct ServerWork {
    pub exec: ServerWorkExec,
    pub create_locks: CreateLocksFn,
}

pub enum ServerInitiated {
    Notification(ServerNotification),
    Request(ServerRequest),
    Work(ServerWork),
}

pub enum ScheduledTask {
    Client(RpcMessage),
    Server(ServerInitiated),
}
