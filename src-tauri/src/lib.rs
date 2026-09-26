//! SimplySoundboard entry point: wires plugins, shared state, commands and lifecycle.

pub mod audio;
pub mod commands;
pub mod config;
pub mod hotkeys;
pub mod library;
pub mod model;
pub mod permissions;
pub mod state;

use tauri::RunEvent;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::Core::new(config::load()))
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::import_sounds,
            commands::update_sound,
            commands::delete_sound,
            commands::reorder_sounds,
            commands::restart_router,
            commands::play_sound,
            commands::stop_sound,
            commands::stop_all
        ])
        .setup(|app| {
            install_signal_handler(app.handle().clone());

            // Building the audio graph shells out to pactl/pw-play, so never block
            // the main thread on it: failures surface through the router status.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if let Err(err) = audio::start(&handle) {
                    log::error!("Audio graph unavailable: {err}");
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                audio::shutdown(app);
            }
        });
}

/// Ctrl+C (or SIGTERM) in a terminal must tear the audio graph down like a normal
/// quit, otherwise the virtual devices would outlive the app.
fn install_signal_handler(handle: tauri::AppHandle) {
    if let Err(err) = ctrlc::set_handler(move || {
        audio::shutdown(&handle);
        handle.exit(0);
    }) {
        log::warn!("Could not install the signal handler: {err}");
    }
}
