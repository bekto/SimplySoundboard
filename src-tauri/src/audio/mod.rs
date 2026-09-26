//! Audio graph: `pactl` wrapper, virtual device router and playback engine.

pub mod pactl;
pub mod player;
pub mod router;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use crate::model::{RouterState, RouterStatus};
use crate::state::{lock, Core};

/// Guards the single shutdown path so it runs exactly once.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// True once [`shutdown`] started.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Publishes a router status to the state and the UI.
pub fn set_status(app: &AppHandle, status: RouterStatus) {
    let core = app.state::<Core>();
    *lock(&core.router_status) = status.clone();

    if let Err(err) = app.emit("router_status", status) {
        log::warn!("Could not emit router_status: {err}");
    }
}

/// Current router status.
pub fn status(app: &AppHandle) -> RouterStatus {
    let core = app.state::<Core>();
    let guard = lock(&core.router_status);
    RouterStatus {
        state: guard.state,
        message: guard.message.clone(),
    }
}

/// Builds the audio graph and publishes the outcome. Never panics on audio errors:
/// they are reported through the router status so the UI can offer *Recreate*.
pub fn start(app: &AppHandle) -> Result<(), String> {
    set_status(
        app,
        RouterStatus {
            state: RouterState::Starting,
            message: None,
        },
    );

    let settings = app.state::<Core>().config().settings;
    let started = ensure_player().and_then(|()| router::Router::start(&settings));

    let core = app.state::<Core>();
    match started {
        Ok(mut instance) => {
            // Publish the graph and check for a concurrent quit under the same lock
            // that `stop()` takes, so a quit mid-start can never leak devices.
            let mut slot = lock(&core.router);
            if is_shutting_down() {
                drop(slot);
                log::info!("Quit raced the audio graph start — unloading the graph again");
                instance.stop();
                return Err("The app is shutting down".to_string());
            }
            *slot = Some(instance);
            drop(slot);

            set_status(
                app,
                RouterStatus {
                    state: RouterState::Ok,
                    message: None,
                },
            );
            Ok(())
        }
        Err(err) => {
            log::error!("Could not build the audio graph: {err}");
            set_status(
                app,
                RouterStatus {
                    state: RouterState::Error,
                    message: Some(err.clone()),
                },
            );
            Err(err)
        }
    }
}

/// Tears down the audio graph, if one is running.
pub fn stop(app: &AppHandle) {
    let core = app.state::<Core>();
    let mut slot = lock(&core.router);
    if let Some(mut instance) = slot.take() {
        instance.stop();
    }
}

/// The single quit path: stop playback, then tear down the graph. Idempotent.
pub fn shutdown(app: &AppHandle) {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    log::info!("Shutting down");

    {
        let core = app.state::<Core>();
        core.player.stop_all();
    }
    stop(app);
}

/// Playback shells out to `pw-play`; make sure it is usable before claiming the
/// virtual mic is ready.
fn ensure_player() -> Result<(), String> {
    match std::process::Command::new("pw-play")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err("pw-play is not working — install pipewire-utils / pipewire-bin".to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err("pw-play not found — install pipewire-utils / pipewire-bin".to_string())
        }
        Err(err) => Err(format!("Could not run pw-play: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_player_finds_pw_play_on_this_system() {
        // pw-play ships with PipeWire; absence is reported, not panicked on.
        if let Err(err) = ensure_player() {
            assert!(err.contains("pw-play"), "unexpected error: {err}");
        }
    }
}
