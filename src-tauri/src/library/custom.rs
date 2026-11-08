//! Games the owner added: any program, kept in the `custom_games` table (owner data). Its cover is a file
//! in the data folder, `<data>/custom/<id>/cover.<ext>`, so clearing the cache never loses it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Art, Error, Game, Image, Progress, Provider, ProviderKind};
use crate::cache::Dirs;
use crate::msg::Msg;

/// One row of `custom_games`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomGame {
    pub id: String,
    pub name: String,
    /// Absolute path of the program.
    pub exe: String,
    /// As typed; split by `epic::split_args`.
    pub args: String,
    /// Unix seconds.
    pub added: i64,
}

pub struct Custom;

/// Every added game, from the database, its covers under `dirs.custom`.
pub fn list(dirs: &Dirs) -> Result<Vec<Game>, Error> {
    let conn = crate::cache::db::open(&dirs.db)?;
    Ok(crate::cache::db::custom_games(&conn)?
        .iter()
        .map(|c| game(c, &dirs.custom))
        .collect())
}

/// Listed by `list` and played by `commands::game::act_custom`, which know the app's folders.
fn not_here() -> Error {
    msg!(customNotHere).into()
}

impl Provider for Custom {
    fn list_games(&self) -> Result<Vec<Game>, Error> {
        Err(not_here())
    }

    fn launch(&self, _game: &Game) -> Result<(), Error> {
        Err(not_here())
    }

    /// A not-installed added game is one whose program is gone.
    fn install(&self, game: &Game) -> Result<(), Error> {
        Err(missing(&game.title))
    }

    fn uninstall(&self, _game: &Game) -> Result<(), Error> {
        Err(msg!(customUninstall).into())
    }

    /// Watched through its process once launched from here (`commands::Watch`).
    fn running_game(&self) -> Option<String> {
        None
    }

    fn download_progress(&self, _game: &Game) -> Option<Progress> {
        None
    }
}

fn missing(title: &str) -> Error {
    msg!(customProgramMissing, title = title).into()
}

/// A new id: the time in hex, short and safe as a file name.
pub fn new_id() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    )
}

/// The id in a `custom-<id>` key: hex, as `new_id` makes it. Anything else is refused, since the id names a folder
/// that removing the game deletes.
pub fn id_of(key: &str) -> Option<&str> {
    key.strip_prefix("custom-")
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Installed while the program is there; the install folder is the program's folder.
pub fn game(c: &CustomGame, covers: &Path) -> Game {
    let exe = Path::new(&c.exe);
    // The cover doubles as the hero art: the game page and the banner show it too.
    let cover = Image {
        path: cover_file(&covers.join(&c.id)),
        url: None,
    };
    Game {
        provider: ProviderKind::Custom,
        external_id: c.id.clone(),
        title: c.name.clone(),
        installed: exe.is_file(),
        install_dir: exe.parent().map(Path::to_path_buf),
        playtime_minutes: None,
        last_played: None,
        size_bytes: None,
        art: Art {
            hero: cover.clone(),
            cover,
            header: Image::default(),
        },
        dlc: Vec::new(),
        update_available: false,
        cloud_saves: false,
    }
}

/// `cover.<ext>` in `dir`, whatever the extension.
pub fn cover_file(dir: &Path) -> Option<PathBuf> {
    fs_err::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_stem() == Some("cover".as_ref()))
}

/// Only an .exe: Windows runs a .bat or .cmd through cmd.exe, and nothing here goes through a shell.
pub fn check_exe(exe: &str) -> Result<(), Msg> {
    let path = Path::new(exe);
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Err(msg!(pickExe));
    }
    if !path.is_file() {
        return Err(msg!(programMissing, path = exe));
    }
    Ok(())
}

/// The command a launch runs: the program and its arguments, in the program's folder.
fn command(c: &CustomGame) -> Command {
    let exe = Path::new(&c.exe);
    let mut command = Command::new(exe);
    command.args(super::epic::split_args(&c.args));
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    command
}

/// Starts the program and returns its process, to watch. Doesn't wait for it.
pub fn launch(c: &CustomGame) -> Result<std::process::Child, Error> {
    check_exe(&c.exe).map_err(|_| missing(&c.name))?;
    Ok(command(c).spawn()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(exe: &str, args: &str) -> CustomGame {
        CustomGame {
            id: "18f2a".into(),
            name: "Hollow Knight".into(),
            exe: exe.into(),
            args: args.into(),
            added: 1,
        }
    }

    #[test]
    fn keys_are_custom_dash_id_and_parse_back() {
        let id = new_id();
        assert!(
            !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()),
            "{id}"
        );
        let key = ProviderKind::Custom.key(&id);
        assert_eq!(key, format!("custom-{id}"));
        assert_eq!(id_of(&key), Some(id.as_str()));
        assert_eq!(ProviderKind::parse("custom"), Some(ProviderKind::Custom));
        assert_eq!(id_of("custom-"), None);
        assert_eq!(id_of("epic-custom-1"), None);
        for bad in [
            "custom-..",
            r"custom-C:\x",
            "custom-a/b",
            r"custom-..\..",
            "custom-18f2a ",
        ] {
            assert_eq!(id_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn launch_runs_the_exe_in_its_folder_with_split_args() {
        let c = row(
            r"D:\Games\Hollow Knight\hollow_knight.exe",
            r#"-screen-fullscreen 0 -name "Big Boss""#,
        );
        let command = command(&c);
        assert_eq!(
            command.get_program(),
            r"D:\Games\Hollow Knight\hollow_knight.exe"
        );
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-screen-fullscreen", "0", "-name", "Big Boss"]);
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new(r"D:\Games\Hollow Knight"))
        );
        // No arguments: none at all, not one empty one.
        assert_eq!(
            super::command(&row(r"C:\x.exe", "  ")).get_args().count(),
            0
        );
    }

    #[test]
    fn only_existing_exe_files_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (exe, bat) = (dir.join("game.EXE"), dir.join("game.bat"));
        std::fs::write(&exe, b"").unwrap();
        std::fs::write(&bat, b"").unwrap();
        assert!(check_exe(&exe.to_string_lossy()).is_ok());
        assert!(check_exe(&bat.to_string_lossy()).is_err());
        assert!(check_exe(&dir.join("gone.exe").to_string_lossy()).is_err());
        assert!(launch(&row(&dir.join("gone.exe").to_string_lossy(), "")).is_err());
    }

    #[test]
    fn game_is_installed_while_the_exe_exists_and_finds_its_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let covers = root.join("custom");
        std::fs::create_dir_all(covers.join("18f2a")).unwrap();
        let exe = root.join("game.exe");
        let c = row(&exe.to_string_lossy(), "");
        let g = game(&c, &covers);
        assert_eq!(
            (g.key(), g.title.as_str(), g.installed),
            ("custom-18f2a".into(), "Hollow Knight", false)
        );
        assert_eq!(g.install_dir.as_deref(), Some(root));
        assert_eq!(g.art.cover.path, None);
        std::fs::write(&exe, b"").unwrap();
        std::fs::write(covers.join("18f2a").join("cover.png"), b"x").unwrap();
        let g = game(&c, &covers);
        assert!(g.installed);
        assert_eq!(
            g.art.cover.path,
            Some(covers.join("18f2a").join("cover.png"))
        );
        assert_eq!(g.art.hero, g.art.cover);
    }
}
