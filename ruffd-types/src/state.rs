use crate::collections::AggAvlTree;
use crate::error::{DocumentError, RuntimeError};
use ruff::settings::Settings;
use std::collections::HashMap;
use std::ops::{Bound, RangeBounds};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub struct DocumentBuffer {
    row_tree: AggAvlTree<usize>,
    // TODO use a rope instead of a vec
    text: Vec<char>,
}

fn row_tree_accumulate(a: &usize, b: &usize) -> usize {
    *a + *b
}

impl Default for DocumentBuffer {
    fn default() -> Self {
        Self {
            row_tree: AggAvlTree::new(row_tree_accumulate),
            text: vec![],
        }
    }
}

fn get_line_lengths(chars: &[char]) -> Vec<usize> {
    let mut rv = vec![];
    let mut curr = 0usize;
    let mut prev_carriage_return = false;
    // below handles ['\n', '\r\n', '\r'] line endings
    chars.iter().for_each(|x| {
        if *x == '\n' {
            curr += 1;
            rv.push(curr);
            curr = 0;
        } else if prev_carriage_return {
            rv.push(curr);
            curr = 1;
        } else {
            curr += 1;
        }
        prev_carriage_return = *x == '\r';
    });
    if prev_carriage_return {
        rv.push(curr);
        curr = 0;
    }
    rv.push(curr);
    rv
}

impl DocumentBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_string(text: String) -> Self {
        let text = text.chars().collect::<Vec<_>>();
        let row_counts = get_line_lengths(&text);
        let row_tree = AggAvlTree::from_vec(row_counts, row_tree_accumulate);
        Self { text, row_tree }
    }

    pub fn insert_text(
        &mut self,
        text: &str,
        row_col: (usize, usize),
    ) -> Result<(), DocumentError> {
        let (row, col) = row_col;
        let char_vec: Vec<char> = text.chars().collect();
        let curr_row_size = self
            .row_tree
            .get(row)
            .ok_or(DocumentError::RowOutOfBounds)?;
        if curr_row_size < col {
            return Err(DocumentError::ColOutOfBounds);
        }
        let suffix_size = curr_row_size - col;
        let row_counts = get_line_lengths(&char_vec);
        // 3 cases: no line breaks, 1 line break, 2 or more line breaks
        let mut row_counts_iter = row_counts.into_iter();
        let first_count = row_counts_iter.next().unwrap();
        self.row_tree.update(row, col + first_count)?;
        if let Some(last_count) = row_counts_iter.next_back() {
            self.row_tree.insert(row + 1, suffix_size + last_count);
        } else {
            // add suffix length to line if there exists no suffix
            self.row_tree.update(row, col + first_count + suffix_size)?;
        }
        while let Some(count) = row_counts_iter.next_back() {
            self.row_tree.insert(row + 1, count);
        }
        // empty row range gives 0
        let idx = self.row_tree.get_range(..row).unwrap_or(0) + col;
        let mut stuff = self.text.drain(idx..).collect::<Vec<_>>();
        self.text.append(&mut text.chars().collect::<Vec<_>>());
        self.text.append(&mut stuff);
        Ok(())
    }

    pub fn delete_range(
        &mut self,
        start_row_col: (usize, usize),
        end_row_col: (usize, usize),
    ) -> Result<(), DocumentError> {
        let (start_row, start_col) = start_row_col;
        let (end_row, end_col) = end_row_col;
        let start_row_size = self
            .row_tree
            .get(start_row)
            .ok_or(DocumentError::RowOutOfBounds)?;
        // TODO generalise column bounds check to account for line endings
        if start_col >= start_row_size {
            return Err(DocumentError::ColOutOfBounds);
        }
        let start_idx = self.row_tree.get_range(..start_row).unwrap_or(0) + start_col;
        let end_row_size = self
            .row_tree
            .get(end_row)
            .ok_or(DocumentError::RowOutOfBounds)?;
        // TODO generalise column bounds check to account for line endings
        if end_col >= end_row_size {
            return Err(DocumentError::ColOutOfBounds);
        }
        let end_idx = self.row_tree.get_range(..end_row).unwrap_or(0) + end_col;
        let mut suffix = self.text.drain(end_idx..).collect::<Vec<_>>();
        self.text.drain(start_idx..).for_each(drop);
        self.text.append(&mut suffix);
        let suffix_len = end_row_size - end_col;
        for _ in (start_row + 1)..=(end_row) {
            self.row_tree.delete(start_row + 1)?;
        }
        self.row_tree.update(start_row, start_col + suffix_len)?;
        Ok(())
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
    pub settings: Arc<RwLock<Settings>>,
}

