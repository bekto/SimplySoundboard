//! Keyboard (/dev/input) permission status and one-time udev setup.

use tauri::{AppHandle, Manager};

use crate::hotkeys::listener::Listener;
use crate::model::{InputState, InputStatus};
use crate::state::{lock, Core};

/// Whether global hotkeys can work right now, and which keyboards are being read.
pub fn input_status(listener: &Listener) -> InputStatus {
    let keyboards = listener.keyboards();

    let state = if !keyboards.is_empty() {
        InputState::Ok
    } else if listener.denied_paths().is_empty() {
        InputState::NoKeyboards
    } else {
        InputState::NoPermission
    };

    InputStatus { state, keyboards }
}

/// Stores the current status and, when it changed, tells the UI.
pub fn publish_input_status(app: &AppHandle) {
    let core = app.state::<Core>();
    let status = input_status(&core.listener);

    let changed = {
        let mut current = lock(&core.input);
        let changed = *current != status;
        *current = status.clone();
        changed
    };

    if changed {
        crate::hotkeys::emit(app, "input_status", status);
    }
}
