//! Host actions (spec 05 §3): the platform executor and launch-content validation.

pub mod inject;
pub mod obs;
pub mod system;

use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use basecamp_core::keys::{self, HostKey};
use basecamp_core::launch::Launch;
use basecamp_core::shortcut::Shortcut;
use basecamp_core::types::{modifier, LaunchKind};

use crate::error::{Error, Result};

/// Everything the host can do. The real implementation is [`system::SystemExecutor`];
/// tests use [`RecordingExecutor`].
pub trait Executor: Send + Sync {
    fn run_program(&self, path: &str, args: &[String]) -> Result<()>;
    fn open_url(&self, url: &str) -> Result<()>;
    fn open_folder(&self, path: &str) -> Result<()>;
    fn lock_session(&self) -> Result<()>;
    /// `shutdown`, `sleep`, `hibernate`, `calculator`, `explorer`, `taskmanager`.
    fn system(&self, what: &str) -> Result<()>;
    fn key(&self, key: &HostKey, down: bool) -> Result<()>;
    /// 1 left, 2 right, 3 middle, 4 back, 5 forward.
    fn mouse_button(&self, button: u8, down: bool) -> Result<()>;
    fn wheel(&self, amount: i32, horizontal: bool) -> Result<()>;
    fn obs(&self, command: &str, arg: Option<&str>) -> Result<()>;
    fn integration(&self, kind: LaunchKind, args: &[&str]) -> Result<()>;
    fn notify(&self, title: &str, body: &str);
}

/// Press-and-release a shortcut with a 50 ms hold (05 §3.1). `Win+L` locks the session.
pub fn tap_shortcut(x: &dyn Executor, s: &Shortcut, hold_ms: u64) -> Result<()> {
    if s.is_lock() {
        return x.lock_session();
    }
    let mut pressed: Vec<&'static HostKey> = Vec::new();
    for (bit, name) in [(modifier::CTRL, "LCtrl"), (modifier::SHIFT, "LShift"), (modifier::ALT, "LAlt"), (modifier::WIN, "LWin")] {
        if s.mods & bit != 0 {
            pressed.push(keys::by_name(name).unwrap());
        }
    }
    if let Some(k) = s.key {
        pressed.push(k);
    }
    let mut res = Ok(());
    for k in &pressed {
        if let Err(e) = x.key(k, true) {
            res = Err(e);
            break;
        }
    }
    thread::sleep(Duration::from_millis(hold_ms));
    for k in pressed.iter().rev() {
        let _ = x.key(k, false);
    }
    res
}

pub fn tap_key(x: &dyn Executor, k: &HostKey, hold_ms: u64) -> Result<()> {
    x.key(k, true)?;
    thread::sleep(Duration::from_millis(hold_ms));
    x.key(k, false)
}

pub fn is_web_url(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://")) && l.len() > "https://".len() - 1
}

/// Split `path~arg~arg` into the program and its arguments.
pub fn split_program(text: &str) -> (String, Vec<String>) {
    let mut parts = text.split('~');
    let path = parts.next().unwrap_or("").trim().to_string();
    let args = parts.filter(|a| !a.is_empty()).map(|a| a.to_string()).collect();
    (path, args)
}

/// Run a decoded launch payload with the minimum validation of 05 §3. Kinds 8 and 9
/// need the host store and are handled by the caller.
pub fn run_launch(x: &dyn Executor, l: &Launch) -> Result<()> {
    let kind = l.kind().ok_or_else(|| Error::Invalid(format!("unknown launch kind {}", l.kind)))?;
    let args = l.args();
    match kind {
        LaunchKind::Program => {
            let (path, a) = split_program(&l.text);
            if !Path::new(&path).is_file() {
                return Err(Error::NotFound(format!("program {path}")));
            }
            x.run_program(&path, &a)
        }
        LaunchKind::Url => {
            if !is_web_url(&l.text) {
                return Err(Error::Invalid(format!("refusing to open {:?}: only http and https URLs are allowed", l.text)));
            }
            x.open_url(l.text.trim())
        }
        LaunchKind::Folder => {
            if !Path::new(&l.text).is_dir() {
                return Err(Error::NotFound(format!("folder {}", l.text)));
            }
            x.open_folder(&l.text)
        }
        LaunchKind::Obs => x.obs(args[0], args.get(1).copied().filter(|a| !a.is_empty())),
        LaunchKind::Twitch | LaunchKind::YouTube | LaunchKind::Spotify => x.integration(kind, &args),
        LaunchKind::HotkeySwitch | LaunchKind::MultiAction => Err(Error::Invalid("needs the host store".into())),
    }
}

