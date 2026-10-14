use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use tokio::net::{UnixListener, UnixStream};

use crate::daemon::Daemon;

static BOUND: Mutex<Option<String>> = Mutex::new(None);

pub fn default_path() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => format!("{d}/sherpad.sock"),
        _ => format!("/tmp/sherpad-{}.sock", unsafe { libc::geteuid() }),
    }
}

/// Only the daemon's own user may connect (B11).
pub fn authorized(peer_uid: u32) -> bool {
    peer_uid == unsafe { libc::geteuid() }
}

pub async fn serve(daemon: Arc<Daemon>, path: &str) -> io::Result<()> {
    if UnixStream::connect(path).await.is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("another daemon is listening on {path}")));
    }
    let _ = std::fs::remove_file(path);
    let old = unsafe { libc::umask(0o177) };
    let bound = UnixListener::bind(path);
    unsafe { libc::umask(old) };
    let listener = bound?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    *BOUND.lock().unwrap() = Some(path.to_string());
    log::info!("listening on {path}");
    loop {
        let (stream, _) = listener.accept().await?;
        match stream.peer_cred() {
            Ok(c) if authorized(c.uid()) => {
                tokio::spawn(super::handle(daemon.clone(), stream));
            }
            Ok(c) => log::warn!("refused IPC connection from uid {}", c.uid()),
            Err(e) => log::warn!("refused IPC connection: {e}"),
        }
    }
}

pub fn cleanup() {
    if let Some(p) = BOUND.lock().unwrap().take() {
        let _ = std::fs::remove_file(p);
    }
}
