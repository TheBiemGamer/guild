//! The store tools (Legendary, gogdl, Nile), installed on request into `<local data>/tools/<name>/`.
//! Each is one pinned GitHub release asset; a download whose SHA-256 differs from the compiled-in hash is
//! deleted before anything runs it. Bump a tool by changing its version, URL and hash together, for each OS.
//! The Linux builds of Legendary and gogdl are Python zipapps that run on the system's `python3`.

use fs_err::{self as fs, File};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::msg::Msg;

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub version: &'static str,
    url: &'static str,
    /// Lowercase hex SHA-256 of the asset at `url`.
    sha256: &'static str,
    exe: &'static str,
    /// A folder under `%LOCALAPPDATA%` where the owner may have put it by hand.
    local: Option<&'static str>,
}

/// derrod/legendary, the build the Epic parsers were written against.
pub const LEGENDARY: Tool = Tool {
    name: "legendary",
    title: "Legendary",
    version: "0.21.1",
    #[cfg(windows)]
    url: "https://github.com/derrod/legendary/releases/download/0.21.1/legendary_windows_x64.exe",
    #[cfg(windows)]
    sha256: "6be77857dc0a6dd33ea7442ce8b5291944ec6da53757f81ca95b57ce794baf5b",
    #[cfg(windows)]
    exe: "legendary.exe",
    #[cfg(windows)]
    local: Some(r"Programs\legendary"),
    #[cfg(target_os = "linux")]
    url: "https://github.com/derrod/legendary/releases/download/0.21.1/legendary_linux_x64",
    #[cfg(target_os = "linux")]
    sha256: "dbed33bbe96031e65858233e4badf0a1d401bb6cb0cc995d1237cf3a0c826a45",
    #[cfg(target_os = "linux")]
    exe: "legendary",
    #[cfg(target_os = "linux")]
    local: None,
};

pub const GOGDL: Tool = Tool {
    name: "gogdl",
    title: "gogdl",
    version: "1.4.0",
    #[cfg(windows)]
    url: "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v1.4.0/gogdl_windows_x86_64.exe",
    #[cfg(windows)]
    sha256: "291b653e6dbf7a394a3342ed0ebcea2cbc83e5f66d030777d65ccdd60d18ab02",
    #[cfg(windows)]
    exe: "gogdl.exe",
    #[cfg(target_os = "linux")]
    url: "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v1.4.0/gogdl_linux_x86_64",
    #[cfg(target_os = "linux")]
    sha256: "2aa2b59c83d3bfbe7b60ead91c6297b263b5d1e6c233a04b7409bafe26738f02",
    #[cfg(target_os = "linux")]
    exe: "gogdl",
    local: None,
};

pub const NILE: Tool = Tool {
    name: "nile",
    title: "Nile",
    version: "1.2.0",
    #[cfg(windows)]
    url: "https://github.com/imLinguin/nile/releases/download/v1.2.0/nile_windows_x86_64.exe",
    #[cfg(windows)]
    sha256: "6531790c59f78cea4a8743bf0582d5afda7fb887f5c143391d7339ad0f42ab88",
    #[cfg(windows)]
    exe: "nile.exe",
    #[cfg(target_os = "linux")]
    url: "https://github.com/imLinguin/nile/releases/download/v1.2.0/nile_linux_x86_64",
    #[cfg(target_os = "linux")]
    sha256: "d73519514df9d52c50c6feec629373cc3a0d9504e087b6c7372a8ca8a95e7bfe",
    #[cfg(target_os = "linux")]
    exe: "nile",
    local: None,
};

pub const ALL: [&Tool; 3] = [&LEGENDARY, &GOGDL, &NILE];

/// The whole download, not each read: a 17 MB file on a slow line takes minutes.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub fn find(name: &str) -> Option<&'static Tool> {
    ALL.into_iter().find(|t| t.name == name)
}

fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| crate::cache::Dirs::new().tools)
}

fn local_data() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf())
}

impl Tool {
    /// Where `install` puts it under `root`.
    fn managed(&self, root: &Path) -> PathBuf {
        root.join(self.name).join(self.exe)
    }

    /// The program to run: ours, else the owner's in `%LOCALAPPDATA%`, else the bare name, found on PATH.
    pub fn exe(&self) -> PathBuf {
        self.locate(root(), local_data().as_deref())
    }

    fn locate(&self, root: &Path, local_data: Option<&Path>) -> PathBuf {
        let managed = self.managed(root);
        let local = self
            .local
            .zip(local_data)
            .map(|(dir, base)| base.join(dir).join(self.exe));
        [Some(managed), local]
            .into_iter()
            .flatten()
            .find(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from(self.name))
    }
}

static VERSIONS: Mutex<Option<HashMap<&'static str, Option<String>>>> = Mutex::new(None);

