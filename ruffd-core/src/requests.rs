use ruffd_macros::request;
use ruffd_types::lsp_types;
use ruffd_types::{Request, RuntimeError};
use std::collections::HashMap;

#[request]
fn doc_code_action(
    _action_params: lsp_types::CodeActionParams,
) -> Result<Option<lsp_types::CodeActionOrCommand>, RuntimeError> {
    Ok(None)
}

lazy_static! {
    pub(crate) static ref REQUEST_REGISTRY: HashMap<&'static str, Request> = {
        let pairs = vec![("textDocument/codeAction", doc_code_action)];
        pairs
            .into_iter()
            .collect::<HashMap<&'static str, Request>>()
    };
}
