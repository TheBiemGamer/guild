//! OS-specific calls, for Windows and Linux. Linux runs the store tools' Linux builds, and Windows games through
//! umu-launcher and Proton (`wine`).

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps a console program from flashing a window.
#[cfg(windows)]
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW)
}

/// Linux opens no console window, so there is nothing to keep from flashing.
#[cfg(target_os = "linux")]
pub fn no_window(command: &mut std::process::Command) -> &mut std::process::Command {
    command
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

/// Opens a folder, URL or store URI with its handler through `xdg-open`, which gets the target as one argument, no
/// shell, so `&` in a URI is safe.
// shortcut: like explorer, xdg-open's failure to find a handler isn't reported in-app.
#[cfg(target_os = "linux")]
pub fn shell_open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), crate::library::Error> {
    use std::process::Stdio;
    let mut child = std::process::Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reaped off-thread so it never lingers as a zombie.
    std::thread::spawn(move || child.wait());
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

/// Opens an `https://` URL in the default browser.
#[cfg(target_os = "linux")]
pub fn open_url(url: &str) -> Result<(), crate::library::Error> {
    if !url.starts_with("https://") {
        return Err(format!("not an https URL: {url}").into());
    }
    shell_open(url)
}

/// Whether process `pid` runs and is named `image` (the name guards against a reused pid).
#[cfg(windows)]
pub fn pid_is(pid: u32, image: &str) -> bool {
    process_listed(&format!("PID eq {pid}"), image)
}

/// Whether any process named `image` runs.
#[cfg(windows)]
pub fn image_running(image: &str) -> bool {
    process_listed(&format!("IMAGENAME eq {image}"), image)
}

/// Whether `tasklist /FI <filter>` (e.g. `PID eq 42`) lists a process named `image`.
// shortcut: starts a tasklist process; callers only ask while a game is believed to be running.
#[cfg(windows)]
fn process_listed(filter: &str, image: &str) -> bool {
    let output = no_window(
        std::process::Command::new("tasklist").args(["/FI", filter, "/FO", "CSV", "/NH"]),
    )
    .output();
    output.is_ok_and(|o| lists_image(&String::from_utf8_lossy(&o.stdout), image))
}

/// tasklist CSV rows look like `"steam.exe","16176","Console","1","131.968 K"`.
#[cfg(windows)]
fn lists_image(tasklist_csv: &str, image: &str) -> bool {
    let wanted = format!("\"{}\"", image.to_ascii_lowercase());
    tasklist_csv
        .lines()
        .any(|row| row.to_ascii_lowercase().starts_with(&wanted))
}

/// Whether process `pid` runs and is named `image` (the name guards against a reused pid).
#[cfg(target_os = "linux")]
pub fn pid_is(pid: u32, image: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm_is(&comm, image))
}

/// Whether any process named `image` runs.
#[cfg(target_os = "linux")]
pub fn image_running(image: &str) -> bool {
    std::fs::read_dir("/proc").is_ok_and(|dir| {
        dir.flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .any(|e| {
                std::fs::read_to_string(e.path().join("comm"))
                    .is_ok_and(|comm| comm_is(&comm, image))
            })
    })
}

/// `/proc/<pid>/comm` holds a process name cut to its first 15 bytes, ending in a newline.
#[cfg(target_os = "linux")]
fn comm_is(comm: &str, image: &str) -> bool {
    let image = &image.as_bytes()[..image.len().min(15)];
    comm.trim_end_matches('\n')
        .as_bytes()
        .eq_ignore_ascii_case(image)
}

/// A DWORD value under `HKEY_CURRENT_USER\<key>`.
#[cfg(windows)]
fn user_registry_u32(key: &str, name: &str) -> std::io::Result<u32> {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(key)?
        .get_value(name)
}

/// The Steam client's pid, as Steam last wrote it.
#[cfg(windows)]
pub fn steam_pid() -> Option<u32> {
    user_registry_u32(r"Software\Valve\Steam\ActiveProcess", "pid").ok()
}

/// The Steam client's pid, as Steam last wrote it.
#[cfg(target_os = "linux")]
pub fn steam_pid() -> Option<u32> {
    let home = directories::BaseDirs::new()?
        .home_dir()
        .join(".steam/steam.pid");
    std::fs::read_to_string(home).ok()?.trim().parse().ok()
}

