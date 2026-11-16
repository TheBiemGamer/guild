//! Gamescope, MangoHud and GameMode around a game on Linux, Windows games (around umu-run) and native ones alike. Each
//! is on when the game's own launch options say so, else when Settings does, and only when it is installed. In the
//! Flatpak, MangoHud and gamescope come from their Flathub Vulkan layer extensions and GameMode from the runtime.

use std::process::Command;

use crate::cache::{Dirs, db};
use crate::library::tool::LaunchOptions;
use crate::platform::GameTools;

const GAMESCOPE: &str = "gamescope";
const MANGOHUD: &str = "mangohud";
const GAMEMODE: &str = "gamemoderun";

/// The tools found on PATH.
pub fn installed() -> GameTools {
    let found = |name| crate::wine::on_path(name).is_some();
    GameTools {
        gamescope: found(GAMESCOPE),
        mangohud: found(MANGOHUD),
        gamemode: found(GAMEMODE),
    }
}

/// The tools a game runs with: its own choice, else the setting, and installed.
pub fn chosen(options: &LaunchOptions) -> GameTools {
    let settings = db::open(&Dirs::new().db)
        .map(|conn| crate::settings::load(&conn).game_tools)
        .unwrap_or_else(|e| {
            tracing::warn!("game tools: {e}");
            GameTools::default()
        });
    pick(options, &settings, installed())
}

fn pick(options: &LaunchOptions, settings: &GameTools, installed: GameTools) -> GameTools {
    GameTools {
        gamescope: options.gamescope.unwrap_or(settings.gamescope) && installed.gamescope,
        mangohud: options.mangohud.unwrap_or(settings.mangohud) && installed.mangohud,
        gamemode: options.gamemode.unwrap_or(settings.gamemode) && installed.gamemode,
    }
}

/// `game` with the chosen tools in front, outermost first: gamescope (fullscreen at the screen's size; MangoHud then
/// draws through its `--mangoapp`), MangoHud, gamemoderun.
pub fn wrap(game: Command, tools: GameTools) -> Command {
    let mut front: Vec<&str> = Vec::new();
    if tools.gamescope {
        front.extend([GAMESCOPE, "-f"]);
        if tools.mangohud {
            front.push("--mangoapp");
        }
        front.push("--");
    } else if tools.mangohud {
        // --dlsym also reaches OpenGL games that load their GL library themselves.
        front.extend([MANGOHUD, "--dlsym"]);
    }
    if tools.gamemode {
        front.push(GAMEMODE);
    }
    let Some((program, args)) = front.split_first() else {
        return game;
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(game.get_program())
        .args(game.get_args());
    if let Some(dir) = game.get_current_dir() {
        command.current_dir(dir);
    }
    for (name, value) in game.get_envs() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn argv(c: &Command) -> Vec<OsString> {
        std::iter::once(c.get_program().to_owned())
            .chain(c.get_args().map(ToOwned::to_owned))
            .collect()
    }

    fn game() -> Command {
        let mut c = Command::new("umu-run");
        c.arg("Game.exe").current_dir("/g").env("WINEPREFIX", "/p");
        c
    }

    const ALL: GameTools = GameTools {
        gamescope: true,
        mangohud: true,
        gamemode: true,
    };

    #[test]
    fn wraps_outermost_first_and_keeps_folder_and_variables() {
        let c = wrap(game(), ALL);
        assert_eq!(
            argv(&c),
            [
                "gamescope",
                "-f",
                "--mangoapp",
                "--",
                "gamemoderun",
                "umu-run",
                "Game.exe"
            ]
        );
        assert_eq!(c.get_current_dir(), Some(std::path::Path::new("/g")));
        assert!(
            c.get_envs()
                .any(|(k, v)| k == "WINEPREFIX" && v == Some("/p".as_ref()))
        );
        let mangohud = GameTools {
            mangohud: true,
            ..GameTools::default()
        };
        assert_eq!(
            argv(&wrap(game(), mangohud)),
            ["mangohud", "--dlsym", "umu-run", "Game.exe"]
        );
        assert_eq!(
            argv(&wrap(game(), GameTools::default())),
            ["umu-run", "Game.exe"]
        );
    }

    #[test]
    fn the_game_choice_beats_settings_and_missing_tools_stay_off() {
        let options = LaunchOptions {
            gamescope: Some(false),
            mangohud: Some(true),
            ..LaunchOptions::default()
        };
        let settings = GameTools {
            gamescope: true,
            mangohud: false,
            gamemode: true,
        };
        assert_eq!(
            pick(&options, &settings, ALL),
            GameTools {
                gamescope: false,
                mangohud: true,
                gamemode: true,
            }
        );
        let none = GameTools::default();
        assert_eq!(pick(&options, &settings, none), none);
    }
}