impl ServerState {
    pub fn from_init(init_params: &lsp_types::InitializeParams) -> Result<Self, RuntimeError> {
        // FIXME configure from client capabilities
        let project_root_val = init_params.root_uri.clone();
        // TODO
        // - hover provider
        // - code action provider
        // - diagnostic provider
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
        let project_root_path = project_root_val
            .clone()
            .map(|url| PathBuf::from(url.to_string()));
        let project_root = Arc::new(RwLock::new(project_root_val));
        let capabilities = Arc::new(RwLock::new(capabilities_val));
        let open_buffers = Arc::new(RwLock::new(HashMap::new()));
        let settings = Arc::new(RwLock::new(Settings::from_pyproject(
            None,
            project_root_path,
        )?));
        Ok(Self {
            settings,
            project_root,
            capabilities,
            open_buffers,
        })
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
    pub settings: RwReqOpt<Settings>,
}

pub struct ServerStateHandles<'a> {
    pub project_root: OptRwGuarded<'a, Option<lsp_types::Url>>,
    pub open_buffers: OptRwGuarded<'a, HashMap<lsp_types::Url, DocumentBuffer>>,
    pub capabilities: OptRwGuarded<'a, lsp_types::ServerCapabilities>,
    pub settings: OptRwGuarded<'a, Settings>,
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
    let settings = match &locks.settings {
        Some(x) => Some(x.lock().await),
        None => None,
    };
    ServerStateHandles {
        project_root,
        open_buffers,
        capabilities,
        settings,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SMALL_PROGRAM: &str = r#"
def main():
    print('I am a small program')

if __name__ == '__main__':
    main()
"#;

    #[test]
    fn test_document_buffer_create() {
        DocumentBuffer::new();
        let doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        assert_eq!(doc.iter().collect::<String>(), SMALL_PROGRAM);
    }

    #[test]
    fn test_document_buffer_insert_front() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.insert_text("some text", (0, 0)).unwrap();
        let expected = {
            let mut rv = "some text".to_owned();
            rv.push_str(SMALL_PROGRAM);
            rv
        };
        assert_eq!(doc.iter().collect::<String>(), expected);
    }

    #[test]
    fn test_document_buffer_insert_arbitrary() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.insert_text("some text", (1, 5)).unwrap();
        let expected = r#"
def msome textain():
    print('I am a small program')

if __name__ == '__main__':
    main()
"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }

    #[test]
    fn test_document_buffer_insert_last_row() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.insert_text("some text", (6, 0)).unwrap();
        let expected = r#"
def main():
    print('I am a small program')

if __name__ == '__main__':
    main()
some text"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }

    #[test]
    fn test_consecutive_inserts() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.insert_text("some text ", (1, 5)).unwrap();
        doc.insert_text("some more text ", (1, 15)).unwrap();
        doc.insert_text("some text ", (4, 0)).unwrap();
        doc.insert_text("some different text", (4, 10)).unwrap();
        let expected = r#"
def msome text some more text ain():
    print('I am a small program')

some text some different textif __name__ == '__main__':
    main()
"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }

    #[test]
    fn test_new_line() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.insert_text("    x = 0\n", (2, 0)).unwrap();
        let expected = r#"
def main():
    x = 0
    print('I am a small program')

if __name__ == '__main__':
    main()
"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }

    #[test]
    fn test_delete() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.delete_range((1, 0), (2, 4)).unwrap();
        let expected = r#"
print('I am a small program')

if __name__ == '__main__':
    main()
"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }
}
