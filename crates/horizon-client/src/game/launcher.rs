//! The phone's launcher (packaging/ios/Launcher), answered: it speaks the control socket's language through
//! `horizon_ios_launcher`'s queue, so `worlds`, `open`, `connect`, `options-set` and the rest mean what they
//! mean to horizon-ctl. Four lines are its own: `launcher-start`, which lets the game load (it waits for the
//! player to start it); `launcher-state`, which says whether the game waits to be started, is loading, waits
//! for files or is at its title; `import PATH`, which installs an archive the launcher downloaded; and
//! `launcher-world {json}`, Select World's actions for folders with spaces in their names.
//!
//! The loading window answers the start, the state and imports before there is a `Game` to ask; anything else
//! waits for the title.
use super::{Game, Loading};
use crate::game::event_loop::EventLoopCtl;
use horizon_client::{
    Result,
    import::{Installed, Kind, Pack},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, TryRecvError},
};

/// The portrait launcher and landscape game share a UIWindow. Permit the
/// launcher's orientation before acknowledging its presentation request.
fn launcher_visibility(window: Option<&winit::window::Window>, visible: bool) {
    horizon_ios_launcher::set_covered(visible);
    #[cfg(target_os = "ios")]
    if let Some(window) = window {
        use winit::platform::ios::{ValidOrientations, WindowExtIOS};
        window.set_valid_orientations(if visible {
            ValidOrientations::LandscapeAndPortrait
        } else {
            ValidOrientations::Landscape
        });
    }
    #[cfg(not(target_os = "ios"))]
    let _ = window;
}

/// The archive a phone is unpacking, as the launcher's loading screen draws it (`import::status`).
fn import_state() -> Value {
    match horizon_client::import::status() {
        Some(status) => json!({
            "label": status.label,
            "files": status.files,
            "total_files": status.total_files,
            "bytes": status.bytes,
            "error": status.error,
        }),
        None => Value::Null,
    }
}

/// What came back for a launcher request so far: a control answer, an import's result, or a mod
/// install's.
enum Waiting {
    Control(Receiver<String>),
    Import(Receiver<std::result::Result<Installed, String>>),
    Mods(Receiver<std::result::Result<Value, String>>),
}

thread_local! {
    /// Launcher requests whose answers have not come back yet. Only the main thread touches them.
    static PENDING: RefCell<Vec<(Waiting, horizon_ios_launcher::Request)>> = const { RefCell::new(Vec::new()) };
}

/// Answer what has come back and keep waiting on the rest. True when an import brought game files.
fn deliver() -> bool {
    let mut resources = false;
    let pending = PENDING.with_borrow_mut(std::mem::take);
    let mut still = Vec::new();
    for (waiting, request) in pending {
        match waiting {
            Waiting::Control(answers) => match answers.try_recv() {
                Ok(answer) => request.answer(&answer),
                // Dropping the request answers it with an error.
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => still.push((Waiting::Control(answers), request)),
            },
            Waiting::Import(result) => match result.try_recv() {
                Ok(Ok(installed)) => {
                    resources |= installed.kind == Kind::Resources;
                    request.answer(&installed_json(&installed).to_string());
                }
                Ok(Err(error)) => request.answer(&json!({ "error": error }).to_string()),
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => still.push((Waiting::Import(result), request)),
            },
            Waiting::Mods(result) => match result.try_recv() {
                Ok(Ok(installed)) => request.answer(&installed.to_string()),
                Ok(Err(error)) => request.answer(&json!({ "error": error }).to_string()),
                Err(TryRecvError::Disconnected) => {}
                Err(TryRecvError::Empty) => still.push((Waiting::Mods(result), request)),
            },
        }
    }
    PENDING.with_borrow_mut(|pending| pending.extend(still));
    resources
}

/// Install the archive named by `import PATH` on a worker: a prepared tree is hundreds of megabytes.
fn start_import(request: horizon_ios_launcher::Request) {
    let path = request
        .line
        .trim_start()
        .strip_prefix("import")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from);
    let Some(path) = path else {
        request.answer(r#"{"error":"import PATH: the archive to install"}"#);
        return;
    };
    let home = horizon_client::import::home();
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("horizon-import".into())
        .spawn(move || {
            let installed = Pack::open(&path)
                .and_then(|pack| pack.install(&home))
                .map_err(|e| e.to_string());
            let _ = sender.send(installed);
        });
    match spawned {
        Ok(_) => PENDING.with_borrow_mut(|p| p.push((Waiting::Import(receiver), request))),
        Err(error) => request.answer(&json!({ "error": error.to_string() }).to_string()),
    }
}

