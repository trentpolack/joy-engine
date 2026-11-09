// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Play: run the open project's game in its own window from the native editor.
//!
//! Joy Editor never embeds a game. Play starts the project's development command in the project folder: its npm `dev`
//! script when its `package.json` defines one (a hosting workspace's convention), otherwise `cargo run` for a project
//! that is a Cargo package. Either way the game runs its native development build, so the game opens in a separate window with its own
//! input and development tools. Games built on `GameAssets` hot reload the assets the editor saves, so editing a FORM
//! asset, effect, or level here updates the running game without restarting it.
//!
//! The session owns at most one game process; stopping Play, starting another project, or closing the editor ends
//! it. Browser builds cannot start processes, so Play reports that it needs the native editor.

use std::path::Path;

/// The project manifest that must define the development command.
const PACKAGE_MANIFEST: &str = "package.json";

/// The npm script that runs a game's native development build.
const DEV_SCRIPT: &str = "dev";

/// The Cargo manifest of a project that is its own Rust package.
const CARGO_MANIFEST: &str = "Cargo.toml";

/// How Play starts a project's game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchCommand {
    /// `npm run dev`.
    NpmDev,
    /// `cargo run`.
    CargoRun,
}

/// Outcomes Play reports; browser builds never start a game, so only native builds use the first and last.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const STARTING: &str = "starting";
const STOPPED: &str = "stopped";
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const EXITED: &str = "exited";

/// Where Play stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// No game is running.
    Idle,
    /// A project's game is running (or still building).
    Running {
        /// The project's display name.
        project: String,
    },
    /// The last game ended; `message` says how.
    Ended {
        /// The project's display name.
        project: String,
        /// How it ended, such as "exited normally" or a failure.
        message: String,
    },
}

/// Owns the running game process, if any.
#[derive(Debug, Default)]
pub struct PlaySession {
    #[cfg(not(target_arch = "wasm32"))]
    process: Option<(String, std::process::Child)>,
    last: Option<(String, String)>,
}

impl PlaySession {
    /// An idle session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this build can start games (native editor builds).
    pub fn available() -> bool {
        cfg!(not(target_arch = "wasm32"))
    }

    /// The current state.
    pub fn state(&self) -> PlayState {
        if let Some(project) = self.running_project() {
            return PlayState::Running {
                project: project.to_owned(),
            };
        }
        match &self.last {
            Some((project, message)) => PlayState::Ended {
                project: project.clone(),
                message: message.clone(),
            },
            None => PlayState::Idle,
        }
    }

    /// Start `project`'s game from its folder, ending any game already running. Fails when the folder has neither a
    /// `package.json` with a `dev` script nor a `Cargo.toml`, or the command cannot start.
    pub fn start(&mut self, project: &str, folder: &Path) -> Result<(), String> {
        self.stop();
        let package = std::fs::read_to_string(folder.join(PACKAGE_MANIFEST)).ok();
        let command = if package.as_deref().is_some_and(defines_dev_script) {
            LaunchCommand::NpmDev
        } else if folder.join(CARGO_MANIFEST).is_file() {
            LaunchCommand::CargoRun
        } else {
            return Err(format!(
                "{project} has no \"{DEV_SCRIPT}\" script in {PACKAGE_MANIFEST} and no {CARGO_MANIFEST}, so it has no game to run."
            ));
        };
        self.spawn(project, folder, command)
    }

    /// End the running game, if any.
    pub fn stop(&mut self) {
        if let Some(project) = self.end_process() {
            self.last = Some((project, STOPPED.to_owned()));
        }
    }

    /// Notice a game that ended on its own; returns a status message when one did.
    pub fn poll(&mut self) -> Option<String> {
        let (project, message) = self.finished_process()?;
        let status = format!("{project} {message}.");
        self.last = Some((project, message));
        Some(status)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Process handling (native builds start processes; browser builds report that they cannot)
    // -----------------------------------------------------------------------------------------------------------------

    #[cfg(not(target_arch = "wasm32"))]
    fn running_project(&self) -> Option<&str> {
        self.process.as_ref().map(|(project, _)| project.as_str())
    }

    #[cfg(target_arch = "wasm32")]
    fn running_project(&self) -> Option<&str> {
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn(&mut self, project: &str, folder: &Path, command: LaunchCommand) -> Result<(), String> {
        let mut process = match command {
            LaunchCommand::NpmDev => {
                // npm is a batch script on Windows, which `Command` runs only through its `.cmd` name.
                let mut process = std::process::Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
                process.args(["run", DEV_SCRIPT]);
                process
            }
            LaunchCommand::CargoRun => {
                let mut process = std::process::Command::new("cargo");
                process.arg("run");
                process
            }
        };
        let child = process
            .current_dir(folder)
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start {project}: {error}"))?;
        self.process = Some((project.to_owned(), child));
        self.last = Some((project.to_owned(), STARTING.to_owned()));
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn(&mut self, project: &str, _folder: &Path, _command: LaunchCommand) -> Result<(), String> {
        Err(format!("Play runs {project} from the native editor; the browser editor cannot start games."))
    }

    /// Kill and reap the running game; returns its project.
    #[cfg(not(target_arch = "wasm32"))]
    fn end_process(&mut self) -> Option<String> {
        let (project, mut child) = self.process.take()?;
        let _ = child.kill();
        let _ = child.wait();
        Some(project)
    }

    #[cfg(target_arch = "wasm32")]
    fn end_process(&mut self) -> Option<String> {
        None
    }

    /// The project and outcome of a game that ended on its own since the last check.
    #[cfg(not(target_arch = "wasm32"))]
    fn finished_process(&mut self) -> Option<(String, String)> {
        let (project, child) = self.process.as_mut()?;
        let message = match child.try_wait() {
            Ok(None) => return None,
            Ok(Some(status)) if status.success() => EXITED.to_owned(),
            Ok(Some(status)) => format!("exited with {status}"),
            Err(error) => format!("could not be checked: {error}"),
        };
        let project = project.clone();
        self.process = None;
        Some((project, message))
    }

    #[cfg(target_arch = "wasm32")]
    fn finished_process(&mut self) -> Option<(String, String)> {
        None
    }
}

impl Drop for PlaySession {
    /// Closing the editor ends the game it started.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether a `package.json` text defines a `dev` script.
fn defines_dev_script(manifest: &str) -> bool {
    joy_engine::serde_json::from_str::<joy_engine::serde_json::Value>(manifest)
        .ok()
        .and_then(|value| value.get("scripts")?.get(DEV_SCRIPT)?.as_str().map(|script| !script.trim().is_empty()))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "play.test.rs"]
mod tests;
