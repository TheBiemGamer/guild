//! OS-specific calls. Windows only for now.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps a console program from flashing a window.
#[cfg(windows)]
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW)
}

/// Opens a folder in File Explorer, or a store URI (`steam://…`, `com.epicgames.launcher://…`) with its registered
/// handler. explorer.exe hands it to the shell without cmd.exe parsing, so `&` in a URI is safe.
// shortcut: explorer never reports a missing handler (Windows shows its own dialog); use ShellExecuteW if we need the error in-app.
#[cfg(windows)]
pub fn shell_open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), crate::library::Error> {
    std::process::Command::new("explorer.exe")
        .arg(target)
        .spawn()?;
    Ok(())
}

/// Opens an `https://` URL in the default browser. explorer.exe would open the Documents folder
/// for anything but a registered protocol; rundll32 hands the URL straight to the shell, no cmd.exe,
/// so `&` and `?` are safe.
#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), crate::library::Error> {
    if !url.starts_with("https://") {
        return Err(format!("not an https URL: {url}").into());
    }
    no_window(
        std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]),
    )
    .spawn()?;
    Ok(())
}

/// Whether `tasklist /FI <filter>` (e.g. `PID eq 42`) lists a process named `image`.
// shortcut: starts a tasklist process; callers only ask while a game is believed to be running.
#[cfg(windows)]
pub fn process_listed(filter: &str, image: &str) -> bool {
    let output = no_window(
        std::process::Command::new("tasklist").args(["/FI", filter, "/FO", "CSV", "/NH"]),
    )
    .output();
    output.is_ok_and(|o| lists_image(&String::from_utf8_lossy(&o.stdout), image))
}

/// tasklist CSV rows look like `"steam.exe","16176","Console","1","131.968 K"`.
fn lists_image(tasklist_csv: &str, image: &str) -> bool {
    let wanted = format!("\"{}\"", image.to_ascii_lowercase());
    tasklist_csv
        .lines()
        .any(|row| row.to_ascii_lowercase().starts_with(&wanted))
}

/// A DWORD value under `HKEY_CURRENT_USER\<key>`.
#[cfg(windows)]
pub fn user_registry_u32(key: &str, name: &str) -> std::io::Result<u32> {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(key)?
        .get_value(name)
}

/// Kills a process and every process it started (Legendary and gogdl unpack themselves into a child process,
/// and gogdl starts download workers). Both resume a killed download when run again.
#[cfg(windows)]
pub fn kill_tree(pid: u32) -> Result<(), crate::library::Error> {
    let status = no_window(std::process::Command::new("taskkill").args([
        "/PID",
        &pid.to_string(),
        "/T",
        "/F",
    ]))
    .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill exited with {status}").into())
    }
}

/// Starts a store tool (Legendary, gogdl, Nile) in a job that Windows ends with the app, even when the app is killed,
/// so a download never runs on after it. Suspended until it is in the job, so the child a PyInstaller build starts at
/// once lands there too. Never for games or `legendary launch`: a game in the job would close with the app.
#[cfg(windows)]
pub fn spawn_tool(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
    let mut child = command
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .spawn()?;
    // SAFETY: the job handle stays open for the app's life, the process handle for `child`'s.
    if let Some(job) = tool_job()
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0
    {
        tracing::warn!(
            "store tool outside the job: {}",
            std::io::Error::last_os_error()
        );
    }
    if let Err(e) = resume(child.id()) {
        _ = child.kill();
        _ = child.wait();
        return Err(e);
    }
    Ok(child)
}

/// `Command::output` for a store tool, started through `spawn_tool`.
#[cfg(windows)]
pub fn tool_output(command: &mut std::process::Command) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    spawn_tool(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?
    .wait_with_output()
}

/// The job store tools run in, made once. Its handle is never closed: the app's exit closes it, which ends them.
#[cfg(windows)]
fn tool_job() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    // A handle is a pointer, which a static can't hold; kept as its address.
    static JOB: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let job = JOB.get_or_init(|| {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the handle is checked before use; `limits` outlives the call that reads it.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                tracing::warn!("store tool job: {}", std::io::Error::last_os_error());
                return None;
            }
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of_val(&limits) as u32,
            );
            if set == 0 {
                tracing::warn!("store tool job: {}", std::io::Error::last_os_error());
                CloseHandle(job);
                return None;
            }
            Some(job as usize)
        }
    });
    job.map(|j| j as windows_sys::Win32::Foundation::HANDLE)
}

