//! Serde data model shared with the frontend (see PLAN.md §5 and §6).

use serde::{Deserialize, Serialize};

/// Keyboard modifier. Serialized lowercase: `"ctrl"`, `"shift"`, `"alt"`, `"super"`.
///
/// Variant order is the canonical sort order used by [`KeyBinding::normalized`].
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Mod {
    Ctrl,
    Shift,
    Alt,
    Super,
}

/// A single key plus its required modifier set.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyBinding {
    /// evdev key code (`KeyCode` discriminant).
    pub code: u16,
    /// Required modifiers, canonical order (Ctrl, Shift, Alt, Super), no duplicates.
    pub mods: Vec<Mod>,
    /// Human readable label, e.g. `"Ctrl + Shift + F9"`.
    pub label: String,
}

impl KeyBinding {
    /// Sorts and de-duplicates [`KeyBinding::mods`] into canonical order.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.mods.sort();
        self.mods.dedup();
        self
    }
}

/// One soundboard card.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub id: String,
    pub name: String,
    /// File name relative to the sounds directory.
    pub file: String,
    pub emoji: String,
    pub color: String,
    /// 0.0–1.0.
    pub volume: f32,
    /// `None` when the duration probe failed.
    pub duration_ms: Option<u64>,
    pub hotkey: Option<KeyBinding>,
}

/// What happens when a sound is triggered while it is already playing.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Retrigger {
    /// Stop the running instance and start a fresh one.
    #[default]
    Restart,
    /// Let instances play on top of each other.
    Overlap,
    /// Stop the running instance and do not start a new one.
    Stop,
}

/// User settings. Missing fields fall back to [`Settings::default`].
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub mic_passthrough: bool,
    /// Real microphone source name; `None` means "follow the system default source".
    pub mic_source: Option<String>,
    pub mic_volume: f32,
    pub to_mic_volume: f32,
    pub monitor_enabled: bool,
    pub monitor_volume: f32,
    pub use_as_default_mic: bool,
    /// Source that was the system default before we took it over.
    pub previous_default_source: Option<String>,
    pub retrigger: Retrigger,
    pub stop_all_hotkey: Option<KeyBinding>,
    pub close_to_tray: bool,
    pub start_minimized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mic_passthrough: true,
            mic_source: None,
            mic_volume: 1.0,
            to_mic_volume: 1.0,
            monitor_enabled: true,
            monitor_volume: 0.6,
            use_as_default_mic: false,
            previous_default_source: None,
            retrigger: Retrigger::Restart,
            stop_all_hotkey: Some(KeyBinding {
                code: 82, // KEY_KP0
                mods: Vec::new(),
                label: "Num 0".to_string(),
            }),
            close_to_tray: true,
            start_minimized: false,
        }
    }
}

/// Persisted configuration (`config.json`).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
    pub sounds: Vec<Sound>,
    #[serde(default)]
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            sounds: Vec::new(),
            settings: Settings::default(),
        }
    }
}

/// Virtual-microphone pipeline state.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RouterState {
    Starting,
    Ok,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RouterStatus {
    pub state: RouterState,
    pub message: Option<String>,
}

impl Default for RouterStatus {
    fn default() -> Self {
        Self {
            state: RouterState::Starting,
            message: None,
        }
    }
}

/// Whether global hotkeys can work on this machine.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InputState {
    Ok,
    NoPermission,
    NoKeyboards,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    pub state: InputState,
    /// Names of the keyboard devices currently being read.
    pub keyboards: Vec<String>,
}

impl Default for InputStatus {
    fn default() -> Self {
        Self {
            state: InputState::NoKeyboards,
            keyboards: Vec::new(),
        }
    }
}

/// A recordable source device offered in the microphone picker.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MicDevice {
    pub name: String,
    pub description: String,
    pub is_default: bool,
}

/// Everything the frontend needs to render itself.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub sounds: Vec<Sound>,
    pub settings: Settings,
    pub router: RouterStatus,
    pub input: InputStatus,
}

/// Deserializes `Option<Option<T>>` so that a present `null` becomes `Some(None)`
/// while a missing field stays `None` (used together with `#[serde(default)]`).
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Partial update for a [`Sound`]. Missing field = unchanged, `null` hotkey = cleared.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SoundPatch {
    pub name: Option<String>,
    pub emoji: Option<String>,
    pub color: Option<String>,
    pub volume: Option<f32>,
    #[serde(default, deserialize_with = "double_option")]
    pub hotkey: Option<Option<KeyBinding>>,
}

