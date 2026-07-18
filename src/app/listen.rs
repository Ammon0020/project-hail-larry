//! Coordinated HTTP and HTTPS listeners for the daemon.
//!
//! TLS mode binds both configured addresses before serving either one. A failed
//! HTTPS bind therefore cannot silently downgrade a TLS-enabled daemon to
//! cleartext-only operation.

use std::fs::File;
use std::future::Future;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use http_body::Body as HttpBody;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::{self, Instant, Sleep};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::{info, warn};

use crate::config::Config;

use super::daemon::{resolved_https_port, BoundAddresses};
use super::tls_cert;

/// Match Go's `http.Server.ReadHeaderTimeout`.
const HTTP_READ_HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Match Go's `http.Server.ReadTimeout` while a handler consumes its body.
const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Match Go's `http.Server.WriteTimeout` for handler and response-body work.
const HTTP_WRITE_TIMEOUT: Duration = Duration::from_secs(60);
/// Match Go's `http.Server.IdleTimeout` for an otherwise inactive connection.
const HTTP_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// TCP listeners already bound for a daemon run.
pub struct Listeners {
    http: TcpListener,
    https: Option<(TcpListener, TlsAcceptor)>,
}

impl Listeners {
    /// Return the concrete OS-assigned addresses.
    pub fn addresses(&self) -> BoundAddresses {
        BoundAddresses {
            http: self.http.local_addr().unwrap_or_else(|_| {
                // Binding succeeded; this fallback is only for an OS error
                // querying a listener already scheduled to close.
                SocketAddr::from(([0, 0, 0, 0], 0))
            }),
            https: self
                .https
                .as_ref()
                .and_then(|(listener, _)| listener.local_addr().ok()),
        }
    }
}

/// Bind HTTP and, when configured, HTTPS listeners.
pub async fn bind(config: &Config) -> Result<Listeners> {
    let http_address = socket_address(&config.host, config.port)?;
    let http = TcpListener::bind(http_address)
        .await
        .with_context(|| format!("bind HTTP listener at {http_address}"))?;

    if !config.tls_enabled {
        if is_wildcard_host(&config.host) {
            warn!("TLS is disabled while HTTP is bound to all interfaces; credentials travel in cleartext");
        }
        return Ok(Listeners { http, https: None });
    }

    let https_address = socket_address(&config.host, resolved_https_port(config)?)?;
    let cert_dir = if config.tls_cert_dir.is_empty() {
        std::path::Path::new(&config.data_dir).join("tls")
    } else {
        std::path::PathBuf::from(&config.tls_cert_dir)
    };
    let cert_paths = tls_cert::ensure_self_signed(&cert_dir, &config.host)?;
    let tls = load_tls_acceptor(&cert_paths)?;
    let https = TcpListener::bind(https_address)
        .await
        .with_context(|| format!("bind HTTPS listener at {https_address}"))?;
    Ok(Listeners {
        http,
        https: Some((https, tls)),
    })
}

/// Serve all bound listeners until cancellation, gracefully draining HTTP.
pub async fn serve(listeners: Listeners, router: Router, cancel: CancellationToken) -> Result<()> {
    let Listeners { http, https } = listeners;
    let router = with_timeouts(router);
    let http_cancel = cancel.child_token();
    let http = serve_http(http, router.clone(), http_cancel);

    let https = async move {
        match https {
            Some((listener, acceptor)) => serve_https(listener, acceptor, router, cancel).await,
            None => Ok(()),
        }
    };

    tokio::try_join!(http, https)?;
    Ok(())
}

/// Serve cleartext HTTP with the same per-connection safeguards as HTTPS.
async fn serve_http(
    listener: TcpListener,
    router: Router,
    cancel: CancellationToken,
) -> Result<()> {
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    info!(address = %listener.local_addr().context("read HTTP listener address")?, "serving HTTP endpoint");
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("accept HTTP connection")?;
                let make_service = make_service.clone();
                connections.spawn(async move {
                    serve_connection(stream, peer, make_service).await;
                });
            }
        }
    }
    drain_connections(&mut connections).await;
    Ok(())
}

