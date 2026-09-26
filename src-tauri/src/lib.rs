//! SimplySoundboard entry point: wires plugins, shared state, commands and lifecycle.

pub mod audio;
pub mod commands;
pub mod config;
pub mod hotkeys;
pub mod library;
pub mod model;
pub mod permissions;
pub mod state;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

/// Id of the tray icon; also used to tell whether a tray is actually available.
const TRAY_ID: &str = "simplysoundboard";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        // Must be the first plugin: a second launch forwards to the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            log::info!("Another instance was started; focusing the existing window");
            show_main_window(app);
        }))
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
            commands::stop_all,
            commands::update_settings,
            commands::list_mics,
            commands::begin_key_capture,
            commands::cancel_key_capture,
            commands::setup_input_access
        ])
        .setup(|app| {
            install_signal_handler(app.handle().clone());

            // Global hotkeys need their own threads; input access problems surface
            // through the input status rather than blocking startup.
            hotkeys::start(app.handle());

            let tray_available = match setup_tray(app.handle()) {
                Ok(()) => true,
                Err(err) => {
                    // GNOME without an AppIndicator extension has no tray at all;
                    // then the app must behave like a normal window.
                    log::warn!("Tray icon unavailable: {err}");
                    false
                }
            };

            let start_minimized = app.state::<state::Core>().config().settings.start_minimized;
            if start_minimized && tray_available {
                log::info!("Starting minimized: use the tray icon to open the window");
            } else if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }

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
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if should_hide_instead_of_quit(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                    log::info!("Window hidden; the app keeps running in the tray");
                } else {
                    shutdown(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                audio::shutdown(app);
            }
        });
}

/// True when closing the window should only hide it: the setting is on and a tray
/// exists to bring it back.
fn should_hide_instead_of_quit(app: &AppHandle) -> bool {
    app.state::<state::Core>().config().settings.close_to_tray && tray_available(app)
}

/// Whether a tray icon was created on this desktop.
fn tray_available(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

/// Creates the tray icon with its menu. Errors on desktops without a status area.
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show SimplySoundboard", true, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, "stop_all", "Stop all sounds", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &stop_all,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SimplySoundboard")
        .menu(&menu)
        // Left click toggles the window, so the menu belongs on right click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "stop_all" => {
                let core = app.state::<state::Core>();
                core.player.stop_all();
            }
            "quit" => shutdown(app),
            other => log::debug!("Unhandled tray menu item {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Shows the main window and gives it focus.
fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Tray left click: hide a visible window, restore a hidden one.
fn toggle_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show_main_window(app);
    }
}

/// The single quit path: stop playback, hand back the default input, tear down the
/// virtual devices, then exit. Idempotent, so it is safe from any entry point.
pub fn shutdown(app: &AppHandle) {
    audio::shutdown(app);
    app.exit(0);
}

/// Ctrl+C (or SIGTERM) in a terminal must tear the audio graph down like a normal
/// quit, otherwise the virtual devices would outlive the app.
fn install_signal_handler(handle: AppHandle) {
    if let Err(err) = ctrlc::set_handler(move || {
        shutdown(&handle);
    }) {
        log::warn!("Could not install the signal handler: {err}");
    }
}