/// The version `--version` reports, or `None` when the tool isn't found. Asked once per app run and again
/// after an install.
pub fn version(tool: &'static Tool) -> Option<String> {
    let mut versions = VERSIONS.lock().unwrap_or_else(|e| e.into_inner());
    versions
        .get_or_insert_default()
        .entry(tool.name)
        .or_insert_with(|| {
            let output =
                crate::platform::tool_output(Command::new(tool.exe()).arg("--version")).ok()?;

            parse_version(&String::from_utf8_lossy(&output.stdout))
        })
        .clone()
}

/// `legendary version "0.21.1", codename "Lowlife"`, `1.4.0` (gogdl), `1.2.0 Robert Speedwagon` (Nile).
fn parse_version(text: &str) -> Option<String> {
    let version = if text.contains('"') {
        text.split('"').nth(1)?
    } else {
        text.split_whitespace().next()?
    };
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_owned())
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    name: &'static str,
    /// `--version` worked, wherever the tool was found.
    installed: bool,
    version: Option<String>,
    /// Found in our tools folder.
    managed: bool,
    pinned: &'static str,
    /// Ours, and not the pinned version.
    update: bool,
}

pub fn view() -> Vec<ToolView> {
    ALL.into_iter()
        .map(|tool| {
            let version = version(tool);
            let managed = tool.managed(root()).exists();
            ToolView {
                name: tool.name,
                installed: version.is_some(),
                update: managed && version.as_deref() != Some(tool.version),
                version,
                managed,
                pinned: tool.version,
            }
        })
        .collect()
}

static INSTALLING: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

/// Marks a tool (or `wine`'s umu-launcher or Proton) as installing until dropped.
pub(crate) struct Installing(&'static str);

impl Installing {
    pub(crate) fn start(name: &'static str) -> Option<Self> {
        let mut busy = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
        // Lazily: a guard built and dropped here would deadlock on this lock and free the running install.
        busy.get_or_insert_default()
            .insert(name)
            .then(|| Self(name))
    }
}

impl Drop for Installing {
    fn drop(&mut self) {
        if let Some(busy) = INSTALLING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            busy.remove(self.0);
        }
    }
}

/// Downloads the pinned build next to its final place, checks its hash, then renames it into place.
/// `on_percent` hears each new whole percent when the size is known.
pub async fn install(
    http: &reqwest::Client,
    tool: &'static Tool,
    on_percent: impl FnMut(u8),
) -> Result<(), Msg> {
    let _installing = Installing::start(tool.name)
        .ok_or_else(|| msg!(toolAlreadyInstalling, tool = tool.title))?;
    let dest = tool.managed(root());
    let part = dest.with_extension("exe.part");
    let digest = match download::<Sha256>(http, tool.url, DOWNLOAD_TIMEOUT, &part, on_percent).await
    {
        Ok(digest) => digest,
        Err(e) => {
            tracing::warn!("install {}: {e}", tool.name);
            let _ = fs::remove_file(&part);
            return Err(msg!(cantDownloadTool, tool = tool.title));
        }
    };
    place(&part, &dest, &digest, tool.sha256)
        .inspect_err(|e| tracing::warn!("install {}: {e}", tool.name))?;
    VERSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .remove(tool.name);
    tracing::info!("installed {} {}", tool.name, tool.version);
    Ok(())
}

/// Streams `url` into `part` and returns its hash `D` (SHA-256 for the tools) in hex.
pub(crate) async fn download<D: Digest>(
    http: &reqwest::Client,
    url: &str,
    timeout: Duration,
    part: &Path,
    mut on_percent: impl FnMut(u8),
) -> Result<String, crate::library::Error> {
    let mut response = http
        .get(url)
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?;
    let total = response.content_length().filter(|&n| n > 0);
    fs::create_dir_all(part.parent().expect("a tool path has a folder"))?;
    let mut file = File::create(part)?;
    let mut hasher = D::new();
    let (mut done, mut shown) = (0u64, None);
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        done += chunk.len() as u64;
        if let Some(total) = total {
            let percent = (done.min(total) * 100 / total) as u8;
            if shown != Some(percent) {
                shown = Some(percent);
                on_percent(percent);
            }
        }
    }
    file.sync_all()?;
    Ok(hex(&hasher.finalize()))
}

