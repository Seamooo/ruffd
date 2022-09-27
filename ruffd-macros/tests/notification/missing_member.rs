use ruffd_macros::notification;

#[notification(open_buffers, bad_struct_member)]
async fn some_notification(_params: i32) -> Result<(), ruffd_types::RpcError> {
    let _open_buffers = open_buffers;
    Ok(())
} 


fn main() {}