/// Install the `.hmod` named by `mods-install PATH` on a worker: it validates the package's content
/// and moves it into `horizon-mods`, off the thread that draws.
fn start_mods_install(request: horizon_ios_launcher::Request) {
    let staged = match super::mods_control::install_arg(&request.line) {
        Ok(staged) => staged,
        Err(error) => {
            request.answer(&json!({ "error": error }).to_string());
            return;
        }
    };
    let dir = std::env::current_dir().unwrap_or_default();
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("horizon-mods-install".into())
        .spawn(move || {
            let _ = sender.send(super::mods_control::install(&dir, &staged));
        });
    match spawned {
        Ok(_) => PENDING.with_borrow_mut(|p| p.push((Waiting::Mods(receiver), request))),
        Err(error) => request.answer(&json!({ "error": error.to_string() }).to_string()),
    }
}

/// `mods-list`, `mods-set` and `mods-delete`: files only, so both launchers answer them at once.
fn mods_command(request: horizon_ios_launcher::Request) {
    let dir = std::env::current_dir().unwrap_or_default();
    let answer = match super::mods_control::command(&dir, &request.line) {
        Ok(Some(value)) => value,
        Ok(None) => json!({ "error": format!("{}: not a mods command", request.line) }),
        Err(error) => json!({ "error": error.to_string() }),
    };
    request.answer(&answer.to_string());
}

/// The launchers' settings hold the mod list as it was when they read it: read it again after a
/// command changed it, so the next Play loads what the Mods page says.
fn refresh_mods(settings: &mut horizon_client::settings::Settings) {
    if super::mods_control::take_dirty()
        && let Ok(dir) = std::env::current_dir()
    {
        super::mods_control::reload(settings, &dir);
    }
}

fn installed_json(installed: &Installed) -> Value {
    json!({
        "ok": true,
        "kind": match installed.kind {
            Kind::Resources => "resources",
            Kind::World => "world",
            Kind::ResourcePack => "resource_pack",
        },
        "name": installed.path.file_name().map(|n| n.to_string_lossy().into_owned()),
        "files": installed.files,
        "bytes": installed.bytes,
    })
}

