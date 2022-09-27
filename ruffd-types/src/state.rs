use std::collections::HashMap;
use std::ops::{Bound, RangeBounds};
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Default)]
pub struct DocumentBuffer {
    text: Vec<char>,
}

impl DocumentBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_string(text: String) -> Self {
        let text = text.chars().collect::<Vec<_>>();
        Self { text }
    }

    pub fn insert_text(&mut self, idx: usize, text: String) {
        let mut stuff = self.text.drain(idx..).collect::<Vec<_>>();
        self.text.append(&mut text.chars().collect::<Vec<_>>());
        self.text.append(&mut stuff);
    }

    pub fn iter_range<R: RangeBounds<usize>>(&self, bounds: R) -> impl Iterator<Item = &char> {
        let begin_idx = match bounds.start_bound() {
            Bound::Excluded(x) => 1 + x,
            Bound::Included(x) => *x,
            Bound::Unbounded => 0,
        };
        let end_idx = match bounds.end_bound() {
            Bound::Excluded(x) => *x,
            Bound::Included(x) => 1 + x,
            Bound::Unbounded => self.text.len(),
        };
        self.text[begin_idx..end_idx].iter()
    }

    pub fn iter(&self) -> impl Iterator<Item = &char> {
        self.text.iter()
    }
}

pub struct ServerState {
    pub project_root: Arc<RwLock<Option<lsp_types::Url>>>,
    pub open_buffers: Arc<RwLock<HashMap<lsp_types::Url, DocumentBuffer>>>,
    pub capabilities: Arc<RwLock<lsp_types::ServerCapabilities>>,
}

impl ServerState {
    pub fn from_init(init_params: &lsp_types::InitializeParams) -> Self {
        // FIXME configure from client capabilities
        let project_root_val = init_params.root_uri.clone();
        let capabilities_val = lsp_types::ServerCapabilities {
            text_document_sync: Some(lsp_types::TextDocumentSyncCapability::Options(
                lsp_types::TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(lsp_types::TextDocumentSyncKind::INCREMENTAL),
                    will_save: None,
                    will_save_wait_until: None,
                    save: None,
                },
            )),
            ..Default::default()
        };
        let project_root = Arc::new(RwLock::new(project_root_val));
        let capabilities = Arc::new(RwLock::new(capabilities_val));
        let open_buffers = Arc::new(RwLock::new(HashMap::new()));
        Self {
            project_root,
            capabilities,
            open_buffers,
        }
    }
}

pub enum RwGuarded<'a, T> {
    Read(RwLockReadGuard<'a, T>),
    Write(RwLockWriteGuard<'a, T>),
}

pub enum RwReq<T> {
    Read(Arc<RwLock<T>>),
    Write(Arc<RwLock<T>>),
}

impl<T> RwReq<T> {
    pub async fn lock(&self) -> RwGuarded<'_, T> {
        match self {
            Self::Read(x) => RwGuarded::Read(x.read().await),
            Self::Write(x) => RwGuarded::Write(x.write().await),
        }
    }
}

type RwReqOpt<T> = Option<RwReq<T>>;

type OptRwGuarded<'a, T> = Option<RwGuarded<'a, T>>;

// TODO potentially create a macro for below based on ServerState

#[derive(Default)]
pub struct ServerStateLocks {
    pub project_root: RwReqOpt<Option<lsp_types::Url>>,
    pub open_buffers: RwReqOpt<HashMap<lsp_types::Url, DocumentBuffer>>,
    pub capabilities: RwReqOpt<lsp_types::ServerCapabilities>,
}

pub struct ServerStateHandles<'a> {
    pub project_root: OptRwGuarded<'a, Option<lsp_types::Url>>,
    pub open_buffers: OptRwGuarded<'a, HashMap<lsp_types::Url, DocumentBuffer>>,
    pub capabilities: OptRwGuarded<'a, lsp_types::ServerCapabilities>,
}

pub async fn server_state_handles_from_locks(locks: &ServerStateLocks) -> ServerStateHandles<'_> {
    let project_root = match &locks.project_root {
        Some(x) => Some(x.lock().await),
        None => None,
    };
    let open_buffers = match &locks.open_buffers {
        Some(x) => Some(x.lock().await),
        None => None,
    };
    let capabilities = match &locks.capabilities {
        Some(x) => Some(x.lock().await),
        None => None,
    };
    ServerStateHandles {
        project_root,
        open_buffers,
        capabilities,
    }
}
