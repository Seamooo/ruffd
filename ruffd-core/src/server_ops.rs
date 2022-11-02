use ruffd_types::ruff::check;
use ruffd_types::ruff::checks::Check;
use ruffd_types::tokio::sync::mpsc::Sender;
use ruffd_types::tokio::sync::Mutex;
use ruffd_types::{lsp_types, serde_json};
use ruffd_types::{
    CreateLocksFn, RpcNotification, RwGuarded, RwReq, ScheduledTask, ServerNotification,
    ServerNotificationExec, ServerState, ServerStateHandles, ServerStateLocks,
};
use std::path::Path;
use std::sync::Arc;

fn check_into_diagnostic(check: Check) -> lsp_types::Diagnostic {
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

fn diagnostics_from_doc(path: &Path, doc: &str) -> Vec<lsp_types::Diagnostic> {
    check(path, doc)
        .unwrap_or_default()
        .into_iter()
        .map(check_into_diagnostic)
        .collect()
}

macro_rules! tup_pat_setter {
    ($rv:ident, mut $name:ident) => {
        $rv.$name = $name;
    };
    ($rv:ident, $name:ident) => {
        $rv.$name = $name;
    };
}

macro_rules! tup_pat_create_lock {
    ($handle:ident, mut $name:ident) => {
        let $name = Some(RwReq::Write($handle.$name.clone()));
    };
    ($handle:ident, $name:ident) => {
        let $name = Some(RwReq::Read($handle.$name.clone()));
    };
}

// Clippy will yell for not using ..Default::default() if macro_rules
// gets linting in its expansion but macro syntax inside the struct
// initializer is not allowed
macro_rules! create_locks_fut {
    ($($args:tt),+) => {
        Box::new(|state: Arc<Mutex<ServerState>>| {
            Box::pin(async move {
                let handle = state.lock().await;
                $(tup_pat_create_lock!(handle, $args))* ;
                let mut rv = ServerStateLocks::default();
                $(tup_pat_setter!(rv, $args))*;
                rv
            })
        })
    };
}

macro_rules! tup_pat_unwrap_state_handles {
    ($handle:ident, mut $name:ident) => {
        let mut $name = match $handle.$name.unwrap() {
            RwGuarded::Write(x) => x,
            _ => unreachable!(),
        };
    };
    ($handle:ident, $name: ident) => {
        let $name = match $handle.$name.unwrap() {
            RwGuarded::Read(x) => x,
            _ => unreachable!(),
        };
    };
}

macro_rules! unwrap_state_handles {
    ($handles:ident, $($args:tt),+) => {
        $(tup_pat_unwrap_state_handles!($handles, $args))*
    }
}

pub fn run_diagnostic_op(document_uri: lsp_types::Url) -> ServerNotification {
    let exec: ServerNotificationExec = Box::new(
        move |state_handles: ServerStateHandles<'_>, _scheduler_channel: Sender<ScheduledTask>| {
            Box::pin(async move {
                unwrap_state_handles!(state_handles, open_buffers);
                let diagnostics = {
                    if let Some(buffer) = open_buffers.get(&document_uri) {
                        let doc = buffer.iter().collect::<String>();
                        if let Ok(path) = document_uri.to_file_path() {
                            diagnostics_from_doc(&path, doc.as_str())
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                };
                RpcNotification::new(
                    "textDocument/publishDiagnostics".to_string(),
                    Some(
                        serde_json::to_value(lsp_types::PublishDiagnosticsParams {
                            uri: document_uri,
                            diagnostics,
                            version: None,
                        })
                        .unwrap(),
                    ),
                )
                .into()
            })
        },
    );
    let create_locks: CreateLocksFn = create_locks_fut!(open_buffers);
    ServerNotification { exec, create_locks }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_diagnostic_gen_position() {
        let doc = r#"
def bar():
    x = 0
    print('does this now work?')
"#;
        let path = lsp_types::Url::parse("file:///tmp/dummy.py")
            .unwrap()
            .to_file_path()
            .unwrap();
        let diagnostics = diagnostics_from_doc(&path, doc);
        let expected_range = lsp_types::Range {
            start: lsp_types::Position {
                line: 2,
                character: 4,
            },
            end: lsp_types::Position {
                line: 2,
                character: 5,
            },
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range, expected_range);
    }
}
