use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::Arc;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetLengthSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

use crate::daemon::Daemon;

pub fn default_name() -> String {
    let user: String = std::env::var("USERNAME")
        .unwrap_or_else(|_| "user".into())
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!(r"\\.\pipe\sherpad-{user}")
}

fn token_sid(process: HANDLE) -> Option<Vec<u8>> {
    unsafe {
        let mut tok = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut tok).ok()?;
        let mut len = 0u32;
        let _ = GetTokenInformation(tok, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let r = GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr() as *mut _), len, &mut len);
        let _ = CloseHandle(tok);
        r.ok()?;
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid = tu.User.Sid;
        let n = GetLengthSid(sid);
        Some(std::slice::from_raw_parts(sid.0 as *const u8, n as usize).to_vec())
    }
}

/// The client process must run as the same user as the daemon (B11).
fn same_user(pipe: &NamedPipeServer) -> bool {
    unsafe {
        let h = HANDLE(pipe.as_raw_handle() as _);
        let mut pid = 0u32;
        if GetNamedPipeClientProcessId(h, &mut pid).is_err() {
            return false;
        }
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let other = token_sid(proc);
        let _ = CloseHandle(proc);
        let me = token_sid(GetCurrentProcess());
        matches!((other, me), (Some(a), Some(b)) if a == b)
    }
}

pub async fn serve(daemon: Arc<Daemon>, name: &str) -> io::Result<()> {
    let mut server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(name).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            io::Error::new(io::ErrorKind::AlreadyExists, format!("another daemon owns {name}"))
        } else {
            e
        }
    })?;
    log::info!("listening on {name}");
    loop {
        server.connect().await?;
        let client = server;
        server = ServerOptions::new().reject_remote_clients(true).create(name)?;
        if same_user(&client) {
            tokio::spawn(super::handle(daemon.clone(), client));
        } else {
            log::warn!("refused IPC connection from another user");
        }
    }
}
