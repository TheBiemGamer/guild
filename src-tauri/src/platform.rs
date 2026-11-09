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
    let dot = steam_dot_dir()?;
    let pid = std::fs::read_to_string(dot.join("steam.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if dot.parent()?.ends_with(FLATPAK_HOME) {
        flatpak_host_pid(pid)
    } else {
        Some(pid)
    }
}

/// Flatpak Steam's home, under the user's home.
#[cfg(target_os = "linux")]
const FLATPAK_HOME: &str = ".var/app/com.valvesoftware.Steam";

/// Flatpak Steam writes its pid as seen inside its sandbox; on the host it is the `steam` process whose `NSpid`
/// (its pid in each nested namespace) ends in that pid.
#[cfg(target_os = "linux")]
fn flatpak_host_pid(sandbox_pid: u32) -> Option<u32> {
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid = e.file_name().to_str()?.parse().ok()?;
        let status = std::fs::read_to_string(e.path().join("status")).ok()?;
        (nspid_ends_in(&status, sandbox_pid) && pid_is(pid, "steam")).then_some(pid)
    })
}

/// Whether `/proc/<pid>/status` shows the process in a nested pid namespace with `pid` as its innermost id.
#[cfg(target_os = "linux")]
fn nspid_ends_in(status: &str, pid: u32) -> bool {
    let Some(ids) = status.lines().find_map(|l| l.strip_prefix("NSpid:")) else {
        return false;
    };
    let ids: Vec<&str> = ids.split_whitespace().collect();
    ids.len() > 1 && ids.last() == Some(&pid.to_string().as_str())
}

/// The `.steam` folder (pid, registry.vdf) of the Steam steamlocate finds: inside the Flatpak's home for Flatpak
/// Steam, else in the user's home.
#[cfg(target_os = "linux")]
fn steam_dot_dir() -> Option<std::path::PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let steam = steamlocate::SteamDir::locate().ok()?;
    Some(dot_dir_for(&home, steam.path()))
}

#[cfg(target_os = "linux")]
fn dot_dir_for(home: &std::path::Path, steam: &std::path::Path) -> std::path::PathBuf {
    let flatpak = home.join(FLATPAK_HOME);
    if steam.starts_with(&flatpak) {
        flatpak.join(".steam")
    } else {
        home.join(".steam")
    }
}

/// The app id Steam says runs: 0 when none does, an error when Steam was never installed for this user.
#[cfg(windows)]
pub fn steam_running_app() -> std::io::Result<u32> {
    user_registry_u32(r"Software\Valve\Steam", "RunningAppID")
}

/// The app id Steam says runs, from the registry Steam keeps in `.steam/registry.vdf` on Linux.
#[cfg(target_os = "linux")]
pub fn steam_running_app() -> std::io::Result<u32> {
    let path = steam_dot_dir()
        .ok_or(std::io::ErrorKind::NotFound)?
        .join("registry.vdf");
    registry_vdf_u32(&std::fs::read_to_string(path)?, "RunningAppID")
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
/// The tool gets SIGKILL when Guild exits, as Windows' job ends it. That signal follows the *thread* that started the
/// tool, so tools are started from one thread that lives as long as Guild, never from a pool thread that may exit.
// shortcut: only the tool itself gets the signal, so children it starts may linger after a crash; kill the group if that shows up.
#[cfg(target_os = "linux")]
pub fn spawn_tool(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::{OnceLock, mpsc};
    type Job = (Command, mpsc::Sender<std::io::Result<Child>>);
    static SPAWNER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    let spawner = SPAWNER.get_or_init(|| {
        let (jobs, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("tool-spawner".into())
            .spawn(move || {
                for (mut command, reply) in rx {
                    _ = reply.send(command.spawn());
                }
            })
            .expect("starting the tool-spawner thread");
        jobs
    });
    let guild = std::process::id();
    command.process_group(0);
    // SAFETY: only async-signal-safe calls (prctl, getppid) between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // Guild exited before prctl took effect.
            if libc::getppid() as u32 != guild {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            Ok(())
        });
    }
    let (reply, result) = mpsc::channel();
    let command = std::mem::replace(command, Command::new(""));
    spawner
        .send((command, reply))
        .map_err(|_| std::io::ErrorKind::BrokenPipe)?;
    result.recv().map_err(|_| std::io::ErrorKind::BrokenPipe)?
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

/// Each Proton build found by name, with its folder; none on Windows.
#[cfg(windows)]
pub fn proton_dirs() -> Vec<(String, std::path::PathBuf)> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn proton_dirs() -> Vec<(String, std::path::PathBuf)> {
    crate::wine::protons()
        .into_iter()
        .map(|p| (p.name, p.path))
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

/// Free and total bytes of the drive a folder is on. A folder that doesn't exist yet (an install creates it) is
/// measured at its nearest existing parent. `None` when no part of the path exists or the OS can't tell.
pub fn disk_space(path: &std::path::Path) -> Option<(u64, u64)> {
    let dir = path.ancestors().find(|p| p.is_dir())?;
    space_of(dir)
}

#[cfg(windows)]
fn space_of(dir: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the outputs are plain numbers it fills in.
    let ok =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    (ok != 0).then_some((free, total))
}

#[cfg(target_os = "linux")]
fn space_of(dir: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is NUL-terminated and outlives the call; `stat` is a plain struct the call fills in.
    let stat = unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(path.as_ptr(), &mut stat) == 0).then_some(stat)
    }?;
    let unit = stat.f_frsize as u64;
    Some((stat.f_bavail as u64 * unit, stat.f_blocks as u64 * unit))
}

