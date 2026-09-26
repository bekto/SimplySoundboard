//! Sound library: import, edit, reorder and delete sounds.
//!
//! Everything here is a plain function over paths/config so it can be unit tested
//! against a temp directory; `commands.rs` only glues it to Tauri.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::{Sound, SoundPatch};

/// Extensions the app accepts, case-insensitive.
pub const ACCEPTED_EXTENSIONS: [&str; 6] = ["wav", "ogg", "oga", "opus", "flac", "mp3"];

/// Card colors, cycled by sound count on import.
pub const COLORS: [&str; 8] = [
    "#7c5cff", "#ec4899", "#f97316", "#eab308", "#22c55e", "#06b6d4", "#3b82f6", "#ef4444",
];

const SLUG_MAX: usize = 40;
const NAME_MAX: usize = 60;

/// Outcome of an import batch.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: Vec<Sound>,
    /// File names that were skipped, in input order.
    pub rejected: Vec<String>,
}

/// Lowercase `[a-z0-9-]` slug of a file stem, collapsed and capped at 40 chars.
pub fn slug(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut last_was_dash = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let capped: String = trimmed.chars().take(SLUG_MAX).collect();
    let capped = capped.trim_end_matches('-').to_string();
    if capped.is_empty() {
        "sound".to_string()
    } else {
        capped
    }
}

/// Human readable default name: separators become spaces, capped at 60 chars.
pub fn display_name(stem: &str) -> String {
    let cleaned = stem.replace(['_', '-'], " ");
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(NAME_MAX).collect()
}

/// `slug-<8 hex>.<ext>` — unique on disk, extension preserved.
pub fn target_file_name(stem: &str, ext: &str) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("{}-{}.{}", slug(stem), &id[..8], ext)
}

/// `Some(ms)` when the container/codec reports a frame count and sample rate.
pub fn probe_duration_ms(path: &Path) -> Option<u64> {
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &Default::default(), &Default::default())
        .ok()?;
    let params = &probed.format.default_track()?.codec_params;

    let frames = params.n_frames?;
    let sample_rate = params.sample_rate?;
    if frames == 0 || sample_rate == 0 {
        return None;
    }
    Some(frames.saturating_mul(1000) / u64::from(sample_rate))
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| ACCEPTED_EXTENSIONS.contains(&ext.as_str()))
}

/// Copies every acceptable file into `sounds_dir` and returns the cards to add.
///
/// Color assignment starts at `start_index`, so the palette keeps cycling across
/// repeated imports. Unreadable/non-audio inputs land in `rejected` as file names.
pub fn import_files(paths: &[String], sounds_dir: &Path, start_index: usize) -> ImportResult {
    let mut result = ImportResult::default();

    if let Err(err) = std::fs::create_dir_all(sounds_dir) {
        log::warn!("Could not create {}: {err}", sounds_dir.display());
    }

    for raw in paths {
        let source = PathBuf::from(raw);
        let label = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| raw.clone());

        if !source.is_file() {
            result.rejected.push(label);
            continue;
        }

        let Some(ext) = extension_of(&source) else {
            result.rejected.push(label);
            continue;
        };

        let Some(stem) = source.file_stem().and_then(|stem| stem.to_str()) else {
            result.rejected.push(label);
            continue;
        };

        let file = target_file_name(stem, &ext);
        let target = sounds_dir.join(&file);
        if let Err(err) = std::fs::copy(&source, &target) {
            log::warn!("Could not copy {}: {err}", source.display());
            result.rejected.push(label);
            continue;
        }

        let color = COLORS[(start_index + result.added.len()) % COLORS.len()].to_string();
        result.added.push(Sound {
            id: uuid::Uuid::new_v4().to_string(),
            name: display_name(stem),
            file,
            emoji: "🔊".to_string(),
            color,
            volume: 1.0,
            duration_ms: probe_duration_ms(&target),
            hotkey: None,
        });
    }

    result
}

