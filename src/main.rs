use ruffd_core::Server;
use ruffd_types::tokio;

#[tokio::main]
async fn main() {
    let mut server = Server::new();
    server.run_stdio().await;
}
