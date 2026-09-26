//! Global hotkeys built on a read-only evdev listener.

pub mod dispatcher;
pub mod keymap;
pub mod listener;

use std::sync::mpsc::channel;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::commands;
use crate::hotkeys::dispatcher::{Action, Outcome};
use crate::permissions;
use crate::state::Core;

/// Starts the listener and dispatcher threads and publishes the input status.
///
/// Keys are read from every keyboard-like `/dev/input/event*` device and matched
/// against the bindings rebuilt from the config; nothing is ever grabbed, so the
/// key still reaches the focused application.
pub fn start(app: &AppHandle) {
    let core = app.state::<Core>();
    let dispatcher = Arc::clone(&core.dispatcher);
    dispatcher.rebuild(&core.config());

    let (sender, receiver) = channel::<listener::RawKey>();
    let handle = app.clone();

    // Matching and playback run off the reader threads: playing a sound spawns a
    // process and must never stall the key stream.
    std::thread::spawn(move || {
        for raw in receiver {
            match dispatcher.process(raw) {
                Outcome::Action(Action::Play(id)) => {
                    if let Err(err) = commands::play_by_id(&handle, &id) {
                        log::warn!("Hotkey playback failed: {err}");
                    }
                }
                Outcome::Action(Action::StopAll) => {
                    handle.state::<Core>().player.stop_all();
                }
                Outcome::Captured(binding) => emit(&handle, "key_captured", binding),
                Outcome::CaptureCancelled => emit(&handle, "key_capture_cancelled", ()),
                Outcome::Ignored => {}
            }
        }
    });

    let on_change: Arc<dyn Fn() + Send + Sync> = {
        let handle = app.clone();
        Arc::new(move || permissions::publish_input_status(&handle))
    };
    listener::spawn(sender, Arc::clone(&core.listener), on_change);

    permissions::publish_input_status(app);
}

/// Emits an event to the frontend, logging instead of failing.
pub fn emit<T: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(err) = app.emit(event, payload) {
        log::warn!("Could not emit {event}: {err}");
    }
}
