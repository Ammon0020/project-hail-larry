//! Coordinated HTTP and HTTPS listeners for the daemon.
//!
//! TLS mode binds both configured addresses before serving either one. A failed
//! HTTPS bind therefore cannot silently downgrade a TLS-enabled daemon to
//! cleartext-only operation.

use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::{info, warn};

use crate::config::Config;

use super::daemon::{resolved_https_port, BoundAddresses};
use super::tls_cert;

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
    let http_cancel = cancel.child_token();
    let http_router = router.clone();
    let http = async move {
        axum::serve(
            http,
            http_router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(http_cancel.cancelled_owned())
        .await
        .context("HTTP listener exited")
    };

    let https = async move {
        match https {
            Some((listener, acceptor)) => serve_https(listener, acceptor, router, cancel).await,
            None => Ok(()),
        }
    };

    tokio::try_join!(http, https)?;
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
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("accept HTTPS connection")?;
                let acceptor = acceptor.clone();
                let make_service = make_service.clone();
                tokio::spawn(async move {
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
                    if let Err(error) = ConnectionBuilder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(
                            TokioIo::new(stream),
                            TowerToHyperService::new(service),
                        )
                        .await
                    {
                        warn!(%peer, %error, "HTTPS connection failed");
                    }
                });
            }
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