async fn serve_https(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    router: Router,
    cancel: CancellationToken,
) -> Result<()> {
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    info!(address = %listener.local_addr().context("read HTTPS listener address")?, "serving HTTPS endpoint");
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("accept HTTPS connection")?;
                let acceptor = acceptor.clone();
                let make_service = make_service.clone();
                connections.spawn(async move {
                    let stream = match acceptor.accept(stream).await {
                        Ok(stream) => stream,
                        Err(error) => {
                            warn!(%peer, %error, "TLS handshake failed");
                            return;
                        }
                    };
                    let service = match make_service.oneshot(peer).await {
                        Ok(service) => service,
                        Err(error) => {
                            warn!(%peer, %error, "create HTTPS request service failed");
                            return;
                        }
                    };
                    serve_connection_with_service(stream, peer, service).await;
                });
            }
        }
    }
    drain_connections(&mut connections).await;
    Ok(())
}

/// Build a Hyper connection with the native HTTP/1 header deadline enabled.
fn connection_builder() -> ConnectionBuilder<TokioExecutor> {
    let mut builder = ConnectionBuilder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(Some(HTTP_READ_HEADER_TIMEOUT));
    builder
}

/// Create an Axum request service for one peer, then serve its TCP connection.
async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    make_service: axum::extract::connect_info::IntoMakeServiceWithConnectInfo<Router, SocketAddr>,
) {
    let service = match make_service.oneshot(peer).await {
        Ok(service) => service,
        Err(error) => {
            warn!(%peer, %error, "create HTTP request service failed");
            return;
        }
    };
    serve_connection_with_service(stream, peer, service).await;
}

/// Serve either a TCP or TLS stream with shared timeout enforcement.
async fn serve_connection_with_service<S, Service>(stream: S, peer: SocketAddr, service: Service)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    Service: tower::Service<hyper::Request<hyper::body::Incoming>, Response = axum::response::Response>
        + Clone
        + Send
        + 'static,
    Service::Future: Send + 'static,
    Service::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    if let Err(error) = connection_builder()
        .serve_connection_with_upgrades(
            TokioIo::new(IdleIo::new(stream)),
            TowerToHyperService::new(service),
        )
        .await
    {
        warn!(%peer, %error, "HTTP connection failed");
    }
}

/// Wait for existing connections so shutdown does not drop in-flight handlers.
async fn drain_connections(connections: &mut JoinSet<()>) {
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            warn!(%error, "HTTP connection task stopped unexpectedly");
        }
    }
}

/// An I/O adapter that ends a connection after the configured inactivity gap.
///
/// Hyper natively supports only the HTTP/1 header deadline. This adapter
/// supplies Go's idle-connection deadline across both cleartext and TLS paths.
struct IdleIo<S> {
    inner: S,
    deadline: Pin<Box<Sleep>>,
    write_deadline: Option<Pin<Box<Sleep>>>,
}

impl<S> IdleIo<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            deadline: Box::pin(time::sleep(HTTP_IDLE_TIMEOUT)),
            write_deadline: None,
        }
    }

    fn poll_idle(&mut self, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        if self.deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP connection idle timeout",
            )));
        }
        Poll::Pending
    }

    fn record_activity(&mut self) {
        self.deadline
            .as_mut()
            .reset(Instant::now() + HTTP_IDLE_TIMEOUT);
    }

    /// Start (or check) a deadline for one socket write that is awaiting
    /// kernel progress. Unlike idle reads, this does not shorten keep-alive
    /// connections: it is armed only while Hyper is actively writing.
    fn write_timed_out(&mut self, cx: &mut TaskContext<'_>) -> bool {
        let deadline = self
            .write_deadline
            .get_or_insert_with(|| Box::pin(time::sleep(HTTP_WRITE_TIMEOUT)));
        deadline.as_mut().poll(cx).is_ready()
    }

    fn finish_write(&mut self) {
        self.write_deadline = None;
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for IdleIo<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if let Poll::Ready(result) = this.poll_idle(cx) {
            return Poll::Ready(result);
        }
        let filled_before = buf.filled().len();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) if buf.filled().len() > filled_before => {
                this.record_activity();
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for IdleIo<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        if let Poll::Ready(result) = this.poll_idle(cx) {
            return Poll::Ready(result.map(|()| 0));
        }
        if !buf.is_empty() && this.write_timed_out(cx) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP response write timeout",
            )));
        }
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) if written > 0 => {
                this.finish_write();
                this.record_activity();
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(error)) => {
                this.finish_write();
                Poll::Ready(Err(error))
            }
            result => result,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if let Poll::Ready(result) = this.poll_idle(cx) {
            return Poll::Ready(result);
        }
        if this.write_timed_out(cx) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP response write timeout",
            )));
        }
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                this.finish_write();
                this.record_activity();
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                this.finish_write();
                Poll::Ready(Err(error))
            }
            result => result,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if let Poll::Ready(result) = this.poll_idle(cx) {
            return Poll::Ready(result);
        }
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        if let Poll::Ready(result) = this.poll_idle(cx) {
            return Poll::Ready(result.map(|()| 0));
        }
        if bufs.iter().any(|buf| !buf.is_empty()) && this.write_timed_out(cx) {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "HTTP response write timeout",
            )));
        }
        match Pin::new(&mut this.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(Ok(written)) if written > 0 => {
                this.finish_write();
                this.record_activity();
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(error)) => {
                this.finish_write();
                Poll::Ready(Err(error))
            }
            result => result,
        }
    }
}

