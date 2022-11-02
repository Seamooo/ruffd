use crate::ruff_utils::diagnostic_from_check;
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
            .map(|check| {
                check.fix.as_ref().map(|fix| {
                    let row_start = fix.patch.location.row() as u32 - 1;
                    let row_end = fix.patch.end_location.row() as u32 - 1;
                    let col_start = fix.patch.location.column() as u32;
                    let col_end = fix.patch.end_location.column() as u32;
                    lsp_types::CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
                        title: format!("fix {}", check.kind.code().as_ref()),
                        kind: Some(lsp_types::CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diagnostic_from_check(check)]),
                        edit: Some(lsp_types::WorkspaceEdit {
                            changes: Some(HashMap::from_iter(vec![(
                                uri.clone(),
                                vec![lsp_types::TextEdit {
                                    range: lsp_types::Range {
                                        start: lsp_types::Position {
                                            line: row_start,
                                            character: col_start,
                                        },
                                        end: lsp_types::Position {
                                            line: row_end,
                                            character: col_end,
                                        },
                                    },
                                    new_text: fix.patch.content.clone(),
                                }],
                            )])),
                            ..Default::default()
                        }),
                        ..Default::default()
                    })
                })
            })
            .filter(Option::is_some)
            .flatten()
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
