//! Tauri command handlers exposed to the frontend.

use tauri::State;

use crate::model::AppState;
use crate::state::{lock, Core};

/// Everything the frontend needs to render itself.
#[tauri::command]
pub fn get_state(core: State<'_, Core>) -> Result<AppState, String> {
    let config = core.config();
    Ok(AppState {
        sounds: config.sounds,
        settings: config.settings,
        router: lock(&core.router_status).clone(),
        input: lock(&core.input).clone(),
    })
}