/// The roots of the fixed drives (`C:\`, `D:\`), for the install panel's folder menu.
#[cfg(windows)]
pub fn drives() -> Vec<std::path::PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    // SAFETY: takes nothing and returns a bit per drive letter.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
            // SAFETY: `wide` is NUL-terminated and outlives the call.
            unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
        })
        .map(std::path::PathBuf::from)
        .collect()
}

/// Where other drives are mounted (`/mnt`, `/media`, `/run/media`), for the install panel's folder menu; the home
/// folder's drive is the default folder's already.
#[cfg(target_os = "linux")]
pub fn drives() -> Vec<std::path::PathBuf> {
    mount_points(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default())
}

#[cfg(target_os = "linux")]
fn mount_points(mounts: &str) -> Vec<std::path::PathBuf> {
    let mut points: Vec<std::path::PathBuf> = mounts
        .lines()
        .filter_map(|l| {
            let mut fields = l.split(' ');
            let device = fields.next()?;
            // Spaces in a mount point are written `\040`.
            let point = fields.next()?.replace("\\040", " ");
            let other = ["/mnt/", "/media/", "/run/media/"]
                .iter()
                .any(|p| point.starts_with(p));
            (device.starts_with("/dev/") && other).then(|| point.into())
        })
        .collect();
    points.sort();
    points.dedup();
    points
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
    fn dot_dir_follows_flatpak_steam() {
        use std::path::Path;
        let home = Path::new("/home/a");
        assert_eq!(
            dot_dir_for(home, Path::new("/home/a/.local/share/Steam")),
            Path::new("/home/a/.steam")
        );
        assert_eq!(
            dot_dir_for(
                home,
                Path::new("/home/a/.var/app/com.valvesoftware.Steam/.local/share/Steam")
            ),
            Path::new("/home/a/.var/app/com.valvesoftware.Steam/.steam")
        );
        assert!(nspid_ends_in("Name:\tsteam\nNSpid:\t4242\t2\n", 2));
        // Not in a nested namespace, or another innermost pid.
        assert!(!nspid_ends_in("NSpid:\t2\n", 2));
        assert!(!nspid_ends_in("NSpid:\t4242\t12\n", 2));
        assert!(!nspid_ends_in("Name:\tsteam\n", 2));
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
    fn other_drives_are_the_mounted_devices_under_mnt_and_media() {
        let mounts = "/dev/nvme0n1p2 / btrfs rw 0 0\n\
            /dev/nvme0n1p2 /home btrfs rw 0 0\n\
            proc /proc proc rw 0 0\n\
            /dev/sda1 /run/media/me/Big\\040Disk ext4 rw 0 0\n\
            /dev/sdb1 /mnt/games ext4 rw 0 0\n\
            tmpfs /mnt/ram tmpfs rw 0 0\n";
        assert_eq!(
            mount_points(mounts),
            [
                std::path::PathBuf::from("/mnt/games"),
                "/run/media/me/Big Disk".into()
            ]
        );
    }

    #[test]
    fn disk_space_measures_a_folder_that_does_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let (free, total) = disk_space(&dir.path().join("Games/Not yet")).unwrap();
        assert!(total > 0 && free <= total);
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_store_tool_outlives_the_thread_that_asked_for_it() {
        use std::process::{Command, Stdio};
        let mut child = std::thread::spawn(|| {
            let mut command = Command::new("sleep");
            command.arg("30").stdout(Stdio::null());
            super::spawn_tool(&mut command).unwrap()
        })
        .join()
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            child.try_wait().unwrap().is_none(),
            "the tool died with the thread"
        );
        super::kill_tree(child.id()).unwrap();
        _ = child.wait();
    }
}
