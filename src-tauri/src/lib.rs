//! SimplySoundboard entry point: wires plugins, shared state, commands and lifecycle.

mod audio;
mod commands;
mod config;
mod hotkeys;
mod library;
mod model;
mod permissions;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
