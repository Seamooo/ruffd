use ruffd_macros::notification;
use ruffd_types::{Notification, RpcError};
use std::collections::HashMap;

#[notification]
fn initialized_notif() -> Result<(), RpcError> {
    dbg!();
    Ok(())
}

lazy_static! {
    pub(crate) static ref NOTIFICATION_REGISTRY: HashMap<&'static str, Notification> = {
        let pairs = vec![("initialized", initialized_notif)];
        pairs
            .into_iter()
            .collect::<HashMap<&'static str, Notification>>()
    };
}