/// Applies a [`SoundPatch`]: only present fields change, volume clamps to 0..=1,
/// names are trimmed and must not be empty, hotkeys are normalized.
pub fn apply_patch(sound: &mut Sound, patch: &SoundPatch) -> Result<(), String> {
    if let Some(name) = patch.name.as_deref() {
        let name = name.trim();
        if name.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        sound.name = name.chars().take(NAME_MAX).collect();
    }
    if let Some(emoji) = patch.emoji.as_deref() {
        sound.emoji = emoji.to_string();
    }
    if let Some(color) = patch.color.as_deref() {
        sound.color = color.to_string();
    }
    if let Some(volume) = patch.volume {
        sound.volume = volume.clamp(0.0, 1.0);
    }
    match &patch.hotkey {
        Some(Some(binding)) => sound.hotkey = Some(binding.clone().normalized()),
        Some(None) => sound.hotkey = None,
        None => {}
    }
    Ok(())
}

/// Reorders `sounds` to match `ids`. `ids` must be a permutation of the current ids.
pub fn reorder(sounds: &mut Vec<Sound>, ids: &[String]) -> Result<(), String> {
    if ids.len() != sounds.len() {
        return Err("Invalid order".to_string());
    }

    let mut ordered: Vec<Sound> = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(position) = sounds.iter().position(|sound| &sound.id == id) else {
            return Err("Invalid order".to_string());
        };
        ordered.push(sounds.remove(position));
    }
    *sounds = ordered;
    Ok(())
}

/// Removes a stored sound file; a missing file is not an error.
pub fn delete_file(sounds_dir: &Path, file: &str) {
    let path = sounds_dir.join(file);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("Could not delete {}: {err}", path.display()),
    }
}

