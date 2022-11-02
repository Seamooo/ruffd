use ruffd_types::lsp_types;
use ruffd_types::ruff::checks::Check;
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
