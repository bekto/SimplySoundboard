//! Tauri command handlers exposed to the frontend.

use tauri::{AppHandle, Manager, State};

use crate::audio;
use crate::config;
use crate::library::{self, ImportResult};
use crate::model::{
    AppState, MicDevice, RouterState, RouterStatus, Settings, SettingsPatch, Sound, SoundPatch,
};
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

/// Copies the given files into the sounds directory and appends cards for them.
#[tauri::command]
pub fn import_sounds(
    app: AppHandle,
    core: State<'_, Core>,
    paths: Vec<String>,
) -> Result<ImportResult, String> {
    let sounds_dir = config::sounds_dir()?;
    let start_index = core.config().sounds.len();
    let result = library::import_files(&paths, &sounds_dir, start_index);

    if let Err(err) = core.mutate_config(|config| {
        config.sounds.extend(result.added.iter().cloned());
        Ok(())
    }) {
        // Do not leave files behind that no card points at.
        library::cleanup_copied(&sounds_dir, &result.added);
        return Err(err);
    }

    on_sounds_changed(&app);
    Ok(result)
}

/// Applies a partial update to one card.
#[tauri::command]
pub fn update_sound(
    app: AppHandle,
    core: State<'_, Core>,
    id: String,
    patch: SoundPatch,
) -> Result<Sound, String> {
    let updated = core.mutate_config(|config| {
        let sound = config
            .sounds
            .iter_mut()
            .find(|sound| sound.id == id)
            .ok_or_else(|| "Sound not found".to_string())?;
        library::apply_patch(sound, &patch)?;
        Ok(sound.clone())
    })?;

    on_sounds_changed(&app);
    Ok(updated)
}

/// Removes a card and its stored file.
#[tauri::command]
pub fn delete_sound(app: AppHandle, core: State<'_, Core>, id: String) -> Result<(), String> {
    let file = core.mutate_config(|config| {
        let position = config
            .sounds
            .iter()
            .position(|sound| sound.id == id)
            .ok_or_else(|| "Sound not found".to_string())?;
        Ok(config.sounds.remove(position).file)
    })?;

    if let Ok(sounds_dir) = config::sounds_dir() {
        library::delete_file(&sounds_dir, &file);
    }

    on_sounds_changed(&app);
    Ok(())
}

/// Persists a new card order; `ids` must be a permutation of the current ids.
#[tauri::command]
pub fn reorder_sounds(
    app: AppHandle,
    core: State<'_, Core>,
    ids: Vec<String>,
) -> Result<(), String> {
    core.mutate_config(|config| library::reorder(&mut config.sounds, &ids))?;

    on_sounds_changed(&app);
    Ok(())
}

/// Hook called after every library mutation: rebuilds the hotkey bindings so a
/// new or changed key takes effect immediately.
pub fn on_sounds_changed(app: &AppHandle) {
    let core = app.state::<Core>();
    core.dispatcher.rebuild(&core.config());
}

/// Rebuilds the virtual microphone from scratch.
#[tauri::command]
pub fn restart_router(app: AppHandle) -> Result<RouterStatus, String> {
    audio::stop(&app);
    audio::start(&app)?;
    Ok(audio::status(&app))
}

/// Plays a sound into the virtual microphone. Shared by the `play_sound` command
/// and the hotkey dispatcher.
pub fn play_by_id(app: &AppHandle, id: &str) -> Result<(), String> {
    if audio::status(app).state != RouterState::Ok {
        return Err("Virtual mic is not running".to_string());
    }

    let core = app.state::<Core>();
    let config = core.config();
    let sound = config
        .sounds
        .iter()
        .find(|sound| sound.id == id)
        .ok_or_else(|| "Sound not found".to_string())?;
    let path = config::sounds_dir()?.join(&sound.file);

    core.player
        .play(app, sound, &path, config.settings.retrigger)
}

#[tauri::command]
pub fn play_sound(app: AppHandle, id: String) -> Result<(), String> {
    play_by_id(&app, &id)
}

#[tauri::command]
pub fn stop_sound(app: AppHandle, id: String) -> Result<(), String> {
    app.state::<Core>().player.stop(&id);
    Ok(())
}

#[tauri::command]
pub fn stop_all(app: AppHandle) -> Result<(), String> {
    app.state::<Core>().player.stop_all();
    Ok(())
}

/// Applies a partial settings update: persist first, then bring the live audio in
/// line. A failing device leaves the settings saved and reports the error.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    core: State<'_, Core>,
    patch: SettingsPatch,
) -> Result<Settings, String> {
    let before = core.config().settings;
    let captured_default = audio::capture_previous_default(&before, &patch);

    let after = core.mutate_config(|config| {
        config.settings.apply_patch(&patch)?;
        if let Some(previous) = captured_default.as_ref() {
            config.settings.previous_default_source = Some(previous.clone());
        }
        Ok(config.settings.clone())
    })?;

    let mut errors: Vec<String> = Vec::new();

    // Turning "use as default mic" off hands the saved source back.
    if patch.use_as_default_mic == Some(false) {
        if let Some(previous) = before.previous_default_source.as_deref() {
            if let Err(err) = audio::restore_default_source(previous) {
                errors.push(format!("default input: {err}"));
            }
        }
    }

    if let Err(err) = audio::apply_settings(&app, &after) {
        errors.push(err);
    }

    if patch.stop_all_hotkey.is_some() {
        on_sounds_changed(&app);
    }

    if errors.is_empty() {
        Ok(after)
    } else {
        Err(errors.join("; "))
    }
}

/// Real microphone devices for the settings picker.
#[tauri::command]
pub fn list_mics(app: AppHandle) -> Result<Vec<MicDevice>, String> {
    let settings = app.state::<Core>().config().settings;
    audio::list_mics(&settings)
}
