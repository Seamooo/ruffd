use crate::service::Service;
use ruffd_types::tokio::io::{self, AsyncRead, AsyncWrite};
use ruffd_types::tokio::net::{TcpStream, ToSocketAddrs};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type StdioService = Service<io::BufReader<io::Stdin>, io::Stdout>;
type TcpService = Service<io::BufReader<TcpReader>, TcpWriter>;

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
    /// Panics if more than one StdioServer is still visible.
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

pub struct TcpReader {
    inner: Arc<Mutex<TcpStream>>,
}

impl AsyncRead for TcpReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> core::task::Poll<std::io::Result<()>> {
        let mut lock_guard = self.inner.lock().unwrap();
        let inner = Pin::new(&mut *lock_guard);
        inner.poll_read(cx, buf)
    }
}

pub struct TcpWriter {
    inner: Arc<Mutex<TcpStream>>,
}

impl AsyncWrite for TcpWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
        buf: &[u8],
    ) -> core::task::Poll<std::io::Result<usize>> {
        let mut lock_guard = self.inner.lock().unwrap();
        let inner = Pin::new(&mut *lock_guard);
        inner.poll_write(cx, buf)
    }
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<std::io::Result<()>> {
        let mut lock_guard = self.inner.lock().unwrap();
        let inner = Pin::new(&mut *lock_guard);
        inner.poll_flush(cx)
    }
    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<std::io::Result<()>> {
        let mut lock_guard = self.inner.lock().unwrap();
        let inner = Pin::new(&mut *lock_guard);
        inner.poll_shutdown(cx)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> core::task::Poll<std::io::Result<usize>> {
        let mut lock_guard = self.inner.lock().unwrap();
        let inner = Pin::new(&mut *lock_guard);
        inner.poll_write_vectored(cx, bufs)
    }
    fn is_write_vectored(&self) -> bool {
        // WARNING: below assumes is_write_vectored for TcpStream to avoid locking
        true
    }
}

/// Slight misnomer in the naming of this struct, this describes a
/// type capable of producing a service communicating to a client,
/// over a TcpSocket, however the connection is initialized from this side,
/// rather than binding to a port and listening, hence behaving more like a client
pub struct TcpServer {
    inner: TcpService,
}

impl TcpServer {
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let stream = Arc::new(Mutex::new(stream));
        let reader = io::BufReader::new(TcpReader {
            inner: stream.clone(),
        });
        let writer = TcpWriter { inner: stream };
        let inner = Service::new(reader, writer);
        Ok(Self { inner })
    }
    pub fn get_service_mut(&mut self) -> &mut TcpService {
        &mut self.inner
    }
}
