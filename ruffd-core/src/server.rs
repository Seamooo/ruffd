use crate::service::Service;
use ruffd_types::tokio::io;
use std::sync::atomic::{AtomicUsize, Ordering};

type StdioService = Service<io::BufReader<io::Stdin>, io::Stdout>;

static STDIO_SERVER_COUNT: AtomicUsize = AtomicUsize::new(0);

pub struct StdioServer {
    inner: StdioService,
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        STDIO_SERVER_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Default for StdioServer {
    /// Instantiatates a new StdioServer instance
    ///
    /// # Panics
    /// Panics if more than one StdioServer is stilil visible.
    /// This is not allowed as there is no way to discriminate  rpc communication
    /// on the same wire
    fn default() -> Self {
        let prev_count = STDIO_SERVER_COUNT.fetch_add(1, Ordering::Relaxed);
        if prev_count != 0 {
            panic!("Cannot instantiate more than one StdioServer")
        }
        let stdout = io::stdout();
        let stdin = io::BufReader::new(io::stdin());
        let inner = Service::new(stdin, stdout);
        Self { inner }
    }
}

impl StdioServer {
    pub fn get_service_mut(&mut self) -> &mut StdioService {
        &mut self.inner
    }
}