/// Removes files that were copied before the config write failed.
pub fn cleanup_copied(sounds_dir: &Path, sounds: &[Sound]) {
    for sound in sounds {
        delete_file(sounds_dir, &sound.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal 16-bit mono 8 kHz PCM wav with `frames` samples.
    fn write_wav(path: &Path, frames: u32) {
        let data_len = frames * 2;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + data_len as usize, 0);
        std::fs::write(path, bytes).expect("write wav");
    }

    #[test]
    fn slug_sanitizes_and_caps() {
        assert_eq!(slug("Air Horn!!"), "air-horn");
        assert_eq!(slug("my_sound__v2"), "my-sound-v2");
        assert_eq!(slug("---"), "sound");
        assert_eq!(slug(""), "sound");
        assert_eq!(slug(&"a".repeat(80)).len(), 40);
    }

    #[test]
    fn display_name_replaces_separators() {
        assert_eq!(display_name("my_cool-sound"), "my cool sound");
        assert_eq!(display_name("  spaced  out  "), "spaced out");
        assert_eq!(display_name(&"x".repeat(100)).chars().count(), 60);
    }

    #[test]
    fn import_accepts_audio_and_rejects_everything_else() {
        let source = tempfile::tempdir().expect("source dir");
        let dest = tempfile::tempdir().expect("dest dir");

        write_wav(&source.path().join("Air Horn.WAV"), 8000);
        std::fs::write(source.path().join("notes.txt"), "nope").expect("write txt");

        let paths = vec![
            source
                .path()
                .join("Air Horn.WAV")
                .to_string_lossy()
                .into_owned(),
            source
                .path()
                .join("notes.txt")
                .to_string_lossy()
                .into_owned(),
            source
                .path()
                .join("missing.mp3")
                .to_string_lossy()
                .into_owned(),
        ];

        let result = import_files(&paths, dest.path(), 0);

        assert_eq!(result.added.len(), 1);
        assert_eq!(result.rejected, vec!["notes.txt", "missing.mp3"]);

        let sound = &result.added[0];
        assert_eq!(sound.name, "Air Horn");
        assert_eq!(sound.emoji, "🔊");
        assert_eq!(sound.color, COLORS[0]);
        assert!((sound.volume - 1.0).abs() < f32::EPSILON);
        assert!(sound.hotkey.is_none());

        let file = dest.path().join(&sound.file);
        assert!(file.exists(), "file should be copied next to the config");
        assert!(sound.file.starts_with("air-horn-"));
        assert!(sound.file.ends_with(".wav"));
        assert_eq!(sound.duration_ms, Some(1000));
    }

    #[test]
    fn import_cycles_colors_and_never_overwrites() {
        let source = tempfile::tempdir().expect("source dir");
        let dest = tempfile::tempdir().expect("dest dir");
        write_wav(&source.path().join("a.wav"), 800);
        write_wav(&source.path().join("b.wav"), 800);

        let paths = vec![
            source.path().join("a.wav").to_string_lossy().into_owned(),
            source.path().join("a.wav").to_string_lossy().into_owned(),
            source.path().join("b.wav").to_string_lossy().into_owned(),
        ];
        let result = import_files(&paths, dest.path(), 7);

        let colors: Vec<&str> = result
            .added
            .iter()
            .map(|sound| sound.color.as_str())
            .collect();
        assert_eq!(colors, vec![COLORS[7], COLORS[0], COLORS[1]]);

        let files: std::collections::HashSet<&str> = result
            .added
            .iter()
            .map(|sound| sound.file.as_str())
            .collect();
        assert_eq!(files.len(), 3, "duplicate source files must not collide");
        assert_eq!(result.added[0].duration_ms, Some(100));
    }

    #[test]
    fn patch_only_touches_present_fields() {
        let mut sound = sample_sound();

        let patch: SoundPatch = serde_json::from_str(r#"{"volume":2.5}"#).expect("patch");
        apply_patch(&mut sound, &patch).expect("apply");

        assert!(
            (sound.volume - 1.0).abs() < f32::EPSILON,
            "volume clamps to 1"
        );
        assert_eq!(sound.name, "Airhorn");
        assert!(sound.hotkey.is_some(), "missing hotkey leaves the binding");

        let clear: SoundPatch =
            serde_json::from_str(r#"{"hotkey":null,"name":"  Horn  "}"#).expect("patch");
        apply_patch(&mut sound, &clear).expect("apply");
        assert!(sound.hotkey.is_none());
        assert_eq!(sound.name, "Horn");

        let empty: SoundPatch = serde_json::from_str(r#"{"name":"   "}"#).expect("patch");
        assert!(apply_patch(&mut sound, &empty).is_err());
        assert_eq!(sound.name, "Horn");
    }

    #[test]
    fn patch_normalizes_hotkey_modifiers() {
        let mut sound = sample_sound();
        let patch: SoundPatch = serde_json::from_str(
            r#"{"hotkey":{"code":79,"mods":["shift","ctrl","shift"],"label":"Ctrl + Shift + F9"}}"#,
        )
        .expect("patch");

        apply_patch(&mut sound, &patch).expect("apply");

        assert_eq!(sound.hotkey.expect("hotkey").mods.len(), 2);
    }

    #[test]
    fn reorder_rejects_non_permutations() {
        let mut sounds = vec![sample_sound(), sample_sound()];
        let ids: Vec<String> = sounds.iter().map(|sound| sound.id.clone()).collect();

        assert!(reorder(&mut sounds, &ids[..1]).is_err());
        assert!(reorder(&mut sounds, &["bogus".to_string(), ids[1].clone()]).is_err());
        assert_eq!(sounds[0].id, ids[0], "failed reorder must not mutate");

        let reversed = vec![ids[1].clone(), ids[0].clone()];
        reorder(&mut sounds, &reversed).expect("reorder");
        assert_eq!(sounds[0].id, ids[1]);
    }

    #[test]
    fn delete_file_ignores_missing_file() {
        let dir = tempfile::tempdir().expect("dir");
        write_wav(&dir.path().join("gone.wav"), 80);

        delete_file(dir.path(), "gone.wav");
        assert!(!dir.path().join("gone.wav").exists());
        delete_file(dir.path(), "gone.wav");
    }

    fn sample_sound() -> Sound {
        Sound {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Airhorn".to_string(),
            file: "airhorn-3f2a91c0.ogg".to_string(),
            emoji: "📯".to_string(),
            color: "#f97316".to_string(),
            volume: 0.8,
            duration_ms: Some(2400),
            hotkey: Some(crate::model::KeyBinding {
                code: 79,
                mods: Vec::new(),
                label: "Num 1".to_string(),
            }),
        }
    }
}
