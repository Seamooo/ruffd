use ruffd_core::server::StdioServer;
use ruffd_types::tokio;

#[tokio::main]
async fn main() {
    let mut server = StdioServer::default();
    server.get_service_mut().run().await;
}
