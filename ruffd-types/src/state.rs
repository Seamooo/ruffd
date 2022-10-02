use crate::collections::{AggAvlTree, Rope};
use crate::error::{DocumentError, RuntimeError};
use ruff::settings::Settings;
use std::collections::HashMap;
use std::ops::RangeBounds;
use std::sync::Arc;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub struct DocumentBuffer {
    row_tree: AggAvlTree<usize>,
    text: Rope<char>,
}

fn row_tree_accumulate(a: &usize, b: &usize) -> usize {
    *a + *b
}

impl Default for DocumentBuffer {
    fn default() -> Self {
        Self {
            row_tree: AggAvlTree::new(row_tree_accumulate),
            text: Rope::default(),
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
        let char_vec = text.chars().collect::<Vec<_>>();
        let row_counts = get_line_lengths(&char_vec);
        let text = Rope::from_document(char_vec);
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
        if self.row_tree.is_empty() {
            if row != 0 || col != 0 {
                return Err(DocumentError::IndexOutOfBounds);
            }
            let row_counts = get_line_lengths(&char_vec);
            row_counts
                .into_iter()
                .for_each(|val| self.row_tree.insert_back(val));
            self.text.insert(char_vec, 0).unwrap();
            return Ok(());
        }
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
        self.text.insert(text.chars().collect::<Vec<_>>(), idx)?;
        Ok(())
    }

    pub fn delete_range(
        &mut self,
        start_row_col: (usize, usize),
        end_row_col: (usize, usize),
    ) -> Result<(), DocumentError> {
        let (start_row, start_col) = start_row_col;
        let (end_row, end_col) = end_row_col;
        if self.row_tree.is_empty() {
            if start_row + start_col + end_row + end_col == 0 {
                return Ok(());
            }
            return Err(DocumentError::IndexOutOfBounds);
        }
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
        self.text.delete(start_idx..end_idx);
        let suffix_len = end_row_size - end_col;
        for _ in (start_row + 1)..=(end_row) {
            self.row_tree.delete(start_row + 1)?;
        }
        self.row_tree.update(start_row, start_col + suffix_len)?;
        Ok(())
    }

    pub fn iter_range<R: RangeBounds<usize>>(&self, bounds: R) -> impl Iterator<Item = &char> {
        self.text.iter_range(bounds)
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
                    will_save: Some(true),
                    will_save_wait_until: None,
                    save: None,
                },
            )),
            ..Default::default()
        };
        let project_root_path = match &project_root_val {
            Some(val) => Some(
                val.to_file_path()
                    .map_err(|_| RuntimeError::UriToPathError(val.clone()))?,
            ),
            None => None,
        };
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

    #[test]
    fn test_delete_empty() {
        let mut doc = DocumentBuffer::new();
        doc.delete_range((0, 0), (0, 0)).unwrap();
    }

    #[test]
    fn test_insert_empty() {
        let mut doc = DocumentBuffer::new();
        let text = "Some text";
        doc.insert_text(text, (0, 0)).unwrap();
        assert_eq!(doc.iter().collect::<String>(), text);
    }

    #[test]
    fn test_delete_all_and_insert() {
        let mut doc = DocumentBuffer::new();
        let text = "Some text";
        doc.insert_text(text, (0, 0)).unwrap();
        doc.delete_range((0, 0), (0, text.len())).unwrap();
        doc.insert_text(text, (0, 0)).unwrap();
        assert_eq!(doc.iter().collect::<String>(), text);
    }

    #[test]
    fn test_edit_example() {
        let mut doc = DocumentBuffer::from_string(SMALL_PROGRAM.to_string());
        doc.delete_range((0, 0), (0, 0)).unwrap();
        doc.insert_text("\n", (0, 0)).unwrap();
        doc.delete_range((0, 0), (0, 0)).unwrap();
        doc.insert_text("f", (0, 0)).unwrap();
        doc.delete_range((0, 1), (0, 1)).unwrap();
        doc.insert_text("r", (0, 1)).unwrap();
        doc.delete_range((0, 2), (0, 2)).unwrap();
        doc.insert_text("o", (0, 2)).unwrap();
        doc.delete_range((0, 3), (0, 3)).unwrap();
        doc.insert_text("m", (0, 3)).unwrap();
        doc.delete_range((0, 4), (0, 4)).unwrap();
        doc.insert_text(" ", (0, 4)).unwrap();
        doc.delete_range((0, 5), (0, 5)).unwrap();
        doc.insert_text("t", (0, 5)).unwrap();
        doc.delete_range((0, 6), (0, 6)).unwrap();
        doc.insert_text("y", (0, 6)).unwrap();
        doc.delete_range((0, 7), (0, 7)).unwrap();
        doc.insert_text("p", (0, 7)).unwrap();
        doc.delete_range((0, 8), (0, 8)).unwrap();
        doc.insert_text("i", (0, 8)).unwrap();
        doc.delete_range((0, 9), (0, 9)).unwrap();
        doc.insert_text("n", (0, 9)).unwrap();
        doc.delete_range((0, 10), (0, 10)).unwrap();
        doc.insert_text("g", (0, 10)).unwrap();
        doc.delete_range((0, 11), (0, 11)).unwrap();
        doc.insert_text(" ", (0, 11)).unwrap();
        doc.delete_range((0, 12), (0, 12)).unwrap();
        doc.insert_text("i", (0, 12)).unwrap();
        doc.delete_range((0, 13), (0, 13)).unwrap();
        doc.insert_text("m", (0, 13)).unwrap();
        doc.delete_range((0, 14), (0, 14)).unwrap();
        doc.insert_text("p", (0, 14)).unwrap();
        doc.delete_range((0, 15), (0, 15)).unwrap();
        doc.insert_text("o", (0, 15)).unwrap();
        doc.delete_range((0, 16), (0, 16)).unwrap();
        doc.insert_text("r", (0, 16)).unwrap();
        doc.delete_range((0, 17), (0, 17)).unwrap();
        doc.insert_text("t", (0, 17)).unwrap();
        doc.delete_range((0, 18), (0, 18)).unwrap();
        doc.insert_text(" ", (0, 18)).unwrap();
        doc.delete_range((0, 19), (0, 19)).unwrap();
        doc.insert_text("M", (0, 19)).unwrap();
        doc.delete_range((0, 19), (0, 20)).unwrap();
        doc.insert_text("D", (0, 19)).unwrap();
        doc.delete_range((0, 20), (0, 20)).unwrap();
        doc.insert_text("i", (0, 20)).unwrap();
        doc.delete_range((0, 21), (0, 21)).unwrap();
        doc.insert_text("c", (0, 21)).unwrap();
        doc.delete_range((0, 22), (0, 22)).unwrap();
        doc.insert_text("t", (0, 22)).unwrap();
        doc.delete_range((1, 0), (1, 0)).unwrap();
        doc.insert_text("\n", (1, 0)).unwrap();
        doc.delete_range((2, 0), (2, 0)).unwrap();
        doc.insert_text("\n", (2, 0)).unwrap();
        doc.delete_range((2, 0), (2, 0)).unwrap();
        doc.insert_text("d", (2, 0)).unwrap();
        doc.delete_range((2, 1), (2, 1)).unwrap();
        doc.insert_text("ef ", (2, 1)).unwrap();
        doc.delete_range((2, 4), (2, 4)).unwrap();
        doc.insert_text("f", (2, 4)).unwrap();
        doc.delete_range((2, 5), (2, 5)).unwrap();
        doc.insert_text("i", (2, 5)).unwrap();
        doc.delete_range((2, 6), (2, 6)).unwrap();
        doc.insert_text("b", (2, 6)).unwrap();
        doc.delete_range((2, 7), (2, 7)).unwrap();
        doc.insert_text("[]", (2, 7)).unwrap();
        doc.delete_range((2, 8), (2, 8)).unwrap();
        doc.insert_text("n", (2, 8)).unwrap();
        doc.delete_range((2, 9), (2, 9)).unwrap();
        doc.insert_text("u", (2, 9)).unwrap();
        doc.delete_range((2, 10), (2, 10)).unwrap();
        doc.insert_text("m", (2, 10)).unwrap();
        doc.delete_range((2, 8), (2, 11)).unwrap();
        doc.insert_text("", (2, 8)).unwrap();
        doc.delete_range((2, 8), (2, 8)).unwrap();
        doc.insert_text("d", (2, 8)).unwrap();
        doc.delete_range((2, 9), (2, 9)).unwrap();
        doc.insert_text("p", (2, 9)).unwrap();
        doc.delete_range((2, 10), (2, 10)).unwrap();
        doc.insert_text(": ", (2, 10)).unwrap();
        doc.delete_range((2, 12), (2, 12)).unwrap();
        doc.insert_text("D", (2, 12)).unwrap();
        doc.delete_range((2, 13), (2, 13)).unwrap();
        doc.insert_text("ict[]", (2, 13)).unwrap();
        doc.delete_range((2, 17), (2, 17)).unwrap();
        doc.insert_text("i", (2, 17)).unwrap();
        doc.delete_range((2, 18), (2, 18)).unwrap();
        doc.insert_text("n", (2, 18)).unwrap();
        doc.delete_range((2, 19), (2, 19)).unwrap();
        doc.insert_text("t", (2, 19)).unwrap();
        doc.delete_range((2, 20), (2, 20)).unwrap();
        doc.insert_text(",", (2, 20)).unwrap();
        doc.delete_range((2, 21), (2, 21)).unwrap();
        doc.insert_text("i", (2, 21)).unwrap();
        doc.delete_range((2, 22), (2, 23)).unwrap();
        doc.insert_text("nt]", (2, 22)).unwrap();
        doc.delete_range((2, 25), (2, 25)).unwrap();
        doc.insert_text(", ", (2, 25)).unwrap();
        doc.delete_range((2, 27), (2, 27)).unwrap();
        doc.insert_text("n", (2, 27)).unwrap();
        doc.delete_range((2, 28), (2, 28)).unwrap();
        doc.insert_text("u", (2, 28)).unwrap();
        doc.delete_range((2, 29), (2, 29)).unwrap();
        doc.insert_text("m", (2, 29)).unwrap();
        doc.delete_range((2, 30), (2, 30)).unwrap();
        doc.insert_text(": i", (2, 30)).unwrap();
        doc.delete_range((2, 33), (2, 34)).unwrap();
        doc.insert_text("nt]", (2, 33)).unwrap();
        doc.delete_range((2, 36), (2, 36)).unwrap();
        doc.insert_text(" -", (2, 36)).unwrap();
        doc.delete_range((2, 38), (2, 38)).unwrap();
        doc.insert_text("> ", (2, 38)).unwrap();
        doc.delete_range((2, 40), (2, 40)).unwrap();
        doc.insert_text("i", (2, 40)).unwrap();
        doc.delete_range((2, 41), (2, 41)).unwrap();
        doc.insert_text("nt:", (2, 41)).unwrap();
        doc.delete_range((3, 0), (3, 0)).unwrap();
        doc.insert_text("    \n", (3, 0)).unwrap();
        doc.delete_range((3, 4), (3, 4)).unwrap();
        doc.insert_text("i", (3, 4)).unwrap();
        doc.delete_range((3, 5), (3, 5)).unwrap();
        doc.insert_text("f ", (3, 5)).unwrap();
        doc.delete_range((3, 7), (3, 7)).unwrap();
        doc.insert_text("n", (3, 7)).unwrap();
        doc.delete_range((3, 8), (3, 8)).unwrap();
        doc.insert_text("um ", (3, 8)).unwrap();
        doc.delete_range((3, 11), (3, 11)).unwrap();
        doc.insert_text("== 0", (3, 11)).unwrap();
        doc.delete_range((3, 0), (4, 0)).unwrap();
        doc.insert_text("    if num == 0:\n        \n", (3, 0))
            .unwrap();
        doc.delete_range((4, 8), (4, 8)).unwrap();
        doc.insert_text("r", (4, 8)).unwrap();
        doc.delete_range((4, 9), (4, 9)).unwrap();
        doc.insert_text("e", (4, 9)).unwrap();
        doc.delete_range((4, 10), (4, 10)).unwrap();
        doc.insert_text("t", (4, 10)).unwrap();
        doc.delete_range((4, 11), (4, 11)).unwrap();
        doc.insert_text("u", (4, 11)).unwrap();
        doc.delete_range((4, 12), (4, 12)).unwrap();
        doc.insert_text("r", (4, 12)).unwrap();
        doc.delete_range((4, 13), (4, 13)).unwrap();
        doc.insert_text("n", (4, 13)).unwrap();
        doc.delete_range((4, 14), (4, 14)).unwrap();
        doc.insert_text(" ", (4, 14)).unwrap();
        doc.delete_range((4, 15), (4, 15)).unwrap();
        doc.insert_text("1", (4, 15)).unwrap();
        doc.delete_range((5, 0), (5, 0)).unwrap();
        doc.insert_text("    \n", (5, 0)).unwrap();
        doc.delete_range((5, 4), (5, 4)).unwrap();
        doc.insert_text("i", (5, 4)).unwrap();
        doc.delete_range((5, 5), (5, 5)).unwrap();
        doc.insert_text("f ", (5, 5)).unwrap();
        doc.delete_range((5, 7), (5, 7)).unwrap();
        doc.insert_text("n", (5, 7)).unwrap();
        doc.delete_range((5, 8), (5, 8)).unwrap();
        doc.insert_text("um ", (5, 8)).unwrap();
        doc.delete_range((5, 11), (5, 11)).unwrap();
        doc.insert_text("== ", (5, 11)).unwrap();
        doc.delete_range((5, 14), (5, 14)).unwrap();
        doc.insert_text("1", (5, 14)).unwrap();
        doc.delete_range((5, 0), (6, 0)).unwrap();
        doc.insert_text("    if num == 1:\n        \n", (5, 0))
            .unwrap();
        doc.delete_range((6, 8), (6, 8)).unwrap();
        doc.insert_text("r", (6, 8)).unwrap();
        doc.delete_range((6, 9), (6, 9)).unwrap();
        doc.insert_text("eturn ", (6, 9)).unwrap();
        doc.delete_range((6, 15), (6, 15)).unwrap();
        doc.insert_text("0", (6, 15)).unwrap();
        doc.delete_range((6, 16), (6, 16)).unwrap();
        doc.insert_text(";", (6, 16)).unwrap();
        doc.delete_range((6, 16), (6, 17)).unwrap();
        doc.insert_text("", (6, 16)).unwrap();
        doc.delete_range((7, 0), (7, 0)).unwrap();
        doc.insert_text("    \n", (7, 0)).unwrap();
        doc.delete_range((7, 4), (7, 4)).unwrap();
        doc.insert_text("i", (7, 4)).unwrap();
        doc.delete_range((7, 5), (7, 5)).unwrap();
        doc.insert_text("f ", (7, 5)).unwrap();
        doc.delete_range((7, 7), (7, 7)).unwrap();
        doc.insert_text("n", (7, 7)).unwrap();
        doc.delete_range((7, 8), (7, 8)).unwrap();
        doc.insert_text("um ", (7, 8)).unwrap();
        doc.delete_range((7, 11), (7, 11)).unwrap();
        doc.insert_text("=", (7, 11)).unwrap();
        doc.delete_range((7, 12), (7, 12)).unwrap();
        doc.insert_text("=", (7, 12)).unwrap();
        doc.delete_range((7, 11), (7, 13)).unwrap();
        doc.insert_text("", (7, 11)).unwrap();
        doc.delete_range((7, 11), (7, 11)).unwrap();
        doc.insert_text("i", (7, 11)).unwrap();
        doc.delete_range((7, 12), (7, 12)).unwrap();
        doc.insert_text("n ", (7, 12)).unwrap();
        doc.delete_range((7, 14), (7, 14)).unwrap();
        doc.insert_text("d", (7, 14)).unwrap();
        doc.delete_range((7, 15), (7, 15)).unwrap();
        doc.insert_text("p:", (7, 15)).unwrap();
        doc.delete_range((8, 0), (8, 0)).unwrap();
        doc.insert_text("        \n", (8, 0)).unwrap();
        doc.delete_range((8, 8), (8, 8)).unwrap();
        doc.insert_text("d", (8, 8)).unwrap();
        doc.delete_range((8, 8), (8, 9)).unwrap();
        doc.insert_text("r", (8, 8)).unwrap();
        doc.delete_range((8, 9), (8, 9)).unwrap();
        doc.insert_text("eturn ", (8, 9)).unwrap();
        doc.delete_range((8, 15), (8, 15)).unwrap();
        doc.insert_text("d", (8, 15)).unwrap();
        doc.delete_range((8, 16), (8, 16)).unwrap();
        doc.insert_text("p[]", (8, 16)).unwrap();
        doc.delete_range((8, 18), (8, 18)).unwrap();
        doc.insert_text("n", (8, 18)).unwrap();
        doc.delete_range((8, 19), (8, 20)).unwrap();
        doc.insert_text("um]", (8, 19)).unwrap();
        doc.delete_range((9, 0), (9, 0)).unwrap();
        doc.insert_text("    \n", (9, 0)).unwrap();
        doc.delete_range((9, 0), (9, 4)).unwrap();
        doc.insert_text("", (9, 0)).unwrap();
        doc.delete_range((9, 0), (9, 0)).unwrap();
        doc.insert_text("    ", (9, 0)).unwrap();
        doc.delete_range((9, 4), (9, 4)).unwrap();
        doc.insert_text("d", (9, 4)).unwrap();
        doc.delete_range((9, 5), (9, 5)).unwrap();
        doc.insert_text("p[]", (9, 5)).unwrap();
        doc.delete_range((9, 7), (9, 7)).unwrap();
        doc.insert_text("n", (9, 7)).unwrap();
        doc.delete_range((9, 8), (9, 8)).unwrap();
        doc.insert_text("um'", (9, 8)).unwrap();
        doc.delete_range((9, 10), (9, 11)).unwrap();
        doc.insert_text("", (9, 10)).unwrap();
        doc.delete_range((9, 11), (9, 11)).unwrap();
        doc.insert_text(" = ", (9, 11)).unwrap();
        doc.delete_range((9, 14), (9, 14)).unwrap();
        doc.insert_text("f", (9, 14)).unwrap();
        doc.delete_range((9, 15), (9, 15)).unwrap();
        doc.insert_text("ib[]", (9, 15)).unwrap();
        doc.delete_range((9, 18), (9, 18)).unwrap();
        doc.insert_text("d", (9, 18)).unwrap();
        doc.delete_range((9, 19), (9, 19)).unwrap();
        doc.insert_text("p,", (9, 19)).unwrap();
        doc.delete_range((9, 21), (9, 21)).unwrap();
        doc.insert_text(" ", (9, 21)).unwrap();
        doc.delete_range((9, 22), (9, 22)).unwrap();
        doc.insert_text("n", (9, 22)).unwrap();
        doc.delete_range((9, 23), (9, 23)).unwrap();
        doc.insert_text("um ", (9, 23)).unwrap();
        doc.delete_range((9, 26), (9, 26)).unwrap();
        doc.insert_text("- ", (9, 26)).unwrap();
        doc.delete_range((9, 28), (9, 28)).unwrap();
        doc.insert_text("2", (9, 28)).unwrap();
        doc.delete_range((9, 30), (9, 30)).unwrap();
        doc.insert_text(" =", (9, 30)).unwrap();
        doc.delete_range((9, 31), (9, 32)).unwrap();
        doc.insert_text("+", (9, 31)).unwrap();
        doc.delete_range((9, 32), (9, 32)).unwrap();
        doc.insert_text(" ", (9, 32)).unwrap();
        doc.delete_range((9, 33), (9, 33)).unwrap();
        doc.insert_text("d", (9, 33)).unwrap();
        doc.delete_range((9, 33), (9, 34)).unwrap();
        doc.insert_text("", (9, 33)).unwrap();
        doc.delete_range((9, 33), (9, 33)).unwrap();
        doc.insert_text("f", (9, 33)).unwrap();
        doc.delete_range((9, 34), (9, 34)).unwrap();
        doc.insert_text("ib[]", (9, 34)).unwrap();
        doc.delete_range((9, 37), (9, 37)).unwrap();
        doc.insert_text("d", (9, 37)).unwrap();
        doc.delete_range((9, 38), (9, 38)).unwrap();
        doc.insert_text("p,", (9, 38)).unwrap();
        doc.delete_range((9, 40), (9, 40)).unwrap();
        doc.insert_text(" ", (9, 40)).unwrap();
        doc.delete_range((9, 41), (9, 41)).unwrap();
        doc.insert_text("n", (9, 41)).unwrap();
        doc.delete_range((9, 42), (9, 42)).unwrap();
        doc.insert_text("um ", (9, 42)).unwrap();
        doc.delete_range((9, 45), (9, 45)).unwrap();
        doc.insert_text("-", (9, 45)).unwrap();
        doc.delete_range((9, 46), (9, 46)).unwrap();
        doc.insert_text(" ", (9, 46)).unwrap();
        doc.delete_range((9, 47), (9, 47)).unwrap();
        doc.insert_text("1", (9, 47)).unwrap();
        doc.delete_range((10, 0), (10, 0)).unwrap();
        doc.insert_text("    \n", (10, 0)).unwrap();
        doc.delete_range((10, 4), (10, 4)).unwrap();
        doc.insert_text("r", (10, 4)).unwrap();
        doc.delete_range((10, 5), (10, 5)).unwrap();
        doc.insert_text("etr", (10, 5)).unwrap();
        doc.delete_range((10, 7), (10, 8)).unwrap();
        doc.insert_text("", (10, 7)).unwrap();
        doc.delete_range((10, 7), (10, 7)).unwrap();
        doc.insert_text("u", (10, 7)).unwrap();
        doc.delete_range((10, 8), (10, 8)).unwrap();
        doc.insert_text("rn ", (10, 8)).unwrap();
        doc.delete_range((10, 11), (10, 11)).unwrap();
        doc.insert_text("d", (10, 11)).unwrap();
        doc.delete_range((10, 12), (10, 12)).unwrap();
        doc.insert_text("p[]", (10, 12)).unwrap();
        doc.delete_range((10, 14), (10, 14)).unwrap();
        doc.insert_text("n", (10, 14)).unwrap();
        doc.delete_range((10, 15), (10, 16)).unwrap();
        doc.insert_text("um]", (10, 15)).unwrap();
        doc.delete_range((7, 10), (7, 10)).unwrap();
        doc.insert_text(" n", (7, 10)).unwrap();
        doc.delete_range((7, 12), (7, 12)).unwrap();
        doc.insert_text("o", (7, 12)).unwrap();
        doc.delete_range((7, 13), (7, 13)).unwrap();
        doc.insert_text("t", (7, 13)).unwrap();
        doc.delete_range((8, 0), (9, 0)).unwrap();
        doc.insert_text("", (8, 0)).unwrap();
        doc.delete_range((8, 4), (8, 4)).unwrap();
        doc.insert_text("    ", (8, 4)).unwrap();
        doc.delete_range((12, 4), (12, 33)).unwrap();
        doc.insert_text("", (12, 4)).unwrap();
        doc.delete_range((12, 4), (12, 4)).unwrap();
        doc.insert_text("p", (12, 4)).unwrap();
        doc.delete_range((12, 5), (12, 5)).unwrap();
        doc.insert_text("r", (12, 5)).unwrap();
        doc.delete_range((12, 6), (12, 6)).unwrap();
        doc.insert_text("i", (12, 6)).unwrap();
        doc.delete_range((12, 7), (12, 7)).unwrap();
        doc.insert_text("n", (12, 7)).unwrap();
        doc.delete_range((12, 8), (12, 8)).unwrap();
        doc.insert_text("t", (12, 8)).unwrap();
        doc.delete_range((12, 9), (12, 9)).unwrap();
        doc.insert_text("[]", (12, 9)).unwrap();
        doc.delete_range((12, 10), (12, 10)).unwrap();
        doc.insert_text("f", (12, 10)).unwrap();
        doc.delete_range((12, 11), (12, 11)).unwrap();
        doc.insert_text("ib[]", (12, 11)).unwrap();
        doc.delete_range((12, 14), (12, 14)).unwrap();
        doc.insert_text("1", (12, 14)).unwrap();
        doc.delete_range((12, 15), (12, 15)).unwrap();
        doc.insert_text("0", (12, 15)).unwrap();
        doc.delete_range((12, 16), (12, 16)).unwrap();
        doc.insert_text("0", (12, 16)).unwrap();
        doc.delete_range((12, 17), (12, 17)).unwrap();
        doc.insert_text("0", (12, 17)).unwrap();
        let expected = r#"from typing import Dict

def fib[dp: Dict[int,int], num: int] -> int:
    if num == 0:
        return 1
    if num == 1:
        return 0
    if num not in dp:
        dp[num] = fib[dp, num - 2] + fib[dp, num - 1]
    return dp[num]

def main():
    print[fib[1000]]

if __name__ == '__main__':
    main()
"#;
        assert_eq!(doc.iter().collect::<String>(), expected);
    }
}
