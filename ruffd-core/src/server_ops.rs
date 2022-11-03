use crate::ruff_utils::diagnostic_from_check;
use ruffd_types::ruff::check;
use ruffd_types::tokio::sync::mpsc::Sender;
use ruffd_types::{create_locks_fut, unwrap_state_handles};
use ruffd_types::{lsp_types, serde_json};
use ruffd_types::{
    CheckRegistry, CreateLocksFn, RpcNotification, ScheduledTask, ServerNotification,
    ServerNotificationExec, ServerStateHandles,
};

pub fn run_diagnostic_op(document_uri: lsp_types::Url) -> ServerNotification {
    let exec: ServerNotificationExec = Box::new(
        move |state_handles: ServerStateHandles<'_>, _scheduler_channel: Sender<ScheduledTask>| {
            Box::pin(async move {
                unwrap_state_handles!(state_handles, open_buffers, mut checks);

                let check_vec = {
                    if let Some(buffer) = open_buffers.get(&document_uri) {
                        let doc = buffer.iter().collect::<String>();
                        if let Ok(path) = document_uri.to_file_path() {
                            check(&path, doc.as_str(), true).unwrap_or_default()
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                };
                let diagnostics = check_vec
                    .iter()
                    .map(diagnostic_from_check)
                    .collect::<Vec<_>>();
                // for now, recreate the registry every op
                let registry = CheckRegistry::from_iter(check_vec);
                checks.insert(document_uri.clone(), registry);
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
    let create_locks: CreateLocksFn = create_locks_fut!(open_buffers, mut checks);
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
        let diagnostics = check(&path, doc, true)
            .unwrap()
            .iter()
            .map(diagnostic_from_check)
            .collect::<Vec<_>>();
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
