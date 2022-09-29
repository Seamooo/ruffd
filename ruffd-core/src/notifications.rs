use crate::server_ops::run_diagnostic_op;
use ruffd_macros::notification;
use ruffd_types::lsp_types;
use ruffd_types::tokio::task;
use ruffd_types::{DocumentBuffer, Notification, RuntimeError, ScheduledTask, ServerInitiated};
use std::collections::HashMap;

#[notification]
fn initialized_notif() -> Result<(), RuntimeError> {
    Ok(())
}

#[notification(mut open_buffers)]
fn document_did_open(doc_info: lsp_types::DidOpenTextDocumentParams) -> Result<(), RuntimeError> {
    let key = doc_info.text_document.uri;
    let key_clone = key.clone();
    let val = DocumentBuffer::from_string(doc_info.text_document.text);
    open_buffers.insert(key, val);
    task::spawn(async move {
        let diagnostic_op = run_diagnostic_op(key_clone);
        _scheduler_channel
            .send(ScheduledTask::Server(ServerInitiated::Notification(
                diagnostic_op,
            )))
            .await
            .ok()
            .unwrap();
    });
    Ok(())
}

#[notification(mut open_buffers)]
fn document_did_change(
    doc_info: lsp_types::DidChangeTextDocumentParams,
) -> Result<(), RuntimeError> {
    if let Some(buffer) = open_buffers.get_mut(&doc_info.text_document.uri) {
        for change in doc_info.content_changes.iter() {
            let range = change.range.ok_or(RuntimeError::UnexpectedNone)?;
            let start = (range.start.line as usize, range.start.character as usize);
            let end = (range.end.line as usize, range.end.character as usize);
            buffer.delete_range(start, end)?;
            buffer.insert_text(change.text.as_str(), start)?;
        }
        let uri = doc_info.text_document.uri.clone();
        task::spawn(async move {
            let diagnostic_op = run_diagnostic_op(uri);
            _scheduler_channel
                .send(ScheduledTask::Server(ServerInitiated::Notification(
                    diagnostic_op,
                )))
                .await
                .ok()
                .unwrap();
        });
        Ok(())
    } else {
        Err(RuntimeError::EditUnopenedDocument(
            doc_info.text_document.uri,
        ))
    }
}

lazy_static! {
    pub(crate) static ref NOTIFICATION_REGISTRY: HashMap<&'static str, Notification> = {
        let pairs = vec![
            ("initialized", initialized_notif),
            ("textDocument/didOpen", document_did_open),
            ("textDocument/didChange", document_did_change),
        ];
        pairs
            .into_iter()
            .collect::<HashMap<&'static str, Notification>>()
    };
}