/// Resumes the threads of a process started suspended.
#[cfg(windows)]
fn resume(pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    // SAFETY: every handle is checked before use and closed once; `entry` is a plain struct the calls fill in.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let mut resumed = false;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    resumed |= ResumeThread(thread) != u32::MAX;
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        let error = std::io::Error::last_os_error();
        CloseHandle(snapshot);
        if resumed { Ok(()) } else { Err(error) }
    }
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

/// Windows' display language as a language code, for the languages the app has words for ("nl"); "en" otherwise.
#[cfg(windows)]
pub fn ui_language() -> &'static str {
    // SAFETY: takes nothing and returns a number.
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    // The low 10 bits are the primary language; 0x13 is Dutch (Netherlands and Belgium alike).
    if id & 0x3ff == 0x13 { "nl" } else { "en" }
}

/// The folder a GOG save location's `<?NAME?>` variable stands for; `None` for one it doesn't know. `install` is
/// the game's install folder.
#[cfg(windows)]
pub fn gog_folder(name: &str, install: &std::path::Path) -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(match name {
        "INSTALL" => install.to_path_buf(),
        "SAVED_GAMES" => base.home_dir().join("Saved Games"),
        "APPLICATION_DATA_LOCAL" => base.data_local_dir().to_path_buf(),
        "APPLICATION_DATA_LOCAL_LOW" => base.home_dir().join("AppData").join("LocalLow"),
        "APPLICATION_DATA_ROAMING" => base.data_dir().to_path_buf(),
        "DOCUMENTS" => directories::UserDirs::new()?.document_dir()?.to_path_buf(),
        _ => return None,
    })
}

#[cfg(windows)]
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager's Startup apps switch for a Run entry.
#[cfg(windows)]
const RUN_APPROVED: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Whether the current user's Run entry `name` is on, read as tauri-plugin-autostart reads it: on unless Task
/// Manager's value ends in a nonzero time. `None` when there is no entry.
#[cfg(windows)]
pub fn autostart(name: &str) -> Option<bool> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    hkcu.open_subkey(RUN)
        .and_then(|k| k.get_raw_value(name))
        .ok()?;
    let approved = hkcu
        .open_subkey(RUN_APPROVED)
        .and_then(|k| k.get_raw_value(name));
    Some(approved.map_or(true, |v| {
        v.bytes.len() < 8 || v.bytes.iter().rev().take(8).all(|b| *b == 0)
    }))
}

/// Deletes the current user's Run entry `name` and its Task Manager switch; a missing one is fine.
#[cfg(windows)]
pub fn remove_autostart(name: &str) -> std::io::Result<()> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    for key in [RUN, RUN_APPROVED] {
        match hkcu
            .open_subkey_with_flags(key, winreg::enums::KEY_SET_VALUE)
            .and_then(|k| k.delete_value(name))
        {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_url_refuses_non_https() {
        for url in [
            "http://example.com",
            "steam://open/main",
            "file:///C:/Windows",
            "notepad.exe",
            "",
        ] {
            assert!(open_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn lists_image_matches_whole_name_ignoring_case() {
        let row = "\"Steam.exe\",\"16176\",\"Console\",\"1\",\"131.968 K\"\r\n";
        assert!(lists_image(row, "steam.exe"));
        // No match: tasklist prints an INFO line instead of a row.
        assert!(!lists_image(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            "steam.exe"
        ));
        // Pid reused by another program, or a name that only starts the same.
        assert!(!lists_image(
            "\"notepad.exe\",\"16176\",\"Console\",\"1\",\"9 K\"",
            "steam.exe"
        ));
        assert!(!lists_image(
            "\"steamwebhelper.exe\",\"16176\",\"Console\",\"1\",\"9 K\"",
            "steam.exe"
        ));
    }

    #[test]
    fn a_store_tool_runs_in_the_job_and_to_its_end() {
        use std::os::windows::io::AsRawHandle;
        use std::process::{Command, Stdio};
        let mut command = Command::new("cmd");
        command.args(["/C", "echo hi"]).stdout(Stdio::piped());
        let child = super::spawn_tool(&mut command).unwrap();
        let mut in_job = 0;
        // SAFETY: both handles are open; `in_job` is written by the call.
        let ok = unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                child.as_raw_handle(),
                super::tool_job().unwrap(),
                &mut in_job,
            )
        };
        assert!(ok != 0 && in_job != 0);
        let output = child.wait_with_output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hi");
    }
}
