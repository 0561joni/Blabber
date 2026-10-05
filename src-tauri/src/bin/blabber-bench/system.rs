//! Machine facts and process-tree memory sampling (macOS).
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub cpu: String,
    pub memory_bytes: u64,
    pub cores: u32,
    pub os: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Power {
    pub source: Option<String>,
    pub low_power_mode: Option<bool>,
    #[serde(default)]
    pub adapter_watts: Option<u32>,
    #[serde(default)]
    pub battery_percent: Option<u32>,
}

/// An M-series Pro/Max chip draws 30–60 W under ASR load. A weaker charger
/// drains the battery and macOS throttles once it runs low, so timings drift.
pub const MIN_ADAPTER_WATTS: u32 = 30;
pub const MIN_BATTERY_PERCENT: u32 = 20;

impl Power {
    /// Why timings taken now would not be trustworthy, if they would not.
    pub fn problem(&self) -> Option<String> {
        if self.source.as_deref().is_some_and(|s| s.contains("Battery")) {
            return Some("the Mac is running on battery".into());
        }
        if self.low_power_mode == Some(true) {
            return Some("Low Power Mode is on".into());
        }
        if let Some(watts) = self.adapter_watts.filter(|&w| w < MIN_ADAPTER_WATTS) {
            return Some(format!(
                "the power adapter delivers only {watts} W (at least {MIN_ADAPTER_WATTS} W needed; use the Mac's own charger)"
            ));
        }
        if let Some(percent) = self.battery_percent.filter(|&p| p < MIN_BATTERY_PERCENT) {
            return Some(format!("the battery is at {percent} %, where macOS throttles; let it charge first"));
        }
        None
    }
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn machine() -> Machine {
    let os = match (
        output("sw_vers", &["-productVersion"]),
        output("sw_vers", &["-buildVersion"]),
    ) {
        (Some(version), Some(build)) => format!("macOS {version} ({build})"),
        _ => std::env::consts::OS.to_string(),
    };
    Machine {
        cpu: output("sysctl", &["-n", "machdep.cpu.brand_string"])
            .unwrap_or_else(|| std::env::consts::ARCH.into()),
        memory_bytes: output("sysctl", &["-n", "hw.memsize"])
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        cores: output("sysctl", &["-n", "hw.ncpu"])
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        os,
    }
}

pub fn power() -> Power {
    let source = output("pmset", &["-g", "ps"]).and_then(|text| {
        let start = text.find('\'')? + 1;
        let end = start + text[start..].find('\'')?;
        Some(text[start..end].to_string())
    });
    let low_power_mode = output("pmset", &["-g"]).and_then(|text| {
        text.lines()
            .find(|line| line.trim_start().starts_with("lowpowermode"))
            .map(|line| line.trim().ends_with('1'))
    });
    let battery = output("pmset", &["-g", "batt"]);
    let battery_percent = battery.as_deref().and_then(|text| {
        let end = text.find('%')?;
        let start = text[..end].rfind(|c: char| !c.is_ascii_digit()).map(|i| i + 1).unwrap_or(0);
        text[start..end].parse().ok()
    });
    let adapter_watts = output("pmset", &["-g", "ac"]).and_then(|text| {
        let line = text.lines().find(|line| line.trim_start().starts_with("Wattage"))?;
        line.split('=').nth(1)?.trim().trim_end_matches('W').trim().parse().ok()
    });
    Power {
        source,
        low_power_mode,
        adapter_watts,
        battery_percent,
    }
}

/// "nominal", or a short description of the thermal or performance limit.
pub fn thermal() -> Option<String> {
    let text = output("pmset", &["-g", "therm"])?;
    let limits: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let value: i64 = value.trim().parse().ok()?;
            let limited = if key.ends_with("Speed_Limit") {
                value < 100
            } else {
                value > 0
            };
            limited.then(|| format!("{key}={value}"))
        })
        .collect();
    let warning = text
        .lines()
        .any(|line| line.contains("warning level") && !line.contains("No "));
    Some(if limits.is_empty() && !warning {
        "nominal".into()
    } else if limits.is_empty() {
        "throttled: thermal warning".into()
    } else {
        format!("throttled: {}", limits.join(", "))
    })
}

