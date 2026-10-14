use std::path::PathBuf;
use std::sync::Arc;

use basecampd::actions::system::SystemExecutor;
use basecampd::actions::{Executor, RecordingExecutor};
use basecampd::daemon::{Daemon, Options};
use basecampd::devices::{Backend, FakeBackend, HidBackend};
use basecampd::{ipc, store};

const HELP: &str = "basecampd — Base Camp compatible daemon for Mountain peripherals

USAGE: basecampd [OPTIONS]

OPTIONS:
  --socket <PATH>   IPC endpoint (Unix socket path or Windows pipe name)
  --config <DIR>    configuration directory
  --fake            simulate one of each supported device (no hardware needed)
  --dry-run         log host actions instead of running them
  --print-socket    print the default IPC endpoint and exit
  --call <METHOD> [JSON]
                    call a method of a running daemon and print the result
  -v, --verbose     debug logging (repeat for trace)
  -V, --version     print the version
  -h, --help        print this help";

/// Close file descriptors inherited from the parent (e.g. the Electron UI that started
/// the daemon), so the daemon never holds the UI's files or sockets open.
#[cfg(target_os = "linux")]
fn close_inherited_fds() {
    let fds: Vec<i32> = std::fs::read_dir("/proc/self/fd")
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok())).collect())
        .unwrap_or_default();
    for fd in fds.into_iter().filter(|&fd| fd > 2) {
        unsafe { libc::close(fd) };
    }
}

fn main() {
    let mut socket = None;
    let mut config: Option<PathBuf> = None;
    let mut fake = false;
    let mut dry_run = false;
    let mut verbose = 0;
    let mut call: Option<(String, String)> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket = args.next(),
            "--config" => config = args.next().map(PathBuf::from),
            "--fake" => fake = true,
            "--dry-run" => dry_run = true,
            "--print-socket" => {
                println!("{}", ipc::default_endpoint());
                return;
            }
            "--call" => {
                let m = args.next().unwrap_or_default();
                let p = args.next().unwrap_or_else(|| "{}".into());
                call = Some((m, p));
            }
            "-v" | "--verbose" => verbose += 1,
            "-vv" => verbose += 2,
            "-V" | "--version" => {
                println!("basecampd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            other => {
                eprintln!("unknown argument {other:?}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    if let Some((method, params)) = call {
        let endpoint = socket.unwrap_or_else(ipc::default_endpoint);
        let params: serde_json::Value = match serde_json::from_str(&params) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("invalid JSON parameters: {e}");
                std::process::exit(2);
            }
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
        match rt.block_on(ipc::client::call(&endpoint, &method, params)) {
            Ok(Ok(v)) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
            Ok(Err(e)) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("cannot reach the daemon at {endpoint}: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(target_os = "linux")]
    close_inherited_fds();
    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();

    let mut config_dir = config.unwrap_or_else(store::default_config_dir);
    if fake {
        config_dir = config_dir.join("simulated");
    }
    let endpoint = socket.unwrap_or_else(ipc::default_endpoint);

    let backend: Box<dyn Backend> = if fake { Box::new(FakeBackend::demo().0) } else { Box::new(HidBackend) };
    let sys = Arc::new(SystemExecutor::default());
    let exec: Arc<dyn Executor> = if dry_run { Arc::new(RecordingExecutor::default()) } else { sys.clone() };
    let opts = Options { config_dir: Some(config_dir.clone()), simulated: fake, ..Default::default() };
    let daemon = match Daemon::new(opts, backend, exec, Some(sys)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("basecampd: {e}");
            std::process::exit(1);
        }
    };
    log::info!("basecampd {} (config {})", env!("CARGO_PKG_VERSION"), config_dir.display());

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    rt.block_on(async {
        daemon.start();
        let d2 = daemon.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("stopping");
            d2.stop();
            ipc::cleanup();
            std::process::exit(0);
        });
        if let Err(e) = ipc::serve(daemon.clone(), &endpoint).await {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                log::info!("{e}; exiting");
                std::process::exit(0);
            }
            eprintln!("basecampd: IPC error on {endpoint}: {e}");
            std::process::exit(1);
        }
    });
}
