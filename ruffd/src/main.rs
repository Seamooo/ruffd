use clap::Parser;
use ruffd_core::server::{StdioServer, TcpServer};
use ruffd_types::tokio;

#[derive(Parser, Debug)]
struct PipeArg {
    #[arg(required_unless_present("named_pipe"))]
    pos_pipe: Option<String>,
    #[arg(long("pipe"), required_unless_present("pos_pipe"))]
    named_pipe: Option<String>,
}

// Below is opinionated (not based on the specification)
// prioritise named argument if present
impl Into<String> for PipeArg {
    fn into(self) -> String {
        match (self.pos_pipe, self.named_pipe) {
            (None, Some(pipe)) => pipe,
            (Some(pipe), None) => pipe,
            (Some(_), Some(named)) => named,
            _ => unreachable!(),
        }
    }
}

#[derive(Parser, Debug)]
struct PortArg {
    #[arg(required_unless_present("named_port"))]
    pos_port: Option<u64>,
    #[arg(long("port"), required_unless_present("pos_port"))]
    named_port: Option<u64>,
}

// Below is opinionated (not based on the specification)
// prioritise named argument if present
impl Into<u64> for PortArg {
    fn into(self) -> u64 {
        match (self.pos_port, self.named_port) {
            (None, Some(port)) => port,
            (Some(port), None) => port,
            (Some(_), Some(named)) => named,
            _ => unreachable!(),
        }
    }
}

#[derive(clap::Subcommand, Debug)]
enum CommMode {
    Stdio,
    Socket {
        /// Port number to connect to client
        #[command(flatten)]
        port: PortArg,
    },
    Pipe {
        /// Pipe name or socket filename
        #[command(flatten)]
        pipe: PipeArg,
    },
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    comm_mode: Option<CommMode>,
}

async fn run_stdio_server() {
    let mut server = StdioServer::default();
    server.get_service_mut().run().await;
}

async fn run_tcp_server(port: u64) {
    let mut server = TcpServer::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    server.get_service_mut().run().await;
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Some(comm_mode) = cli.comm_mode {
        match comm_mode {
            CommMode::Stdio => run_stdio_server().await,
            CommMode::Socket { port } => run_tcp_server(port.into()).await,
            _ => unimplemented!(),
        }
    } else {
        run_stdio_server().await;
    }
}