/// `install` into another tools folder, for tests that must not touch the owner's.
#[cfg(test)]
pub async fn install_to(http: &reqwest::Client, tool: &Tool, root: &Path) -> Result<(), Msg> {
    let (part, dest) = (
        tool.managed(root).with_extension("exe.part"),
        tool.managed(root),
    );
    let digest = download::<Sha256>(http, tool.url, DOWNLOAD_TIMEOUT, &part, |_| {}).await?;
    place(&part, &dest, &digest, tool.sha256)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Moves a verified download into place; one that doesn't match `expected` is deleted and the old copy kept.
fn place(part: &Path, dest: &Path, digest: &str, expected: &str) -> Result<(), Msg> {
    if digest != expected {
        let _ = fs::remove_file(part);
        return Err(msg!(downloadMismatch));
    }
    #[cfg(unix)]
    let made_runnable = {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(part, std::fs::Permissions::from_mode(0o755))
    };
    #[cfg(not(unix))]
    let made_runnable = Ok(());
    made_runnable
        .and_then(|()| fs::rename(part, dest))
        .map_err(|e| {
            let _ = fs::remove_file(part);
            tracing::warn!("install {}: {e}", dest.display());
            // Windows refuses to replace a program that is running.
            msg!(cantReplaceTool)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_as_lowercase_hex() {
        // SHA-256 of "abc", from FIPS 180-2.
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn pinned_hashes_are_lowercase_sha256_hex() {
        for tool in ALL {
            assert_eq!(tool.sha256.len(), 64, "{}", tool.name);
            assert!(
                tool.sha256
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{}",
                tool.name
            );
            assert!(tool.url.contains(tool.version), "{}", tool.name);
        }
    }

    #[test]
    fn a_matching_download_replaces_the_old_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (part, dest) = (dir.join("legendary.exe.part"), dir.join("legendary.exe"));
        fs::write(&part, "new").unwrap();
        fs::write(&dest, "old").unwrap();
        let digest = hex(&Sha256::digest(b"new"));
        place(&part, &dest, &digest, &digest).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
        assert!(!part.exists());
    }

    #[test]
    fn a_mismatched_download_is_deleted_and_the_old_copy_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (part, dest) = (dir.join("legendary.exe.part"), dir.join("legendary.exe"));
        fs::write(&part, "tampered").unwrap();
        fs::write(&dest, "old").unwrap();
        let digest = hex(&Sha256::digest(b"tampered"));
        assert_eq!(
            place(&part, &dest, &digest, LEGENDARY.sha256),
            Err(msg!(downloadMismatch))
        );
        assert!(!part.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
    }

    #[test]
    fn lookup_prefers_ours_then_local_app_data_then_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (root, local) = (dir.join("tools"), dir.join("local"));
        assert_eq!(
            LEGENDARY.locate(&root, Some(&local)),
            PathBuf::from("legendary")
        );
        assert_eq!(GOGDL.locate(&root, Some(&local)), PathBuf::from("gogdl"));

        #[cfg(windows)]
        {
            let owners = local
                .join("Programs")
                .join("legendary")
                .join("legendary.exe");
            fs::create_dir_all(owners.parent().unwrap()).unwrap();
            fs::write(&owners, "").unwrap();
            assert_eq!(LEGENDARY.locate(&root, Some(&local)), owners);
            assert_eq!(LEGENDARY.locate(&root, None), PathBuf::from("legendary"));
        }

        let ours = root.join("legendary").join(LEGENDARY.exe);
        fs::create_dir_all(ours.parent().unwrap()).unwrap();
        fs::write(&ours, "").unwrap();
        assert_eq!(LEGENDARY.locate(&root, Some(&local)), ours);
    }

    #[test]
    fn parses_each_tools_version_line() {
        assert_eq!(
            parse_version("legendary version \"0.21.1\", codename \"Lowlife\"\n").as_deref(),
            Some("0.21.1")
        );
        assert_eq!(parse_version("1.4.0\r\n").as_deref(), Some("1.4.0"));
        assert_eq!(
            parse_version("1.2.0 Robert Speedwagon\n").as_deref(),
            Some("1.2.0")
        );
        assert_eq!(parse_version("something else"), None);
        assert_eq!(parse_version("legendary version \"\""), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn one_install_per_tool_at_a_time() {
        let first = Installing::start("test-tool").unwrap();
        assert!(Installing::start("test-tool").is_none());
        assert!(Installing::start("other-test-tool").is_some());
        drop(first);
        assert!(Installing::start("test-tool").is_some());
    }

    /// Downloads every pinned build into a temp folder and checks its hash and `--version`.
    /// Network: run by hand with `cargo test -- --ignored real_downloads`.
    #[test]
    #[ignore]
    fn real_downloads_match_their_pins() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let http = reqwest::Client::new();
        tauri::async_runtime::block_on(async {
            for tool in ALL {
                let part = tool.managed(dir).with_extension("exe.part");
                let digest = download::<Sha256>(&http, tool.url, DOWNLOAD_TIMEOUT, &part, |_| {})
                    .await
                    .unwrap();
                let dest = tool.managed(dir);
                place(&part, &dest, &digest, tool.sha256).unwrap();
                let output = Command::new(&dest).arg("--version").output().unwrap();
                assert_eq!(
                    parse_version(&String::from_utf8_lossy(&output.stdout)).as_deref(),
                    Some(tool.version)
                );
            }
        });
    }

    #[test]
    fn tool_view_payload_snapshot() {
        let view = ToolView {
            name: LEGENDARY.name,
            installed: true,
            version: Some("0.0.1".into()),
            managed: true,
            pinned: LEGENDARY.version,
            update: true,
        };
        // The pinned version moves with every tool bump.
        insta::assert_json_snapshot!(view, { ".pinned" => "[version]" });
    }
}
