use crate::notifications::NOTIFICATION_REGISTRY;
use crate::requests::REQUEST_REGISTRY;
use crate::{PKG_NAME, PKG_VERSION};
use regex::Regex;
use ruffd_types::tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use ruffd_types::tokio::sync::mpsc::{channel, Receiver, Sender};
use ruffd_types::tokio::sync::{Mutex, Notify, RwLock};
use ruffd_types::tokio::task;
use ruffd_types::{lsp_types, serde_json};
use ruffd_types::{
    server_state_handles_from_locks, RpcErrors, RpcMessage, RpcResponse, RpcResult, RuntimeError,
    ServerState,
};
use std::collections::HashMap;
use std::future::Future;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::Arc;

lazy_static! {
    static ref PAYLOAD_START_PATTERN: Regex =
        Regex::new(r"Content-Length:\s*(?P<size>\d+)\r\n$").unwrap();
    static ref SERVER_INFO: lsp_types::ServerInfo = lsp_types::ServerInfo {
        name: PKG_NAME.to_string(),
        version: Some(PKG_VERSION.to_string()),
    };
}

#[derive(Default)]
pub struct Server {
    state: Arc<Mutex<Option<Arc<Mutex<ServerState>>>>>,
    user_tasks: Arc<RwLock<HashMap<lsp_types::NumberOrString, task::JoinHandle<()>>>>,
    listen_task: Option<task::JoinHandle<()>>,
    sender_task: Option<task::JoinHandle<()>>,
}

impl Server {
    /// Create a new server instance
    pub fn new() -> Self {
        Self::default()
    }

    async fn init(
        &mut self,
        init_params: &lsp_types::InitializeParams,
    ) -> lsp_types::ServerCapabilities {
        let capabilities_lock = {
            let mut state_handle = self.state.lock().await;
            let new_state = ServerState::from_init(init_params);
            let rv = new_state.capabilities.clone();
            *state_handle = Some(Arc::new(Mutex::new(new_state)));
            rv
        };
        // FIXME erroneous lock here
        let capabilities = capabilities_lock.read().await;
        capabilities.clone()
    }

    /// Start server operating by reading and writing from `stdin` and `stdout`
    /// respectively
    pub async fn run_stdio(&mut self) {
        let stdout = io::stdout();
        let stdin = io::BufReader::new(io::stdin());
        self.run(stdin, stdout).await;
    }

    pub async fn run_socket<A: ToSocketAddrs>(&mut self, _addr: A) {
        unimplemented!();
    }

    pub async fn run_pipe(&mut self) {
        unimplemented!();
    }

    async fn handle_loop(
        &mut self,
        mut msg_channel: Receiver<String>,
        response_channel: Sender<RpcResponse>,
    ) {
        loop {
            let next_message = msg_channel.recv().await.unwrap();
            let rpc_message: RpcResult<RpcMessage> =
                match serde_json::from_str::<RpcMessage>(&next_message) {
                    Ok(x) => {
                        if !x.jsonrpc.eq("2.0") {
                            Err(RpcErrors::INVALID_REQUEST)
                        } else {
                            Ok(x)
                        }
                    }
                    Err(x) => Err(x.into()),
                };
            match rpc_message {
                Ok(msg) => {
                    let curr_state = self.state.lock().await.clone();
                    if curr_state.is_none() {
                        // below code path shouldn't be hit
                        let resp =
                            RpcResponse::from_error(msg.id, RpcErrors::SERVER_NOT_INITIALIZED);
                        let response_channel = response_channel.clone();
                        task::spawn(async move {
                            response_channel.send(resp).await.unwrap();
                        });
                        continue;
                    }
                    let curr_state = curr_state.unwrap();
                    let user_tasks = self.user_tasks.clone();
                    let id = msg.id;
                    let id_clone = id.clone();
                    let assurance_lock = Arc::new(Mutex::new(()));
                    let fut_lock = assurance_lock.clone();
                    let fut_cleanup = Box::pin(async move {
                        let _lock_guard = fut_lock.lock().await;
                        let mut tasks_lg = user_tasks.write().await;
                        if let Some(x) = id_clone {
                            tasks_lg.remove(&x);
                        }
                    });
                    if let Some(task_handle) = schedule_task(
                        curr_state.clone(),
                        msg.method,
                        msg.params,
                        id.clone(),
                        response_channel.clone(),
                        Some(fut_cleanup),
                    )
                    .await
                    {
                        let tasks_lock = self.user_tasks.clone();
                        let mut tasks_lg = tasks_lock.write().await;
                        if let Some(x) = id {
                            tasks_lg.insert(x, task_handle);
                        }
                    }
                }
                Err(err) => {
                    let resp = RpcResponse::from_error(None, err);
                    let response_channel = response_channel.clone();
                    task::spawn(async move {
                        response_channel.send(resp).await.unwrap();
                    });
                }
            }
        }
    }