/// The app id Steam says runs: 0 when none does, an error when Steam was never installed for this user.
#[cfg(windows)]
pub fn steam_running_app() -> std::io::Result<u32> {
    user_registry_u32(r"Software\Valve\Steam", "RunningAppID")
}

/// The app id Steam says runs, from the registry Steam keeps in `~/.steam/registry.vdf` on Linux.
// shortcut: Flatpak Steam keeps it under ~/.var/app/com.valvesoftware.Steam; read there too if owners use it.
#[cfg(target_os = "linux")]
pub fn steam_running_app() -> std::io::Result<u32> {
    let home = directories::BaseDirs::new()
        .ok_or(std::io::ErrorKind::NotFound)?
        .home_dir()
        .join(".steam/registry.vdf");
    registry_vdf_u32(&std::fs::read_to_string(home)?, "RunningAppID")
        .ok_or_else(|| std::io::ErrorKind::NotFound.into())
}

/// A number under `Registry/HKCU/Software/Valve/Steam` in Steam's registry.vdf.
#[cfg(target_os = "linux")]
fn registry_vdf_u32(text: &str, name: &str) -> Option<u32> {
    use crate::library::steam::child;
    let vdf = keyvalues_parser::Vdf::from(keyvalues_parser::parse(text).ok()?);
    ["HKCU", "Software", "Valve", "Steam", name]
        .iter()
        .try_fold(&vdf.value, |v, k| child(v, k))?
        .get_str()?
        .parse()
        .ok()
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

/// Kills a store tool and every process it started: `spawn_tool` makes it lead its own process group.
#[cfg(target_os = "linux")]
pub fn kill_tree(pid: u32) -> Result<(), crate::library::Error> {
    let status = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill exited with {status}").into())
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

/// Starts a store tool (Legendary, gogdl, Nile) as the leader of its own process group, so `kill_tree` ends it with
/// the children it starts. Never for games or `legendary launch`.
// shortcut: a tool outlives a Guild that exits mid-download (Windows' job ends it); PR_SET_PDEATHSIG needs libc.
#[cfg(target_os = "linux")]
pub fn spawn_tool(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    command.process_group(0).spawn()
}

/// `Command::output` for a store tool, started through `spawn_tool`.
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

/// Bytes a process has made the disk write (`write_bytes` in `/proc/<pid>/io`), or `None` when it's gone or not
/// ours to read.
#[cfg(target_os = "linux")]
pub fn process_bytes_written(pid: u32) -> Option<u64> {
    std::fs::read_to_string(format!("/proc/{pid}/io"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("write_bytes:"))?
        .trim()
        .parse()
        .ok()
}

/// Windows' display language as a language code, for the languages the app has words for ("nl"); "en" otherwise.
#[cfg(windows)]
pub fn ui_language() -> &'static str {
    // SAFETY: takes nothing and returns a number.
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    // The low 10 bits are the primary language; 0x13 is Dutch (Netherlands and Belgium alike).
    if id & 0x3ff == 0x13 { "nl" } else { "en" }
}

/// The locale's language (`LC_ALL`, else `LC_MESSAGES`, else `LANG`) for the languages the app has words for ("nl");
/// "en" otherwise.
#[cfg(target_os = "linux")]
pub fn ui_language() -> &'static str {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    if locale.starts_with("nl") { "nl" } else { "en" }
}

/// The folder a GOG save location's `<?NAME?>` variable stands for; `None` for one it doesn't know. `install` is
/// the game's install folder; `key` its copy key, for the Wine prefix on Linux.
#[cfg(windows)]
pub fn gog_folder(name: &str, install: &std::path::Path, _key: &str) -> Option<std::path::PathBuf> {
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

/// The folder a GOG save location's `<?NAME?>` variable stands for: a Windows game keeps its saves in its Wine prefix.
#[cfg(target_os = "linux")]
pub fn gog_folder(name: &str, install: &std::path::Path, key: &str) -> Option<std::path::PathBuf> {
    match name {
        "INSTALL" => Some(install.to_path_buf()),
        _ => crate::wine::windows_folder(&crate::wine::prefix(key)?, name),
    }
}

/// Windows games on Linux, for Settings: whether umu-launcher and winetricks are found, the Proton builds found
/// (newest first) and the one chosen in Settings (none is the newest).
#[derive(serde::Serialize, specta::Type)]
#[cfg_attr(windows, allow(dead_code))]
pub struct RunnerView {
    pub umu: bool,
    pub protons: Vec<ProtonView>,
    pub proton: Option<String>,
    pub winetricks: bool,
}

/// A Proton build found; only Guild's own may be removed.
#[derive(serde::Serialize, specta::Type)]
#[cfg_attr(windows, allow(dead_code))]
pub struct ProtonView {
    pub name: String,
    pub removable: bool,
}

/// Where Settings installs Proton builds from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ProtonKind {
    GeProton,
    ProtonCachyos,
}

/// A release Settings can install: its tag, the build's name and whether that build is installed.
#[derive(serde::Serialize, specta::Type)]
#[cfg_attr(windows, allow(dead_code))]
pub struct ReleaseView {
    pub tag: String,
    pub name: String,
    pub installed: bool,
}

/// Windows has no runner to set up.
#[cfg(windows)]
pub fn runner() -> Option<RunnerView> {
    None
}

#[cfg(target_os = "linux")]
pub fn runner() -> Option<RunnerView> {
    Some(crate::wine::view())
}

/// The Proton builds a game may pick, by name; none on Windows.
#[cfg(windows)]
pub fn protons() -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn protons() -> Vec<String> {
    crate::wine::proton_names()
}

/// A Windows game's program as it runs: as is on Windows. `key` is its copy key, `proton` its own Proton choice.
#[cfg(windows)]
pub fn windows_game(
    game: std::process::Command,
    _key: &str,
    _proton: Option<&str>,
) -> Result<std::process::Command, crate::msg::Msg> {
    Ok(game)
}

/// A Windows game's program as it runs: through umu-launcher and Proton, in the copy's own Wine prefix.
#[cfg(target_os = "linux")]
pub fn windows_game(
    game: std::process::Command,
    key: &str,
    proton: Option<&str>,
) -> Result<std::process::Command, crate::msg::Msg> {
    crate::wine::game_command(game, key, proton)
}

/// Whether a launched game still waits on its first-run setup; never on Windows.
#[cfg(windows)]
pub fn setting_up(_key: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn setting_up(key: &str) -> bool {
    crate::wine::setting_up(key)
}

/// Points a store tool at a Windows game's Wine prefix (`WINEPREFIX`), where its saves are, setting the prefix up
/// first; nothing on Windows.
#[cfg(windows)]
pub fn in_prefix(_command: &mut std::process::Command, _key: &str) {}

#[cfg(target_os = "linux")]
pub fn in_prefix(command: &mut std::process::Command, key: &str) {
    crate::wine::ready_prefix(key);
    if let Some(prefix) = crate::wine::prefix(key) {
        command.env("WINEPREFIX", prefix);
    }
}

/// Downloads umu-launcher; Windows needs none.
#[cfg(windows)]
pub async fn install_umu(_on_percent: impl FnMut(u8)) -> Result<(), crate::msg::Msg> {
    Err(msg!(unknownTool))
}

#[cfg(target_os = "linux")]
pub async fn install_umu(on_percent: impl FnMut(u8)) -> Result<(), crate::msg::Msg> {
    crate::wine::install_umu(on_percent).await
}

/// A Proton source's recent releases; `None` when they couldn't be fetched, and on Windows. Blocking.
#[cfg(windows)]
pub fn proton_releases(_kind: ProtonKind) -> Option<Vec<ReleaseView>> {
    None
}

#[cfg(target_os = "linux")]
pub fn proton_releases(kind: ProtonKind) -> Option<Vec<ReleaseView>> {
    crate::wine::release_views(kind)
}

/// Installs one Proton release; Windows needs none.
#[cfg(windows)]
pub async fn install_proton(
    _kind: ProtonKind,
    _tag: String,
    _on_percent: impl FnMut(u8),
) -> Result<(), crate::msg::Msg> {
    Err(msg!(unknownTool))
}

#[cfg(target_os = "linux")]
pub async fn install_proton(
    kind: ProtonKind,
    tag: String,
    on_percent: impl FnMut(u8),
) -> Result<(), crate::msg::Msg> {
    crate::wine::install_proton(kind, tag, on_percent).await
}

/// Removes a Proton build Guild installed; Windows has none.
#[cfg(windows)]
pub fn remove_proton(_name: &str) -> Result<(), crate::msg::Msg> {
    Err(msg!(protonNotRemovable))
}

#[cfg(target_os = "linux")]
pub fn remove_proton(name: &str) -> Result<(), crate::msg::Msg> {
    crate::wine::remove_proton(name)
}

/// The copy keys with a Wine prefix and each prefix's folder; none on Windows.
#[cfg(windows)]
pub fn prefixes() -> Vec<(String, std::path::PathBuf)> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn prefixes() -> Vec<(String, std::path::PathBuf)> {
    crate::wine::prefix_keys()
        .into_iter()
        .filter_map(|k| crate::wine::prefix(&k).map(|p| (k, p)))
        .collect()
}

/// Deletes a copy's Wine prefix; Windows has none.
#[cfg(windows)]
pub fn remove_prefix(_key: &str) -> Result<(), crate::msg::Msg> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn remove_prefix(key: &str) -> Result<(), crate::msg::Msg> {
    crate::wine::remove_prefix(key)
}

/// Starts winecfg or winetricks (`program`) in a copy's prefix with its Proton build; Windows has none.
#[cfg(windows)]
pub fn run_in_prefix(
    _key: &str,
    _program: &str,
    _proton: Option<&str>,
) -> Result<(), crate::msg::Msg> {
    Err(msg!(unknownTool))
}

#[cfg(target_os = "linux")]
pub fn run_in_prefix(
    key: &str,
    program: &str,
    proton: Option<&str>,
) -> Result<(), crate::msg::Msg> {
    crate::wine::run_in_prefix(key, program, proton)
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

/// Linux has no Run entries; tauri-plugin-autostart keeps its own desktop file.
#[cfg(target_os = "linux")]
pub fn autostart(_name: &str) -> Option<bool> {
    None
}

#[cfg(target_os = "linux")]
pub fn remove_autostart(_name: &str) -> std::io::Result<()> {
    Ok(())
}

/// "Windows 11 (build 26200)", from the registry; builds from 22000 on are Windows 11.
#[cfg(windows)]
pub fn os_version() -> String {
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

/// The distribution's name, like "CachyOS Linux" (`PRETTY_NAME` in `/etc/os-release`).
#[cfg(target_os = "linux")]
pub fn os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| pretty_name(&text))
        .unwrap_or_else(|| "Linux".into())
}

#[cfg(target_os = "linux")]
fn pretty_name(os_release: &str) -> Option<String> {
    let value = os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))?
        .trim()
        .trim_matches(['"', '\'']);
    (!value.is_empty()).then(|| value.to_owned())
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

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn comm_matches_the_kernels_cut_name_ignoring_case() {
        assert!(comm_is("steam\n", "steam"));
        assert!(!comm_is("steamwebhelper\n", "steam"));
        assert!(comm_is("Prison Architec\n", "Prison Architect64.exe"));
        assert!(pid_is(
            std::process::id(),
            std::fs::read_to_string("/proc/self/comm")
                .unwrap()
                .trim_end()
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registry_vdf_and_os_release_parse() {
        let vdf = r#""Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "RunningAppID" "620" } } } } }"#;
        assert_eq!(registry_vdf_u32(vdf, "RunningAppID"), Some(620));
        assert_eq!(registry_vdf_u32(vdf, "Missing"), None);
        assert_eq!(
            pretty_name("NAME=Arch\nPRETTY_NAME=\"CachyOS Linux\"\n").as_deref(),
            Some("CachyOS Linux")
        );
        assert_eq!(pretty_name("NAME=Arch\n"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_store_tool_and_its_children_die_together() {
        use std::process::{Command, Stdio};
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & wait"])
            .stdout(Stdio::null());
        let mut child = super::spawn_tool(&mut command).unwrap();
        super::kill_tree(child.id()).unwrap();
        assert!(!child.wait().unwrap().success());
    }
}
