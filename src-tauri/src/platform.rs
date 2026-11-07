//! OS-specific calls. Windows only for now.

/// Keeps a console program from flashing a window.
#[cfg(windows)]
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(crate::library::CREATE_NO_WINDOW)
}

/// Kills a process and every process it started (Legendary and gogdl unpack themselves into a child process,
/// and gogdl starts download workers). Both resume a killed download when run again.
#[cfg(windows)]
pub fn kill_tree(pid: u32) -> Result<(), crate::library::Error> {
    let status = no_window(std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])).status()?;
    if status.success() { Ok(()) } else { Err(format!("taskkill exited with {status}").into()) }
}

/// Bytes a process has written since it started (`WriteTransferCount`), or `None` when it's gone or not ours to read.
#[cfg(windows)]
pub fn process_bytes_written(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetProcessIoCounters, IO_COUNTERS, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked before use and closed once; `counters` is a plain struct the call fills in.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut counters: IO_COUNTERS = std::mem::zeroed();
        let ok = GetProcessIoCounters(handle, &mut counters);
        CloseHandle(handle);
        (ok != 0).then_some(counters.WriteTransferCount)
    }
}

/// "Windows 11 (build 26200)", from the registry; builds from 22000 on are Windows 11.
#[cfg(windows)]
pub fn windows_version() -> String {
    let build: Option<u32> = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuild"))
        .ok()
        .and_then(|b| b.parse().ok());
    match build {
        Some(b) => format!("Windows {} (build {b})", if b >= 22_000 { 11 } else { 10 }),
        None => "Windows".into(),
    }
}
