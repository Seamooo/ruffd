use ruffd_macros::notification;

#[notification]
async fn some_notification(params: i32, bad_param: i32) -> Result<(), ruffd_types::RpcError> {
    Ok(())
} 

fn main() {}

