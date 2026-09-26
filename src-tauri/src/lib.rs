//! SimplySoundboard entry point: wires plugins, shared state, commands and lifecycle.

pub mod audio;
pub mod commands;
pub mod config;
pub mod hotkeys;
pub mod library;
pub mod model;
pub mod permissions;
pub mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::Core::new(config::load()))
        .invoke_handler(tauri::generate_handler![commands::get_state])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
