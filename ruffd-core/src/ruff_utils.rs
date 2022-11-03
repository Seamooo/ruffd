use ruffd_types::lsp_types;
use ruffd_types::ruff::checks::Check;
use std::collections::HashMap;

pub fn diagnostic_from_check(check: &Check) -> lsp_types::Diagnostic {
    let range = {
        // diagnostic is zero indexed, but message rows are 1-indexed
        let row_start = check.location.row() as u32 - 1;
        let col_start = check.location.column() as u32;
        let row_end = check.end_location.row() as u32 - 1;
        let col_end = check.end_location.column() as u32;
        let start = lsp_types::Position {
            line: row_start,
            character: col_start,
        };
        let end = lsp_types::Position {
            line: row_end,
            character: col_end,
        };
        lsp_types::Range { start, end }
    };
    let code = Some(lsp_types::NumberOrString::String(
        check.kind.code().as_ref().to_string(),
    ));
    let source = Some(String::from("ruff"));
    let message = check.kind.body();
    lsp_types::Diagnostic {
        range,
        code,
        source,
        message,
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        code_description: None,
        tags: None,
        related_information: None,
        data: None,
    }
}

pub fn action_from_check(
    check: &Check,
    document_uri: &lsp_types::Url,
) -> Option<lsp_types::CodeAction> {
    check.fix.as_ref().map(|fix| {
        let row_start = fix.patch.location.row() as u32 - 1;
        let row_end = fix.patch.end_location.row() as u32 - 1;
        let col_start = fix.patch.location.column() as u32;
        let col_end = fix.patch.end_location.column() as u32;
        lsp_types::CodeAction {
            title: format!("fix {}", check.kind.code().as_ref()),
            kind: Some(lsp_types::CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic_from_check(check)]),
            edit: Some(lsp_types::WorkspaceEdit {
                changes: Some(HashMap::from_iter(vec![(
                    document_uri.clone(),
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
        }
    })
}
