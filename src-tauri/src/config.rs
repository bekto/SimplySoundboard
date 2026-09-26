//! Config file and sounds directory paths, with safe load/save.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::Config;

const APP_DIR: &str = "simplysoundboard";
const CONFIG_FILE: &str = "config.json";
const SOUNDS_DIR: &str = "sounds";

/// `~/.config/simplysoundboard`
pub fn config_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join(APP_DIR))
        .ok_or_else(|| "Could not determine the config directory".to_string())
}

/// `~/.config/simplysoundboard/config.json`
pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

/// `~/.local/share/simplysoundboard/sounds`, created if missing.
pub fn sounds_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .ok_or_else(|| "Could not determine the data directory".to_string())?
        .join(APP_DIR)
        .join(SOUNDS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    Ok(dir)
}

/// Loads the user config. A missing or unreadable file yields defaults; a corrupt
/// file is moved to `config.json.bak` and defaults are returned.
pub fn load() -> Config {
    match config_path() {
        Ok(path) => load_from(&path),
        Err(err) => {
            log::warn!("{err} — using default config");
            Config::default()
        }
    }
}

/// [`load`] against an explicit config file path.
pub fn load_from(path: &Path) -> Config {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Config::default(),
        Err(err) => {
            log::warn!(
                "Could not read {} ({err}) — using default config",
                path.display()
            );
            return Config::default();
        }
    };

    match serde_json::from_str(&raw) {
        Ok(config) => config,
        Err(err) => {
            let backup = path.with_extension("json.bak");
            log::warn!(
                "{} is not valid config ({err}) — moved to {}, using default config",
                path.display(),
                backup.display()
            );
            if let Err(err) = std::fs::rename(path, &backup) {
                log::warn!("Could not back up {}: {err}", path.display());
            }
            Config::default()
        }
    }
}

/// Atomically writes the user config (`config.json.tmp` → rename).
pub fn save(config: &Config) -> Result<(), String> {
    save_to(&config_path()?, config)
}

/// [`save`] against an explicit config file path.
pub fn save_to(path: &Path, config: &Config) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|err| format!("Could not serialize config: {err}"))?;

    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp)
        .map_err(|err| format!("Could not write {}: {err}", tmp.display()))?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|err| format!("Could not write {}: {err}", tmp.display()))?;
    drop(file);

    std::fs::rename(&tmp, path).map_err(|err| format!("Could not save config: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyBinding, Retrigger, Sound};

    fn sample_config() -> Config {
        let mut config = Config::default();
        config.sounds.push(Sound {
            id: "b3f0".to_string(),
            name: "Airhorn".to_string(),
            file: "airhorn-3f2a91c0.ogg".to_string(),
            emoji: "📯".to_string(),
            color: "#f97316".to_string(),
            volume: 0.8,
            duration_ms: Some(2400),
            hotkey: Some(KeyBinding {
                code: 79,
                mods: Vec::new(),
                label: "Num 1".to_string(),
            }),
        });
        config.settings.monitor_enabled = false;
        config.settings.retrigger = Retrigger::Overlap;
        config
    }

    #[test]
    fn config_round_trips_through_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.json");

        save_to(&path, &sample_config()).expect("save");
        let loaded = load_from(&path);

        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        assert_eq!(loaded.sounds.len(), 1);
        let sound = &loaded.sounds[0];
        assert_eq!(sound.name, "Airhorn");
        assert_eq!(sound.duration_ms, Some(2400));
        assert_eq!(sound.hotkey.as_ref().expect("hotkey").label, "Num 1");
        assert!(!loaded.settings.monitor_enabled);
        assert_eq!(loaded.settings.retrigger, Retrigger::Overlap);
    }

    #[test]
    fn missing_file_yields_default_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let loaded = load_from(&dir.path().join("nope.json"));
        assert_eq!(loaded.version, 1);
        assert!(loaded.sounds.is_empty());
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaulted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json at all").expect("write");

        let loaded = load_from(&path);

        assert!(!path.exists(), "corrupt file should be moved away");
        let backup = path.with_extension("json.bak");
        assert!(backup.exists());
        assert_eq!(
            std::fs::read_to_string(&backup).expect("read backup"),
            "{ not json at all"
        );
        assert!(loaded.sounds.is_empty());
        assert!(loaded.settings.mic_passthrough);
    }

    #[test]
    fn missing_settings_key_in_file_uses_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"version":1,"sounds":[],"settings":{"toMicVolume":0.25}}"#,
        )
        .expect("write");

        let loaded = load_from(&path);

        assert!((loaded.settings.to_mic_volume - 0.25).abs() < f32::EPSILON);
        assert!(loaded.settings.mic_passthrough);
        assert!(loaded.settings.stop_all_hotkey.is_some());
    }
}
