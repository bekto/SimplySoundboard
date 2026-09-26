//! Audio graph: `pactl` wrapper, virtual device router and playback engine.

pub mod pactl;
pub mod player;
pub mod router;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use crate::model::{MicDevice, RouterState, RouterStatus, Settings, SettingsPatch};
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
    let started = ensure_player()
        .and_then(|()| router::Router::start(&settings))
        .map(|router| (router, settings));

    let core = app.state::<Core>();
    match started {
        Ok((mut instance, settings)) => {
            if let Err(err) = instance.apply_all(&settings) {
                log::warn!("Could not apply the initial volumes: {err}");
            }
            if let Err(err) = apply_default_mic(&settings) {
                log::warn!("Could not take over the default input: {err}");
            }

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

/// Live side effects of a settings change: graph shape, volumes, default input.
pub fn apply_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let mut errors = Vec::new();

    {
        let core = app.state::<Core>();
        let mut slot = lock(&core.router);
        match slot.as_mut() {
            Some(router) => {
                if let Err(err) = router.apply_all(settings) {
                    errors.push(err);
                }
            }
            None => errors.push("audio graph is not running".to_string()),
        }
    }

    if let Err(err) = apply_default_mic(settings) {
        errors.push(err);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Real input devices for the picker: our own devices and raw monitors excluded.
pub fn list_mics(settings: &Settings) -> Result<Vec<MicDevice>, String> {
    let current_default = pactl::default_source().unwrap_or_default();
    // While we hold the system default, the "real" default is the saved one.
    let real_default = if current_default == router::MIC_SOURCE {
        settings.previous_default_source.clone().unwrap_or_default()
    } else {
        current_default
    };

    Ok(pactl::list_sources()?
        .into_iter()
        .filter(|source| !router::is_feedback_prone(&source.name))
        .map(|source| MicDevice {
            description: if source.description.is_empty() {
                source.name.clone()
            } else {
                source.description
            },
            is_default: source.name == real_default,
            name: source.name,
        })
        .collect())
}

/// The source handed back when the app quits, if this patch takes over the default.
pub fn capture_previous_default(before: &Settings, patch: &SettingsPatch) -> Option<String> {
    if patch.use_as_default_mic != Some(true) || before.use_as_default_mic {
        return None;
    }

    match pactl::default_source() {
        Ok(current) if !current.is_empty() && current != router::MIC_SOURCE => Some(current),
        Ok(_) => None,
        Err(err) => {
            log::warn!("Could not read the current default source: {err}");
            None
        }
    }
}

/// Makes `ssb_mic` the system default input.
pub fn apply_default_mic(settings: &Settings) -> Result<(), String> {
    if !settings.use_as_default_mic {
        return Ok(());
    }
    if pactl::default_source().unwrap_or_default() == router::MIC_SOURCE {
        return Ok(());
    }
    pactl::set_default_source(router::MIC_SOURCE)
}

/// Puts `previous` back as the default input, unless the user has already pointed
/// the default somewhere else in the meantime.
pub fn restore_default_source(previous: &str) -> Result<(), String> {
    let current = pactl::default_source().unwrap_or_default();
    if current != router::MIC_SOURCE {
        log::info!("Default input is {current:?}, not ours — leaving it alone");
        return Ok(());
    }

    if !pactl::list_sources()?
        .iter()
        .any(|source| source.name == previous)
    {
        return Err(format!("{previous} is no longer available"));
    }

    pactl::set_default_source(previous)
}

/// The single quit path: stop playback, hand the default input back, tear down the
/// graph. Idempotent.
pub fn shutdown(app: &AppHandle) {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    log::info!("Shutting down");

    let settings = {
        let core = app.state::<Core>();
        core.player.stop_all();
        core.config().settings
    };

    if settings.use_as_default_mic {
        if let Some(previous) = settings.previous_default_source.as_deref() {
            if let Err(err) = restore_default_source(previous) {
                log::warn!("Could not restore the default input: {err}");
            }
        }
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