pub fn git_state(repo: &std::path::Path) -> (Option<String>, Option<bool>) {
    let dir = repo.to_string_lossy().to_string();
    let commit = output("git", &["-C", &dir, "rev-parse", "--short", "HEAD"]);
    let dirty = output("git", &["-C", &dir, "status", "--porcelain"]).map(|text| !text.is_empty());
    (commit, dirty)
}

/// Blabber instances that could compete for memory, the GPU or the Qwen lock.
pub fn running_app_processes() -> Vec<String> {
    let mut found = Vec::new();
    for name in ["Blabber", "speech-to-text"] {
        if let Some(pids) = output("pgrep", &["-x", name]) {
            if !pids.is_empty() {
                found.push(format!("{name} (pid {})", pids.replace('\n', ", ")));
            }
        }
    }
    found
}

#[cfg(target_os = "macos")]
fn children(pid: i32) -> Vec<i32> {
    let mut buffer = vec![0 as libc::pid_t; 256];
    let bytes = unsafe {
        libc::proc_listchildpids(
            pid,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * std::mem::size_of::<libc::pid_t>()) as i32,
        )
    };
    if bytes <= 0 {
        return Vec::new();
    }
    // The return value is a count of PIDs on current macOS versions; older
    // versions returned bytes. Both fit the buffer; zero entries are unused.
    let count = (bytes as usize).min(buffer.len());
    buffer.truncate(count);
    buffer.retain(|&child| child > 0);
    buffer
}
#[cfg(not(target_os = "macos"))]
fn children(_: i32) -> Vec<i32> {
    Vec::new()
}

/// `pid` and all of its descendants.
pub fn process_tree(pid: i32) -> Vec<i32> {
    let mut all = vec![pid];
    let mut index = 0;
    while index < all.len() && all.len() < 512 {
        for child in children(all[index]) {
            if !all.contains(&child) {
                all.push(child);
            }
        }
        index += 1;
    }
    all
}

#[cfg(target_os = "macos")]
fn footprint(pid: i32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        )
    };
    (status == 0).then_some(info.ri_phys_footprint)
}
#[cfg(not(target_os = "macos"))]
fn footprint(_: i32) -> Option<u64> {
    None
}

/// Physical footprint of a process tree.
pub fn tree_footprint(pid: i32) -> u64 {
    process_tree(pid).into_iter().filter_map(footprint).sum()
}

/// System-wide wired memory. The Neural Engine's model memory is wired and is
/// not part of any process footprint.
#[cfg(target_os = "macos")]
pub fn wired_bytes() -> Option<u64> {
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let status = unsafe {
        #[allow(deprecated)]
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    #[allow(deprecated)]
    let page = unsafe { libc::vm_page_size } as u64;
    (status == 0).then(|| stats.wire_count as u64 * page)
}
#[cfg(not(target_os = "macos"))]
pub fn wired_bytes() -> Option<u64> {
    None
}

/// Samples a process tree's peak footprint and the peak rise of wired memory
/// until dropped.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    wired_peak: Arc<AtomicU64>,
    wired_baseline: Option<u64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    pub fn start(pid: i32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let wired_baseline = wired_bytes();
        let wired_peak = Arc::new(AtomicU64::new(wired_baseline.unwrap_or(0)));
        let thread = {
            let (stop, peak, wired_peak) = (stop.clone(), peak.clone(), wired_peak.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    peak.fetch_max(tree_footprint(pid), Ordering::Relaxed);
                    if let Some(wired) = wired_bytes() {
                        wired_peak.fetch_max(wired, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
        };
        Self {
            stop,
            peak,
            wired_peak,
            wired_baseline,
            thread: Some(thread),
        }
    }

    pub fn peak_bytes(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    pub fn wired_delta_bytes(&self) -> Option<u64> {
        self.wired_baseline
            .map(|baseline| self.wired_peak.load(Ordering::Relaxed).saturating_sub(baseline))
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Kills a process and every descendant (helpers live in their own process
/// groups, so killing the child alone would orphan them).
pub fn kill_tree(pid: i32) {
    let tree = process_tree(pid);
    for pid in tree.iter().rev() {
        unsafe {
            libc::kill(*pid, libc::SIGKILL);
        }
    }
}
