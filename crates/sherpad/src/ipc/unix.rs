use std::io;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use tokio::net::UnixListener;

use crate::daemon::Daemon;

static BOUND: Mutex<Option<String>> = Mutex::new(None);
/// Held for the daemon's lifetime: an exclusive `flock` on `<socket>.lock`.
static LOCK: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Take the single-instance lock, so two daemons can never race to (re)bind the socket.
fn lock_instance(path: &str) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(format!("{path}.lock"))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("another daemon owns {path}")));
        }
        return Err(e);
    }
    *LOCK.lock().unwrap() = Some(f);
    Ok(())
}

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

pub type Bound = UnixListener;

/// Take the instance lock and bind the socket (mode 0600). Fails with `AlreadyExists` if
/// another daemon owns it.
pub fn bind(path: &str) -> io::Result<Bound> {
    lock_instance(path)?;
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
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
    Ok(listener)
}

pub async fn serve(daemon: Arc<Daemon>, listener: Bound) -> io::Result<()> {
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