/// Partial update for [`Settings`]. Same missing-vs-`null` rule as [`SoundPatch`].
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub mic_passthrough: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub mic_source: Option<Option<String>>,
    pub mic_volume: Option<f32>,
    pub to_mic_volume: Option<f32>,
    pub monitor_enabled: Option<bool>,
    pub monitor_volume: Option<f32>,
    pub use_as_default_mic: Option<bool>,
    pub retrigger: Option<Retrigger>,
    #[serde(default, deserialize_with = "double_option")]
    pub stop_all_hotkey: Option<Option<KeyBinding>>,
    pub close_to_tray: Option<bool>,
    pub start_minimized: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_json() {
        let config = Config::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: Config = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.version, config.version);
        assert!(back.sounds.is_empty());
        assert!(back.settings.mic_passthrough);
        assert!((back.settings.monitor_volume - 0.6).abs() < f32::EPSILON);
        assert_eq!(back.settings.retrigger, Retrigger::Restart);
        assert_eq!(
            back.settings
                .stop_all_hotkey
                .expect("default stop all key")
                .code,
            82
        );
    }

    #[test]
    fn missing_settings_fields_get_defaults() {
        let json = r#"{"version":1,"sounds":[],"settings":{"monitorEnabled":false}}"#;
        let config: Config = serde_json::from_str(json).expect("deserialize");

        assert!(!config.settings.monitor_enabled);
        assert!(config.settings.mic_passthrough);
        assert!((config.settings.to_mic_volume - 1.0).abs() < f32::EPSILON);
        assert!(config.settings.close_to_tray);
        assert_eq!(config.settings.retrigger, Retrigger::Restart);
    }

    #[test]
    fn json_field_names_are_camel_case() {
        let sound = Sound {
            id: "id".to_string(),
            name: "Airhorn".to_string(),
            file: "airhorn-3f2a91c0.ogg".to_string(),
            emoji: "📯".to_string(),
            color: "#f97316".to_string(),
            volume: 0.8,
            duration_ms: Some(2400),
            hotkey: None,
        };
        let json = serde_json::to_value(&sound).expect("serialize");
        assert_eq!(json["durationMs"], serde_json::json!(2400));

        let settings = serde_json::to_value(Settings::default()).expect("serialize settings");
        assert!(settings.get("stopAllHotkey").is_some());
        assert!(settings.get("stop_all_hotkey").is_none());
        assert_eq!(
            settings["stopAllHotkey"]["label"],
            serde_json::json!("Num 0")
        );

        let input = serde_json::to_value(InputStatus {
            state: InputState::NoPermission,
            keyboards: Vec::new(),
        })
        .expect("serialize input status");
        assert_eq!(input["state"], serde_json::json!("noPermission"));
    }

    #[test]
    fn sound_patch_distinguishes_missing_from_null_hotkey() {
        let missing: SoundPatch = serde_json::from_str(r#"{"name":"x"}"#).expect("missing");
        assert!(missing.hotkey.is_none());
        assert_eq!(missing.name.as_deref(), Some("x"));

        let cleared: SoundPatch = serde_json::from_str(r#"{"hotkey":null}"#).expect("null");
        assert_eq!(cleared.hotkey, Some(None));

        let set: SoundPatch =
            serde_json::from_str(r#"{"hotkey":{"code":82,"mods":[],"label":"Num 0"}}"#)
                .expect("set");
        assert_eq!(set.hotkey.expect("outer").expect("inner").code, 82);
    }

    #[test]
    fn settings_patch_distinguishes_missing_from_null_mic_source() {
        let missing: SettingsPatch = serde_json::from_str(r#"{"micVolume":0.5}"#).expect("missing");
        assert!(missing.mic_source.is_none());

        let cleared: SettingsPatch = serde_json::from_str(r#"{"micSource":null}"#).expect("null");
        assert_eq!(cleared.mic_source, Some(None));

        let set: SettingsPatch =
            serde_json::from_str(r#"{"micSource":"alsa_input.pci-0000_00_1f.3.analog-stereo"}"#)
                .expect("set");
        assert_eq!(
            set.mic_source.expect("outer").as_deref(),
            Some("alsa_input.pci-0000_00_1f.3.analog-stereo")
        );
    }

    #[test]
    fn key_binding_normalizes_modifiers() {
        let binding = KeyBinding {
            code: 79,
            mods: vec![Mod::Shift, Mod::Ctrl, Mod::Shift],
            label: "Ctrl + Shift + F9".to_string(),
        };
        assert_eq!(binding.normalized().mods, vec![Mod::Ctrl, Mod::Shift]);
    }
}
