//! Local IPC: newline-delimited JSON over a Unix socket (mode 0600, peer UID checked) or a
//! Windows named pipe (local clients only, peer SID checked). Never TCP (spec 02 §2).
//!
//! Request:  `{"id": 1, "method": "ListDevices", "params": {...}}`
//! Response: `{"id": 1, "result": ...}` or `{"id": 1, "error": {"message": "..."}}`
//! Signal:   `{"signal": "DeviceAttached", "data": {...}}`

pub mod client;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::io;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc};

use crate::daemon::Daemon;

/// Is a peer with this UID allowed to talk to the daemon? (B11)
#[cfg(unix)]
pub fn peer_allowed(uid: u32) -> bool {
    unix::authorized(uid)
}

/// Platform default endpoint.
pub fn default_endpoint() -> String {
    #[cfg(unix)]
    {
        unix::default_path()
    }
    #[cfg(windows)]
    {
        windows::default_name()
    }
}

/// Serve until the listener fails. Returns `AlreadyExists` if another daemon owns the
/// endpoint.
pub async fn serve(daemon: Arc<Daemon>, endpoint: &str) -> io::Result<()> {
    serve_bound(daemon, bind(endpoint)?).await
}

#[cfg(unix)]
pub use unix::Bound;
#[cfg(windows)]
pub use windows::Bound;

/// Claim the endpoint (single instance). Must run inside the tokio runtime.
pub fn bind(endpoint: &str) -> io::Result<Bound> {
    #[cfg(unix)]
    {
        unix::bind(endpoint)
    }
    #[cfg(windows)]
    {
        windows::bind(endpoint)
    }
}

/// Accept connections on an endpoint claimed with [`bind`].
pub async fn serve_bound(daemon: Arc<Daemon>, bound: Bound) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::serve(daemon, bound).await
    }
    #[cfg(windows)]
    {
        windows::serve(daemon, bound).await
    }
}

/// Handle one authorised connection.
pub(crate) async fn handle<S>(daemon: Arc<Daemon>, stream: S)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (r, mut w) = tokio::io::split(stream);
    let mut lines = BufReader::new(r).lines();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut signals = daemon.bus.subscribe();
    let writer = tokio::spawn(async move {
        loop {
            let line = tokio::select! {
                m = rx.recv() => match m {
                    Some(s) => s,
                    None => break,
                },
                s = signals.recv() => match s {
                    Ok(sig) => serde_json::to_string(&sig).unwrap_or_default(),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            };
            if w.write_all(line.as_bytes()).await.is_err() || w.write_all(b"\n").await.is_err() {
                break;
            }
        }
    });
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(json!({"id": null, "error": {"message": format!("bad request: {e}")}}).to_string());
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("").to_string();
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let d = daemon.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            if method == "Shutdown" {
                let _ = tx.send(json!({"id": id, "result": true}).to_string());
                // Let the reply reach the client before the runtime winds down.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                log::info!("shutdown requested over IPC");
                request_shutdown();
                return;
            }
            let res = tokio::task::spawn_blocking(move || d.call(&method, &params)).await;
            let msg = match res {
                Ok(Ok(v)) => json!({"id": id, "result": v}),
                Ok(Err(e)) => json!({"id": id, "error": {"message": e.to_string()}}),
                Err(e) => json!({"id": id, "error": {"message": format!("internal error: {e}")}}),
            };
            let _ = tx.send(msg.to_string());
        });
    }
    drop(tx);
    let _ = writer.await;
}

fn shutdown_notify() -> &'static tokio::sync::Notify {
    static N: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// Ask the daemon to stop (IPC `Shutdown`). `main` performs the orderly shutdown.
pub fn request_shutdown() {
    shutdown_notify().notify_one();
}

/// Resolves when the daemon should stop: SIGINT/SIGTERM/SIGHUP on Unix, console close,
/// logoff or shutdown on Windows, or an IPC `Shutdown` request.
pub async fn shutdown_signal() {
    let ipc = shutdown_notify().notified();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut hup = signal(SignalKind::hangup()).expect("SIGHUP handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("SIGINT received"),
            _ = term.recv() => log::info!("SIGTERM received"),
            _ = hup.recv() => log::info!("SIGHUP received"),
            _ = ipc => {}
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        let mut close = windows::ctrl_close().expect("ctrl-close handler");
        let mut logoff = windows::ctrl_logoff().expect("logoff handler");
        let mut shutdown = windows::ctrl_shutdown().expect("shutdown handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("Ctrl-C received"),
            _ = close.recv() => log::info!("console closed"),
            _ = logoff.recv() => log::info!("user logging off"),
            _ = shutdown.recv() => log::info!("system shutting down"),
            _ = ipc => {}
        }
    }
}

/// Remove the socket file on exit (Unix).
pub fn cleanup() {
    #[cfg(unix)]
    unix::cleanup();
}
