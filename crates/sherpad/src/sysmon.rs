//! PC metrics and master volume for the media dock (spec 01 §7).

use std::process::Command;
use std::time::Instant;

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

pub struct SysMon {
    sys: System,
    disks: Disks,
    nets: Networks,
    last_net: Option<(Instant, u64)>,
}

impl Default for SysMon {
    fn default() -> Self {
        Self::new()
    }
}

impl SysMon {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new().with_cpu(CpuRefreshKind::new().with_cpu_usage()).with_memory(MemoryRefreshKind::new().with_ram()),
        );
        SysMon { sys, disks: Disks::new_with_refreshed_list(), nets: Networks::new_with_refreshed_list(), last_net: None }
    }

    pub fn cpu(&mut self) -> u32 {
        self.sys.refresh_cpu_usage();
        self.sys.global_cpu_usage().round() as u32
    }

    pub fn ram(&mut self) -> u32 {
        self.sys.refresh_memory();
        let total = self.sys.total_memory().max(1);
        (self.sys.used_memory() * 100 / total) as u32
    }

    pub fn disk(&mut self) -> u32 {
        self.disks.refresh_list();
        let (mut total, mut avail) = (0u64, 0u64);
        for d in self.disks.list() {
            total += d.total_space();
            avail += d.available_space();
        }
        if total == 0 {
            return 0;
        }
        ((total - avail) * 100 / total) as u32
    }

    /// Network throughput in Mbit/s, clamped to 0..100.
    /// TODO(spec): the unit of the dock's network value is unknown.
    pub fn network(&mut self) -> u32 {
        self.nets.refresh();
        let bytes: u64 = self.nets.values().map(|n| n.total_received() + n.total_transmitted()).sum();
        let now = Instant::now();
        let v = match self.last_net {
            Some((t, b)) => {
                let secs = now.duration_since(t).as_secs_f64().max(0.001);
                ((bytes.saturating_sub(b) as f64 * 8.0 / 1_000_000.0) / secs).round() as u32
            }
            None => 0,
        };
        self.last_net = Some((now, bytes));
        v.min(100)
    }

    /// GPU load. Linux: AMD/Intel sysfs `gpu_busy_percent`. TODO: NVIDIA and Windows.
    pub fn gpu(&mut self) -> u32 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(rd) = std::fs::read_dir("/sys/class/drm") {
                for e in rd.flatten() {
                    if let Ok(s) = std::fs::read_to_string(e.path().join("device/gpu_busy_percent")) {
                        if let Ok(v) = s.trim().parse::<u32>() {
                            return v.min(100);
                        }
                    }
                }
            }
        }
        0
    }
}

/// System master volume 0..100.
pub fn master_volume() -> Option<u8> {
    #[cfg(windows)]
    {
        windows_volume()
    }
    #[cfg(not(windows))]
    {
        // PipeWire, then PulseAudio.
        if let Ok(o) = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(v) = s.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()) {
                return Some((v * 100.0).round().clamp(0.0, 100.0) as u8);
            }
        }
        if let Ok(o) = Command::new("pactl").args(["get-sink-volume", "@DEFAULT_SINK@"]).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Some(p) = s.split('/').nth(1).and_then(|p| p.trim().trim_end_matches('%').trim().parse::<u32>().ok()) {
                return Some(p.min(100) as u8);
            }
        }
        None
    }
}

#[cfg(windows)]
fn windows_volume() -> Option<u8> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        let v = vol.GetMasterVolumeLevelScalar().ok()?;
        Some((v * 100.0).round().clamp(0.0, 100.0) as u8)
    }
}

#[allow(dead_code)]
fn _unused(_: Command) {}
