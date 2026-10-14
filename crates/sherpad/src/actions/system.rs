//! The real executor: starts programs as the user (never through a shell), opens URLs and
//! folders, locks the session, injects input and talks to OBS.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;

use sherpa_core::keys::HostKey;
use sherpa_core::types::LaunchKind;

use super::inject::Injector;
use super::obs::{Obs, ObsConfig};
use super::Executor;
use crate::error::{Error, Result};

pub struct SystemExecutor {
    injector: Mutex<Option<Injector>>,
    pub obs: Mutex<ObsConfig>,
}

impl Default for SystemExecutor {
    fn default() -> Self {
        SystemExecutor { injector: Mutex::new(None), obs: Mutex::new(ObsConfig::default()) }
    }
}

/// Spawn a detached process and reap it in the background.
fn spawn(program: &str, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = cmd.spawn().map_err(|e| Error::Io(format!("{program}: {e}")))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Run a command and wait for success.
#[cfg(not(windows))]
fn run(program: &str, args: &[&str]) -> Result<()> {
    let st = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| Error::Io(format!("{program}: {e}")))?;
    if st.success() {
        Ok(())
    } else {
        Err(Error::Io(format!("{program} failed ({st})")))
    }
}

/// Try candidates in order until one starts.
#[cfg(not(windows))]
fn spawn_first(candidates: &[&[&str]]) -> Result<()> {
    let mut last = Error::NotFound("no suitable program".into());
    for c in candidates {
        match spawn(c[0], &c[1..]) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(windows)]
fn shell_open(target: &str) -> Result<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as isize <= 32 {
        return Err(Error::Io(format!("ShellExecute failed for {target}")));
    }
    Ok(())
}

impl SystemExecutor {
    fn with_injector<T>(&self, f: impl FnOnce(&mut Injector) -> Result<T>) -> Result<T> {
        let mut g = self.injector.lock().unwrap();
        if g.is_none() {
            *g = Some(Injector::open()?);
        }
        f(g.as_mut().unwrap())
    }

    /// Whether synthetic input is available (uinput permission on Linux).
    pub fn input_available(&self) -> bool {
        self.with_injector(|_| Ok(())).is_ok()
    }
}

impl Executor for SystemExecutor {
    fn run_program(&self, path: &str, args: &[String]) -> Result<()> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        spawn(path, &a)
    }

    fn open_url(&self, url: &str) -> Result<()> {
        #[cfg(windows)]
        {
            shell_open(url)
        }
        #[cfg(not(windows))]
        {
            spawn("xdg-open", &[url])
        }
    }

    fn open_folder(&self, path: &str) -> Result<()> {
        #[cfg(windows)]
        {
            shell_open(path)
        }
        #[cfg(not(windows))]
        {
            spawn("xdg-open", &[path])
        }
    }

    fn lock_session(&self) -> Result<()> {
        #[cfg(windows)]
        {
            unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map_err(|e| Error::Io(e.to_string()))
        }
        #[cfg(not(windows))]
        {
            run("loginctl", &["lock-session"])
                .or_else(|_| run("loginctl", &["lock-sessions"]))
                .or_else(|_| run("xdg-screensaver", &["lock"]))
        }
    }

    fn system(&self, what: &str) -> Result<()> {
        #[cfg(windows)]
        {
            match what {
                "shutdown" => spawn("shutdown", &["/s", "/t", "0"]),
                "sleep" => spawn("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
                "hibernate" => spawn("shutdown", &["/h"]),
                "calculator" => spawn("calc.exe", &[]),
                "explorer" => spawn("explorer.exe", &[]),
                "taskmanager" => spawn("taskmgr.exe", &[]),
                other => Err(Error::Unsupported(other.to_string())),
            }
        }
        #[cfg(not(windows))]
        {
            match what {
                "shutdown" => run("systemctl", &["poweroff"]),
                "sleep" => run("systemctl", &["suspend"]),
                "hibernate" => run("systemctl", &["hibernate"]),
                "calculator" => spawn_first(&[&["gnome-calculator"], &["kcalc"], &["galculator"], &["mate-calc"], &["xcalc"]]),
                "explorer" => {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
                    spawn("xdg-open", &[&home])
                }
                "taskmanager" => spawn_first(&[
                    &["gnome-system-monitor"],
                    &["plasma-systemmonitor"],
                    &["ksysguard"],
                    &["xfce4-taskmanager"],
                    &["mate-system-monitor"],
                ]),
                other => Err(Error::Unsupported(other.to_string())),
            }
        }
    }

    fn key(&self, key: &HostKey, down: bool) -> Result<()> {
        self.with_injector(|i| i.key(key, down))
    }

    fn mouse_button(&self, button: u8, down: bool) -> Result<()> {
        self.with_injector(|i| i.mouse_button(button, down))
    }

    fn wheel(&self, amount: i32, horizontal: bool) -> Result<()> {
        self.with_injector(|i| i.wheel(amount, horizontal))
    }

    fn obs(&self, command: &str, arg: Option<&str>) -> Result<()> {
        let cfg = self.obs.lock().unwrap().clone();
        Obs::new(cfg).run(command, arg)
    }

    fn integration(&self, kind: LaunchKind, args: &[&str]) -> Result<()> {
        // TODO(spec): Twitch, YouTube and Spotify need their own OAuth applications
        // (F-INT-1/F-INT-2). They are not shipped in this version.
        Err(Error::Unsupported(format!("{kind:?} integration is not configured (action {})", args.first().unwrap_or(&""))))
    }

    fn notify(&self, title: &str, body: &str) {
        log::info!("notification: {title}: {body}");
        #[cfg(not(windows))]
        {
            let _ = spawn("notify-send", &["-a", "Sherpa", title, body]);
        }
    }
}