    async fn run<R, W>(&mut self, mut reader: R, mut writer: W)
    where
        R: AsyncBufReadExt + AsyncReadExt + Unpin + Send + 'static,
        W: AsyncWriteExt + Unpin + Send + 'static,
    {
        eprintln!("starting server");
        let (init_req_id, init_params) = get_init_msg(&mut reader, &mut writer).await;
        eprintln!("received init message");
        let capabilities = self.init(&init_params).await;
        let initialize_result = lsp_types::InitializeResult {
            capabilities,
            server_info: Some(SERVER_INFO.clone()),
        };
        let result_resp = RpcResponse::from_result(init_req_id, initialize_result);
        let result_msg = serde_json::to_string(&result_resp).unwrap();
        write_msg(&mut writer, result_msg.as_bytes()).await.unwrap();
        let (msg_s, msg_r) = channel(1000);
        let (resp_s, resp_r) = channel(1000);
        let (msg_listen, resp_listen) = (msg_s.clone(), resp_s.clone());
        let listen_task = task::spawn(async move {
            eprintln!("started listener");
            listen_loop(&mut reader, msg_listen, resp_listen).await;
        });
        let sender_task = task::spawn(async move {
            eprintln!("started sender");
            sender_loop(&mut writer, resp_r).await;
        });
        self.listen_task = Some(listen_task);
        self.sender_task = Some(sender_task);
        self.handle_loop(msg_r, resp_s).await;
    }
}

async fn schedule_task(
    state: Arc<Mutex<ServerState>>,
    method: String,
    params: Option<serde_json::Value>,
    id: Option<lsp_types::NumberOrString>,
    response_channel: Sender<RpcResponse>,
    cleanup_fut: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
) -> Option<task::JoinHandle<()>> {
    if let Some(request) = REQUEST_REGISTRY.get(method.as_str()) {
        let locks = (request.create_locks)(state.clone()).await;
        let notify = Arc::new(Notify::new());
        let notify_clone = notify.clone();
        let fut = async move {
            let handles = server_state_handles_from_locks(&locks).await;
            notify_clone.notify_one();
            let resp = (request.exec)(handles, id, params).await;
            response_channel.send(resp).await.unwrap();
        };
        let task_handle = task::spawn(async move {
            fut.await;
            if let Some(x) = cleanup_fut {
                x.await;
            }
        });
        notify.notified().await;
        Some(task_handle)
    } else if let Some(notification) = NOTIFICATION_REGISTRY.get(method.as_str()) {
        let locks = (notification.create_locks)(state.clone()).await;
        let notify = Arc::new(Notify::new());
        let notify_clone = notify.clone();
        let fut = async move {
            let handles = server_state_handles_from_locks(&locks).await;
            notify_clone.notify_one();
            let resp = (notification.exec)(handles, id, params).await;
            if let Some(x) = resp {
                response_channel.send(x).await.unwrap();
            }
        };
        let task_handle = task::spawn(async move {
            fut.await;
            if let Some(x) = cleanup_fut {
                x.await;
            }
        });
        notify.notified().await;
        Some(task_handle)
    } else {
        None
    }
}