/// A body adapter that fails when its complete transfer exceeds a deadline.
struct DeadlineBody {
    inner: Pin<Box<Body>>,
    deadline: Pin<Box<Sleep>>,
    timeout_message: &'static str,
}

impl DeadlineBody {
    fn new(inner: Body, timeout: Duration, timeout_message: &'static str) -> Self {
        Self {
            inner: Box::pin(inner),
            deadline: Box::pin(time::sleep(timeout)),
            timeout_message,
        }
    }
}

impl HttpBody for DeadlineBody {
    type Data = axum::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.as_mut().get_mut();
        if this.deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(axum::Error::new(io::Error::new(
                io::ErrorKind::TimedOut,
                this.timeout_message,
            )))));
        }
        this.inner.as_mut().poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.as_ref().is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.as_ref().size_hint()
    }
}

/// Apply the Go server's read and write deadlines at the Axum boundary.
///
/// Hyper's native `header_read_timeout` is HTTP/1-only; the body and response
/// adapters below cover HTTP/1 and HTTP/2. They differ from Go in two ways:
/// HTTP body transfers are timed from handler entry rather than accept, and a
/// response-body timeout closes the stream instead of changing a started
/// response. Router-level body caps remain 10 MiB in `crate::api` (50 MiB on
/// file-write POST only).
fn with_timeouts(router: Router) -> Router {
    router
        .layer(middleware::from_fn(response_write_timeout))
        .layer(middleware::from_fn(request_body_read_timeout))
}

/// Cap body consumption so a client cannot hold a handler with a slow upload.
async fn request_body_read_timeout(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let request = Request::from_parts(
        parts,
        Body::new(DeadlineBody::new(
            body,
            HTTP_READ_TIMEOUT,
            "HTTP request body read timed out",
        )),
    );
    next.run(request).await
}

/// Cap handler execution and response streaming at Go's write deadline.
async fn response_write_timeout(request: Request, next: Next) -> Response {
    match time::timeout(HTTP_WRITE_TIMEOUT, next.run(request)).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Response::from_parts(
                parts,
                Body::new(DeadlineBody::new(
                    body,
                    HTTP_WRITE_TIMEOUT,
                    "HTTP response write timed out",
                )),
            )
        }
        Err(_) => {
            warn!("HTTP request processing exceeded write timeout");
            (
                StatusCode::GATEWAY_TIMEOUT,
                "HTTP request processing timed out",
            )
                .into_response()
        }
    }
}

fn socket_address(host: &str, port: i64) -> Result<SocketAddr> {
    let port = u16::try_from(port).map_err(|_| anyhow!("invalid configured port {port}"))?;
    let host = if host.is_empty() { "0.0.0.0" } else { host };
    format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parse listen address {host}:{port}"))
}

fn is_wildcard_host(host: &str) -> bool {
    host.is_empty() || matches!(host, "0.0.0.0" | "::")
}

fn load_tls_acceptor(paths: &tls_cert::CertificatePaths) -> Result<TlsAcceptor> {
    let cert_file = File::open(&paths.cert)
        .with_context(|| format!("open TLS certificate {}", paths.cert.display()))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse TLS certificate PEM")?;
    if certs.is_empty() {
        return Err(anyhow!("TLS certificate PEM contains no certificates"));
    }

    let key_file = File::open(&paths.key)
        .with_context(|| format!("open TLS private key {}", paths.key.display()))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("parse TLS private key PEM")?
        .ok_or_else(|| anyhow!("TLS private key PEM contains no supported key"))?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build rustls server configuration")?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_timeouts_match_go_server_defaults() {
        assert_eq!(HTTP_READ_HEADER_TIMEOUT, Duration::from_secs(5));
        assert_eq!(HTTP_READ_TIMEOUT, Duration::from_secs(30));
        assert_eq!(HTTP_WRITE_TIMEOUT, Duration::from_secs(60));
        assert_eq!(HTTP_IDLE_TIMEOUT, Duration::from_secs(120));
    }
}