/// Records every call; used by tests and by `--dry-run`.
#[derive(Default)]
pub struct RecordingExecutor {
    pub calls: Mutex<Vec<String>>,
}

impl RecordingExecutor {
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn rec(&self, s: String) -> Result<()> {
        log::info!("[dry-run] {s}");
        self.calls.lock().unwrap().push(s);
        Ok(())
    }
}

impl Executor for RecordingExecutor {
    fn run_program(&self, path: &str, args: &[String]) -> Result<()> {
        self.rec(format!("program {path} {}", args.join(" ")).trim_end().to_string())
    }
    fn open_url(&self, url: &str) -> Result<()> {
        self.rec(format!("url {url}"))
    }
    fn open_folder(&self, path: &str) -> Result<()> {
        self.rec(format!("folder {path}"))
    }
    fn lock_session(&self) -> Result<()> {
        self.rec("lock".into())
    }
    fn system(&self, what: &str) -> Result<()> {
        self.rec(format!("system {what}"))
    }
    fn key(&self, key: &HostKey, down: bool) -> Result<()> {
        self.rec(format!("key {} {}", key.name, if down { "down" } else { "up" }))
    }
    fn mouse_button(&self, button: u8, down: bool) -> Result<()> {
        self.rec(format!("mouse {button} {}", if down { "down" } else { "up" }))
    }
    fn wheel(&self, amount: i32, horizontal: bool) -> Result<()> {
        self.rec(format!("wheel {amount}{}", if horizontal { " h" } else { "" }))
    }
    fn obs(&self, command: &str, arg: Option<&str>) -> Result<()> {
        self.rec(format!("obs {command}{}", arg.map(|a| format!(" ~ {a}")).unwrap_or_default()))
    }
    fn integration(&self, kind: LaunchKind, args: &[&str]) -> Result<()> {
        self.rec(format!("integration {kind:?} {}", args.join("~")))
    }
    fn notify(&self, title: &str, body: &str) {
        let _ = self.rec(format!("notify {title}: {body}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecamp_core::launch;

    #[test]
    fn r9_r10_r11() {
        let x = RecordingExecutor::default();
        run_launch(&x, &launch::decode(&basecamp_core::unhex("02 68 74 74 70 3a 2f 2f 61 00 00")).unwrap()).unwrap();
        assert_eq!(x.take(), vec!["url http://a"]);
        assert!(run_launch(&x, &Launch { kind: 1, text: "/definitely/not/here".into() }).is_err());
        assert!(run_launch(&x, &Launch { kind: 2, text: "file:///etc/passwd".into() }).is_err());
        assert!(x.take().is_empty(), "nothing runs");
    }

    #[test]
    fn shortcut_tap_order() {
        let x = RecordingExecutor::default();
        tap_shortcut(&x, &Shortcut::parse("Ctrl+Shift+X").unwrap(), 0).unwrap();
        assert_eq!(x.take(), vec!["key LCtrl down", "key LShift down", "key X down", "key X up", "key LShift up", "key LCtrl up"]);
        tap_shortcut(&x, &Shortcut::parse("Win+L").unwrap(), 0).unwrap();
        assert_eq!(x.take(), vec!["lock"]);
    }

    #[test]
    fn program_args() {
        assert_eq!(split_program("/usr/bin/foo~-a~b"), ("/usr/bin/foo".into(), vec!["-a".into(), "b".into()]));
    }
}