impl Game<'_> {
    /// The launcher's requests, once a frame: its own lines here, the rest through the control socket's
    /// dispatcher, as if horizon-ctl had sent them.
    pub(super) fn poll_launcher(&mut self, event_loop: &dyn EventLoopCtl) {
        for request in horizon_ios_launcher::take() {
            let command = request.line.split_whitespace().next().unwrap_or_default();
            match command {
                "launcher-visible" => {
                    launcher_visibility(self.window.as_deref(), request.line.ends_with(" true"));
                    request.answer(r#"{"ok":true}"#);
                }

                "launcher-state" => {
                    let state = json!({
                        "phase": if self.title.is_some() { "title" } else { "world" },
                        "screen": self.screen_name(),
                        "player": self.options.player_name(),
                        "version": horizon_core::version::VERSION_ID,
                        "import": import_state(),
                    });
                    request.answer(&state.to_string());
                }
                "import" => start_import(request),
                "mods-list" | "mods-set" | "mods-delete" => mods_command(request),
                "mods-install" => start_mods_install(request),
                // The game is up: the download screen has nothing left to show.
                "download-state" => request.answer(r#"{"phase":"done"}"#),
                "launcher-start" => request.answer(r#"{"ok":true}"#),
                "launcher-session" => session_request(request),
                "launcher-world" => self.launcher_world(request),
                _ => {
                    let (reply, answers) = mpsc::channel();
                    let line = request.line.clone();
                    self.answer(horizon_client::control::Request { line, reply }, event_loop);
                    PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
                }
            }
        }
        // Game files that arrive while the game runs are read on its next start.
        deliver();
        refresh_mods(&mut self.settings);
    }

    /// `launcher-world`: Select World's Play by folder here; the rest is files (`world_files`).
    fn launcher_world(&mut self, request: horizon_ios_launcher::Request) {
        let Some(request) = world_files(request) else {
            // Renamed or deleted: Select World reads the saves again.
            if let Some(title) = self.title.as_mut() {
                title.worlds_changed();
            }
            return;
        };
        // Only Play is left: it needs the title screen.
        let args = world_args(&request.line);
        let folder = args["folder"].as_str().unwrap_or_default().to_owned();
        let answer = if self.title.is_none() {
            json!({ "error": "a world is already open" })
        } else {
            match self.open_world(&folder, None, None, false, None) {
                Ok(()) => json!({ "ok": true }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        };
        request.answer(&answer.to_string());
    }
}

/// `launcher-world {"action": "open"|"rename"|"backup"|"delete", "folder": ..., "name": ...}`'s arguments: Select
/// World and Edit World by folder, for folders the control socket's word-split lines cannot name ("New World").
fn world_args(line: &str) -> Value {
    line.trim_start()
        .strip_prefix("launcher-world")
        .and_then(|rest| serde_json::from_str(rest).ok())
        .unwrap_or_default()
}

/// Rename, Back Up and Delete, which are files and need no game; the request back for Play, which does.
fn world_files(request: horizon_ios_launcher::Request) -> Option<horizon_ios_launcher::Request> {
    let args = world_args(&request.line);
    let (Some(action), Some(folder)) = (args["action"].as_str(), args["folder"].as_str()) else {
        request.answer(r#"{"error":"launcher-world {\"action\", \"folder\"}"}"#);
        return None;
    };
    let saves = std::env::current_dir().unwrap_or_default().join("saves");
    let done = match action {
        "open" => return Some(request),
        "rename" => {
            let name = args["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() {
                Err("a world needs a name".into())
            } else {
                horizon_client::title::world_management::rename(&saves, folder, name)
            }
        }
        "delete" => horizon_client::title::world_management::delete(&saves, folder),
        "backup" => {
            // A big world takes a while to zip: off the main thread, as Make Backup does.
            let (reply, answers) = mpsc::channel();
            let folder = folder.to_owned();
            std::thread::spawn(move || {
                let value = match horizon_client::title::world_management::backup(&saves, &folder) {
                    Ok(bytes) => json!({ "ok": true, "bytes": bytes }),
                    Err(e) => json!({ "error": format!("backup of {folder} failed: {e}") }),
                };
                let _ = reply.send(value.to_string());
            });
            PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
            return None;
        }
        _ => Err(format!("launcher-world: no action {action}").into()),
    };
    let answer = match done {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "error": e.to_string() }),
    };
    request.answer(&answer.to_string());
    None
}

/// `launcher-session {"name", "uuid", "access_token", "user_type": "msa", "xuid"}`: the Microsoft account the
/// phone's launcher signed in (the session file's fields, docs/microsoft-account.md), kept in memory only;
/// `launcher-session off` plays offline again. The token is never echoed back.
fn session_request(request: horizon_ios_launcher::Request) {
    let rest = request
        .line
        .trim_start()
        .strip_prefix("launcher-session")
        .map(str::trim)
        .unwrap_or_default();
    let answer = if rest == "off" {
        horizon_client::auth::clear_current();
        json!({ "ok": true })
    } else {
        match horizon_client::auth::AuthSession::parse(rest) {
            Ok(session) => {
                let name = session.name.clone();
                horizon_client::auth::set_current(session);
                json!({ "ok": true, "name": name })
            }
            Err(e) => json!({ "error": e }),
        }
    };
    request.answer(&answer.to_string());
}

/// The control lines that are only files (worlds, servers, pings: `saves_control::file_command`), answered
/// without a game; the request back for anything else.
fn file_request(request: horizon_ios_launcher::Request) -> Option<horizon_ios_launcher::Request> {
    let words: Vec<&str> = request.line.split_whitespace().collect();
    let (reply, answers) = mpsc::channel();
    match super::saves_control::file_command(&words, &request.line, &reply) {
        Ok(None) => Some(request),
        Ok(Some(Some(value))) => {
            request.answer(&value.to_string());
            None
        }
        // Answered later, from a worker.
        Ok(Some(None)) => {
            PENDING.with_borrow_mut(|p| p.push((Waiting::Control(answers), request)));
            None
        }
        Err(e) => {
            request.answer(&json!({ "error": e.to_string() }).to_string());
            None
        }
    }
}

impl Loading {
    /// `options-get KEY...` and `options-set KEY VALUE` before the game exists, on the settings it will be
    /// made with, saved to options.txt as the options screen saves them (game/options.rs does the same in a
    /// running game, where a change also applies at once).
    fn launcher_options(&mut self, line: &str) -> Value {
        let words: Vec<&str> = line.split_whitespace().collect();
        let file = self.settings.to_file();
        if words[0] == "options-get" {
            let mut options = serde_json::Map::new();
            for key in &words[1..] {
                match file.describe(key) {
                    Some(described) => options.insert((*key).to_owned(), described),
                    None => return json!({ "error": format!("unknown option {key}") }),
                };
            }
            return json!({ "options": options });
        }
        let Some(key) = words.get(1).copied() else {
            return json!({ "error": "options-set KEY VALUE" });
        };
        let value = line
            .trim_start()
            .strip_prefix("options-set")
            .and_then(|rest| rest.trim_start().strip_prefix(key))
            .map(str::trim)
            .unwrap_or_default();
        // Horizon's own files are not in options.txt: with_option keeps them.
        // A graphics preset also applies its options there, as the options
        // screen's slider does.
        self.settings = match self.settings.with_option(key, value) {
            Ok(settings) => settings,
            Err(e) => return json!({ "error": e }),
        };
        self.settings.save(
            &std::env::current_dir()
                .unwrap_or_default()
                .join("options.txt"),
        );
        json!({ "ok": true, "key": key, "option": self.settings.to_file().describe(key) })
    }

    /// The launcher's requests before the game exists: the start, its state, imports (how a phone with no game
    /// files gets them from the launcher), the options, and what is only files: worlds, servers, pings.
    /// Game files that arrive end the wait for them.
    pub(super) fn poll_launcher(&mut self) -> Result<()> {
        for request in horizon_ios_launcher::take() {
            let command = request.line.split_whitespace().next().unwrap_or_default();
            match command {
                "launcher-visible" => {
                    launcher_visibility(self.window.as_deref(), request.line.ends_with(" true"));
                    request.answer(r#"{"ok":true}"#);
                }

                "launcher-start" => {
                    // Play: take the folder an archive dropped into `Documents` produced, if it is in.
                    self.adopt_imported_resources();
                    horizon_ios_launcher::start();
                    request.answer(r#"{"ok":true}"#);
                }
                "launcher-state" => {
                    let state = if horizon_ios_launcher::waiting() {
                        json!({ "phase": "waiting", "import": import_state() })
                    } else if self.importing() {
                        json!({ "phase": "needs_files", "reason": self.import_reason(), "import": import_state() })
                    } else {
                        json!({
                            "phase": "loading",
                            "progress": self.progress.fraction(),
                            "import": import_state(),
                        })
                    };
                    request.answer(&state.to_string());
                }
                "download-state" => request.answer(&self.download_state().to_string()),
                "download-retry" => {
                    let answer = match self.retry_download() {
                        Ok(()) => json!({ "ok": true }),
                        Err(e) => json!({ "error": e.to_string() }),
                    };
                    request.answer(&answer.to_string());
                }
                "import" => start_import(request),
                "launcher-session" => session_request(request),
                "options-get" | "options-set" => {
                    let answer = self.launcher_options(&request.line);
                    request.answer(&answer.to_string());
                }
                "shader-pack" => {
                    let words: Vec<&str> = request.line.split_whitespace().collect();
                    let answer = match super::shader_packs::command(
                        &mut self.settings,
                        &words,
                        &request.line,
                    ) {
                        Ok(value) => value.unwrap_or_default(),
                        Err(e) => json!({ "error": e.to_string() }),
                    };
                    request.answer(&answer.to_string());
                }
                "launcher-world" => {
                    if let Some(request) = world_files(request) {
                        request.answer(
                            &json!({ "error": "start the game to play a world", "starting": true })
                                .to_string(),
                        );
                    }
                }
                "mods-list" | "mods-set" | "mods-delete" => mods_command(request),
                "mods-install" => start_mods_install(request),
                _ => {
                    if let Some(request) = file_request(request) {
                        request.answer(
                            &json!({ "error": "the game is still starting", "starting": true })
                                .to_string(),
                        );
                    }
                }
            }
        }
        let resources = deliver();
        // Before a restarted load reads the settings.
        refresh_mods(&mut self.settings);
        if resources && self.importing() {
            self.files_arrived()?;
        }
        Ok(())
    }
}
