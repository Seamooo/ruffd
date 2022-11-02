use crate::ruff_utils::diagnostic_from_check;
use ruffd_types::ruff::check;
use ruffd_types::tokio::sync::mpsc::Sender;
use ruffd_types::tokio::sync::Mutex;
use ruffd_types::{lsp_types, serde_json};
use ruffd_types::{
    CheckRegistry, CreateLocksFn, RpcNotification, RwGuarded, RwReq, ScheduledTask,
    ServerNotification, ServerNotificationExec, ServerState, ServerStateHandles, ServerStateLocks,
};
use std::sync::Arc;

// TODO move below macros to ruffd_types and export create_locks_fut
// and unwrap_state_handles

macro_rules! tup_pat_setter {
    ($rv:ident, mut $name:ident, $($tail:tt)*) => {
        $rv.$name = $name;
        tup_pat_setter!($rv, $($tail)*);
    };
    ($rv:ident, $name:ident, $($tail:tt)*) => {
        $rv.$name = $name;
        tup_pat_setter!($rv, $($tail)*);
    };
    ($rv:ident, mut $name:ident) => {
        $rv.$name = $name;
    };
    ($rv:ident, $name:ident) => {
        $rv.$name = $name;
    };
    ($rv:ident,) => {};
    ($rv:ident) => {};
}

macro_rules! create_read_lock {
    ($handle:ident, $name:ident) => {
        let $name = Some(RwReq::Read($handle.$name.clone()));
    };
}

macro_rules! create_write_lock {
    ($handle:ident, $name:ident) => {
        let $name = Some(RwReq::Write($handle.$name.clone()));
    };
}

macro_rules! create_locks_statements {
    ($handle:ident, mut $name:ident, $($tail:tt)*) => {
        create_write_lock!($handle, $name);
        create_locks_statements!($handle, $($tail)*);
    };
    ($handle:ident, $name:ident, $($tail:tt)*) => {
        create_read_lock!($handle, $name);
        create_locks_statements!($handle, $($tail)*);
    };
    ($handle:ident, mut $name:ident) => {
        create_write_lock!($handle, $name);
    };
    ($handle:ident, $name:ident) => {
        create_read_lock!($handle, $name);
    };
    ($handle:ident,) => {};
    ($handle:ident) => {};
}

// Clippy will yell for not using ..Default::default() if macro_rules!
// gets linting in its expansion but macro syntax inside the struct
// initializer is not allowed
macro_rules! create_locks_fut {
    ($($args:tt)*) => {
        Box::new(|state: Arc<Mutex<ServerState>>| {
            Box::pin(async move {
                let handle = state.lock().await;
                create_locks_statements!(handle, $($args)*);
                let mut rv = ServerStateLocks::default();
                tup_pat_setter!(rv, $($args)*);
                rv
            })
        })
    };
}

macro_rules! unwrap_write_handle {
    ($handles:ident, $name:ident) => {
        let mut $name = match $handles.$name.unwrap() {
            RwGuarded::Write(x) => x,
            _ => unreachable!(),
        };
    };
}

macro_rules! unwrap_read_handle {
    ($handles:ident, $name:ident) => {
        let $name = match $handles.$name.unwrap() {
            RwGuarded::Read(x) => x,
            _ => unreachable!(),
        };
    };
}

macro_rules! unwrap_state_handles {
    ($handles:ident, mut $name:ident, $($tail:tt)*) => {
        unwrap_write_handle!($handles, $ident);
        unwrap_state_handles!($handles, $($tail)*);
    };
    ($handles:ident, $name:ident, $($tail:tt)*) => {
        unwrap_read_handle!($handles, $name);
        unwrap_state_handles!($handles, $($tail)*);
    };
    ($handles:ident, mut $name:ident) => {
        unwrap_write_handle!($handles, $name);
    };
    ($handles:ident, $name:ident) => {
        unwrap_read_handle!($handles, $name);
    };
    ($handles:ident,) => {};
    ($handles:ident) => {};
}

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