async fn listen_loop<R>(
    reader: &mut R,
    msg_channel: Sender<String>,
    response_channel: Sender<RpcResponse>,
) where
    R: AsyncBufReadExt + AsyncReadExt + Unpin,
{
    loop {
        match read_next_msg(reader).await {
            Ok(message) => msg_channel.send(message).await.unwrap(),
            Err(err) => {
                let resp = RpcResponse::from_error(None, err);
                let response_channel = response_channel.clone();
                task::spawn(async move {
                    response_channel.send(resp).await.unwrap();
                });
            }
        }
    }
}

async fn sender_loop<W>(writer: &mut W, mut response_channel: Receiver<RpcResponse>)
where
    W: AsyncWriteExt + Unpin,
{
    loop {
        let resp = response_channel.recv().await.unwrap();
        let msg_str = serde_json::to_string(&resp).unwrap();
        write_msg(writer, msg_str.as_bytes()).await.unwrap();
    }
}

fn parse_init_request(
    req_msg: &str,
) -> RpcResult<(lsp_types::NumberOrString, lsp_types::InitializeParams)> {
    // NOTE any message not matching the format required for initialization
    // is treated as a PARSE_ERROR
    let request: RpcMessage = serde_json::from_str(req_msg)?;
    let id = request.id.ok_or(RpcErrors::PARSE_ERROR)?;
    if !request.method.eq("initialize") {
        return Err(RpcErrors::PARSE_ERROR);
    }
    let param_string = request.params.ok_or(RpcErrors::PARSE_ERROR)?;
    let params: lsp_types::InitializeParams = serde_json::from_value(param_string)?;
    Ok((id, params))
}

async fn get_init_msg<R, W>(
    reader: &mut R,
    writer: &mut W,
) -> (lsp_types::NumberOrString, lsp_types::InitializeParams)
where
    R: AsyncBufReadExt + AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    loop {
        let message_result = match read_next_msg(reader).await {
            Ok(msg) => parse_init_request(msg.as_str()),
            Err(err) => Err(err),
        };
        match message_result {
            Ok(rv) => {
                break rv;
            }
            Err(err) => {
                let resp = RpcResponse::from_error(None, err);
                let resp_str = serde_json::to_string(&resp).unwrap();
                write_msg(writer, resp_str.as_bytes()).await.unwrap();
            }
        }
    }
}

async fn read_next_msg<R>(reader: &mut R) -> RpcResult<String>
where
    R: AsyncBufReadExt + AsyncReadExt + Unpin,
{
    let mut buff = String::new();
    let num_str = loop {
        buff.clear();
        reader.read_line(&mut buff).await?;
        if let Some(match_str) = PAYLOAD_START_PATTERN.captures(&buff) {
            break match_str["size"].to_string();
        }
    };
    let content_length = num_str.parse::<u32>().unwrap();
    // content-type
    buff.clear();
    reader.read_line(&mut buff).await?;
    (buff.trim().eq("utf8") || buff.trim().eq("utf-8") || buff.trim().eq(""))
        .then_some(..)
        .ok_or(RuntimeError::UnknownEncoding(buff))?;
    let mut bytes_rv = vec![0u8; content_length as usize];
    reader.read_exact(&mut bytes_rv).await?;
    Ok(String::from_utf8(bytes_rv).unwrap())
}

async fn write_msg<W>(writer: &mut W, msg: &[u8]) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    // NOTE this function cannot return an RpcResult as it is the reporter of
    // RpcErrors
    let header_str = format!("Content-Length: {}\r\n\r\n", msg.len());
    let bytes = [header_str.as_bytes(), msg].concat();
    writer.write_all(&bytes[..]).await?;
    writer.flush().await?;
    Ok(())
}
