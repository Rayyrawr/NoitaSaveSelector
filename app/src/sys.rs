//! Processes, Steam and launching.

use anyhow::{Context, Result, bail};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use sysinfo::{ProcessesToUpdate, System};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Processes that must not run while saves are swapped.
pub const GAME_PROCESSES: [&str; 4] = ["noita.exe", "noita_dev.exe", "noita_proxy.exe", "noita together.exe"];

pub struct Procs {
    sys: System,
}

impl Procs {
    pub fn new() -> Self {
        Procs { sys: System::new() }
    }

    /// Lower-case names of all running processes.
    pub fn refresh(&mut self) -> Vec<String> {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.sys.processes().values().map(|p| p.name().to_string_lossy().to_lowercase()).collect()
    }
}

/// Stop a running EW proxy (needed for the windowless CLI mode). Returns how many were stopped.
pub fn kill_proxy() -> usize {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.processes().values().filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case("noita_proxy.exe")).filter(|p| p.kill()).count()
}

pub fn running_games(names: &[String]) -> Vec<String> {
    let mut v: Vec<String> = GAME_PROCESSES.iter().filter(|g| names.iter().any(|n| n == *g)).map(|s| s.to_string()).collect();
    v.dedup();
    v
}

pub fn steam_running(names: &[String]) -> bool {
    names.iter().any(|n| n == "steam.exe")
}

/// Steam Cloud for Noita (app 881100) per Steam account: Some(true) = on, Some(false) = off, None = unknown.
pub fn noita_cloud_enabled(steam_dir: &str) -> Option<bool> {
    let userdata = Path::new(steam_dir).join("userdata");
    let mut any = None;
    for acc in std::fs::read_dir(userdata).ok()?.flatten() {
        // Only accounts that have Noita data matter.
        if !acc.path().join("881100").exists() {
            continue;
        }
        let cfg = std::fs::read_to_string(acc.path().join(r"7\remote\sharedconfig.vdf")).unwrap_or_default();
        let enabled = match cfg.find("\"881100\"") {
            Some(i) => {
                let block = &cfg[i..];
                let end = block.find('}').unwrap_or(block.len());
                !block[..end].replace(['\t', ' '], "").contains("\"cloudenabled\"\"0\"")
            }
            None => true, // Steam default
        };
        if enabled {
            return Some(true);
        }
        any = Some(false);
    }
    any
}

fn spawn(exe: &str, args: &[&str]) -> Result<()> {
    spawn_env(exe, args, &[])
}

fn spawn_env(exe: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<()> {
    let path = Path::new(exe);
    if !path.exists() {
        bail!("Not found: {exe}\n(adjust the path in Settings)");
    }
    let mut cmd = Command::new(path);
    cmd.args(args);
    cmd.envs(envs.iter().copied());
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn().with_context(|| format!("Failed to start: {exe}"))?;
    Ok(())
}

pub fn launch(exe: &str) -> Result<()> {
    spawn(exe, &[])
}

/// Local address the EW mod and proxy use to talk to each other in CLI mode (their default).
pub const EW_LOCAL_ADDR: &str = "127.0.0.1:21251";

pub fn launch_with(exe: &str, args: &[String]) -> Result<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    spawn(exe, &a)
}

/// Start with NP_NOITA_ADDR pinned, so a headless proxy and a separately started Noita find each other.
pub fn launch_ew_pinned(exe: &str, args: &[String]) -> Result<()> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    spawn_env(exe, &a, &[("NP_NOITA_ADDR", EW_LOCAL_ADDR)])
}

/// This machine's Tailscale IPv4 (100.x.y.z), if Tailscale is installed and up.
pub fn tailscale_ip() -> Option<String> {
    let exe = Path::new(r"C:\Program Files\Tailscale\tailscale.exe");
    let out = Command::new(if exe.exists() { exe } else { Path::new("tailscale") })
        .args(["ip", "-4"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && ip.starts_with("100.")).then_some(ip)
}

pub fn steam_exe(steam_dir: &str) -> String {
    Path::new(steam_dir).join("steam.exe").to_string_lossy().into_owned()
}

pub fn shutdown_steam(steam_dir: &str) -> Result<()> {
    spawn(&steam_exe(steam_dir), &["-shutdown"])
}

pub fn start_steam(steam_dir: &str) -> Result<()> {
    spawn(&steam_exe(steam_dir), &["-silent"])
}

pub fn open_folder(path: &Path) {
    let _ = Command::new("explorer").arg(path).creation_flags(CREATE_NO_WINDOW).spawn();
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Claim the per-session mutex (the installer checks the same name). The handle is kept for the
/// lifetime of the process. `wait`: after a self-update the old instance may still be closing.
static INSTANCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn single_instance(wait: bool) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    // NSS_INSTANCE gives test runs their own mutex (like the other NSS_* overrides).
    let name = wide(&format!("NoitaSaveSelectorMutex{}", std::env::var("NSS_INSTANCE").unwrap_or_default()));
    for _ in 0..if wait { 60 } else { 1 } {
        // SAFETY: plain Win32 calls with a valid, NUL-terminated name.
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS {
                INSTANCE.store(h as usize, std::sync::atomic::Ordering::SeqCst);
                return true;
            }
            if !h.is_null() {
                CloseHandle(h);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

pub fn message_box(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
    let (t, m) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// Let go of the instance mutex, so the uninstaller does not wait for this (closing) process.
fn release_instance() {
    use windows_sys::Win32::Foundation::CloseHandle;
    let h = INSTANCE.swap(0, std::sync::atomic::Ordering::SeqCst);
    if h != 0 {
        // SAFETY: the handle came from CreateMutexW and is closed exactly once.
        unsafe {
            CloseHandle(h as _);
        }
    }
}

/// Inno Setup's uninstaller next to the exe; None for a portable copy.
pub fn uninstaller() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("unins000.exe")).filter(|p| p.exists())
}

/// Start removing the app. The caller exits right after.
pub fn start_uninstall() -> Result<()> {
    let exe = std::env::current_exe()?;
    release_instance();
    match uninstaller() {
        // Our own dialog already asked, so no second confirmation.
        Some(u) => {
            Command::new(u).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]).spawn().context("Could not start the uninstaller")?;
        }
        // Portable: delete the exe once this process is gone.
        None => {
            Command::new("cmd")
                .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & del /f /q \"{}\"", exe.display()))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .context("Could not schedule deleting the exe")?;
        }
    }
    Ok(())
}
