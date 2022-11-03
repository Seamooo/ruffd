use crate::ruff_utils::action_from_check;
use ruffd_macros::request;
use ruffd_types::lsp_types;
use ruffd_types::{Request, RuntimeError};
use std::collections::HashMap;

#[request(checks)]
fn doc_code_action(
    action_params: lsp_types::CodeActionParams,
) -> Result<Option<Vec<lsp_types::CodeActionOrCommand>>, RuntimeError> {
    let uri = action_params.text_document.uri;
    if let Some(registry) = checks.get(&uri) {
        let start_line = action_params.range.start.line as usize;
        let start_col = action_params.range.start.character as usize;
        let end_line = action_params.range.end.line as usize;
        let end_col = action_params.range.end.character as usize;
        let start = (start_line, start_col);
        let end = (end_line, end_col);
        let rv = registry
            .iter_range(start..end)
            .map(|check| action_from_check(check, &uri))
            .filter(Option::is_some)
            .flatten()
            .map(lsp_types::CodeActionOrCommand::CodeAction)
            .collect::<Vec<_>>();
        Ok(Some(rv))
    } else {
        Ok(None)
    }
}

lazy_static! {
    pub(crate) static ref REQUEST_REGISTRY: HashMap<&'static str, Request> = {
        let pairs = vec![("textDocument/codeAction", doc_code_action)];
        pairs
            .into_iter()
            .collect::<HashMap<&'static str, Request>>()
    };
}
