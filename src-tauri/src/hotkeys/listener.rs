//! Enumerates keyboard devices and streams raw key events.
//!
//! Hotkeys read `/dev/input/event*` directly, which is the only way to see keys
//! while a game or another Wayland compositor has focus. Devices are never
//! grabbed, so the key still reaches whatever the user is typing into.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use evdev::{Device, KeyCode};

use crate::state::lock;

/// A key transition exactly as the device reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawKey {
    /// evdev key code, matching [`KeyCode`].
    pub code: u16,
    /// evdev value: 0 = released, 1 = pressed, 2 = autorepeat.
    pub value: i32,
}

/// How often `/dev/input` is rescanned for hotplugged keyboards.
const RESCAN_INTERVAL: Duration = Duration::from_secs(3);
const DEVICE_DIR: &str = "/dev/input";

/// What the listener currently sees. Shared with the UI through `Core`.
#[derive(Default)]
pub struct Listener {
    /// Opened keyboards: device path -> device name.
    open: std::sync::Mutex<Vec<(PathBuf, String)>>,
    /// Keyboard-like paths that exist but could not be opened (no permission).
    denied: std::sync::Mutex<Vec<PathBuf>>,
    /// Wakes the manager thread for an immediate rescan.
    wake: std::sync::Mutex<Option<Sender<()>>>,
}

impl Listener {
    /// Names of the keyboards currently being read.
    pub fn keyboards(&self) -> Vec<String> {
        let mut names: Vec<String> = lock(&self.open)
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Paths that looked like keyboards but were not readable.
    pub fn denied_paths(&self) -> Vec<PathBuf> {
        lock(&self.denied).clone()
    }

    /// Asks the manager thread to rescan `/dev/input` right now (used after the
    /// udev rule was installed, so the UI does not have to wait up to 3 s).
    pub fn rescan_now(&self) {
        let wake = lock(&self.wake);
        if let Some(sender) = wake.as_ref() {
            let _ = sender.send(());
        }
    }

    fn set_wake(&self, sender: Sender<()>) {
        *lock(&self.wake) = Some(sender);
    }

    fn is_open(&self, path: &Path) -> bool {
        lock(&self.open).iter().any(|(open, _)| open == path)
    }

    pub(crate) fn note_opened(&self, path: &Path, name: &str) {
        let mut open = lock(&self.open);
        if !open.iter().any(|(existing, _)| existing == path) {
            open.push((path.to_path_buf(), name.to_string()));
        }
    }

    pub(crate) fn note_closed(&self, path: &Path) {
        lock(&self.open).retain(|(open, _)| open != path);
    }

    pub(crate) fn note_denied(&self, path: &Path) {
        let mut denied = lock(&self.denied);
        if !denied.iter().any(|existing| existing == path) {
            denied.push(path.to_path_buf());
        }
    }

    pub(crate) fn clear_denied(&self, path: &Path) {
        lock(&self.denied).retain(|denied| denied != path);
    }
}

/// True for devices with keyboard-like keys: letters, numpad digits or function keys.
pub fn is_keyboard(device: &Device) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };

    keys.contains(KeyCode::KEY_A)
        || keys.contains(KeyCode::KEY_KP0)
        || keys.contains(KeyCode::KEY_F1)
}

/// Spawns the manager thread: rescans `/dev/input` every 3 s, opening every new
/// keyboard-like device on its own reader thread. `on_change` is called after each
/// scan that altered the set of readable devices.
pub fn spawn(
    keys: Sender<RawKey>,
    listener: Arc<Listener>,
    on_change: Arc<dyn Fn() + Send + Sync>,
) {
    let (wake_sender, wake_receiver) = std::sync::mpsc::channel::<()>();
    listener.set_wake(wake_sender);

    std::thread::spawn(move || loop {
        if scan_once(&keys, &listener) {
            on_change();
        }
        // Sleep until the next periodic scan or an explicit rescan request.
        if wake_receiver.recv_timeout(RESCAN_INTERVAL).is_ok() {
            log::debug!("Rescanning /dev/input on request");
        }
    });
}

/// One pass over `/dev/input/event*`. Returns true when the listener state changed.
fn scan_once(keys: &Sender<RawKey>, listener: &Arc<Listener>) -> bool {
    let mut changed = false;

    for path in event_paths() {
        if listener.is_open(&path) {
            continue;
        }

        match Device::open(&path) {
            Ok(device) => {
                if !is_keyboard(&device) {
                    continue;
                }
                let name = device.name().unwrap_or("keyboard").to_string();
                listener.clear_denied(&path);
                listener.note_opened(&path, &name);
                changed = true;
                log::info!("Reading keys from {} ({name})", path.display());
                spawn_reader(path, device, keys.clone(), Arc::clone(listener));
            }
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                // Without permission we cannot inspect the device, so classify it
                // through sysfs. A denied mouse must not look like a missing rule.
                if looks_like_keyboard(&path) && !listener.denied_paths().contains(&path) {
                    log::debug!("No permission for {}", path.display());
                    listener.note_denied(&path);
                    changed = true;
                }
            }
            Err(err) => log::debug!("Could not open {}: {err}", path.display()),
        }
    }

    changed
}

/// Blocking reader for one device; exits when the device disappears.
fn spawn_reader(path: PathBuf, mut device: Device, keys: Sender<RawKey>, listener: Arc<Listener>) {
    std::thread::spawn(move || {
        loop {
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(err) => {
                    log::info!("Stopped reading {}: {err}", path.display());
                    break;
                }
            };

            for event in events {
                if event.event_type() != evdev::EventType::KEY {
                    continue;
                }
                let raw = RawKey {
                    code: event.code(),
                    value: event.value(),
                };
                if keys.send(raw).is_err() {
                    // The dispatcher is gone; the app is shutting down.
                    return;
                }
            }
        }

        listener.note_closed(&path);
    });
}

fn event_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(DEVICE_DIR)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("event"))
                })
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

/// Reads `KEY_A`/`KEY_KP0`/`KEY_F1` out of the sysfs capability bitmask, which
/// needs no privileges.
fn looks_like_keyboard(path: &Path) -> bool {
    let Some(event_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let capabilities = Path::new("/sys/class/input")
        .join(event_name)
        .join("device/capabilities/key");

    let Ok(mask) = std::fs::read_to_string(&capabilities) else {
        return true; // Cannot tell; treat as a keyboard and let the UI offer setup.
    };

    mask_has_keyboard_bits(&mask)
}

/// True when the capability bitmask advertises a letter, numpad digit or F1.
///
/// The kernel prints the mask most-significant word first, so key codes 0..63 live
/// in the last word while codes 64.. live in the first.
fn mask_has_keyboard_bits(mask: &str) -> bool {
    let words: Vec<u64> = mask
        .split_whitespace()
        .map(|word| u64::from_str_radix(word, 16))
        .collect::<Result<Vec<u64>, _>>()
        .unwrap_or_default();

    if words.is_empty() {
        // Unreadable or empty mask: assume a keyboard rather than hiding the setup.
        return true;
    }

    // words is most-significant first: the last word holds codes 0..63.
    let word_count = words.len();
    let bit_set = |code: u16| -> bool {
        // Masks shorter than the code's word simply do not advertise the key.
        let Some(word_index) = (word_count - 1).checked_sub(usize::from(code / 64)) else {
            return false;
        };
        words
            .get(word_index)
            .is_some_and(|word| (word >> (code % 64)) & 1 == 1)
    };

    [KeyCode::KEY_A.0, KeyCode::KEY_KP0.0, KeyCode::KEY_F1.0]
        .into_iter()
        .any(bit_set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_paths_only_lists_event_nodes() {
        let paths = event_paths();
        assert!(
            paths.iter().all(|path| path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("event")),
            "unexpected paths: {paths:?}"
        );
    }

    #[test]
    fn listener_reports_open_and_denied_devices() {
        let listener = Listener::default();
        assert!(listener.keyboards().is_empty());

        listener.note_opened(Path::new("/dev/input/event3"), "Gaming Keyboard");
        listener.note_opened(Path::new("/dev/input/event8"), "Logitech PRO X");
        listener.note_opened(Path::new("/dev/input/event3"), "Gaming Keyboard");
        assert_eq!(listener.keyboards().len(), 2, "duplicates are ignored");

        listener.note_denied(Path::new("/dev/input/event4"));
        listener.note_denied(Path::new("/dev/input/event4"));
        assert_eq!(listener.denied_paths().len(), 1);

        listener.note_closed(Path::new("/dev/input/event3"));
        assert_eq!(listener.keyboards(), vec!["Logitech PRO X".to_string()]);

        listener.clear_denied(Path::new("/dev/input/event4"));
        assert!(listener.denied_paths().is_empty());
    }

    #[test]
    fn a_real_scan_yields_a_consistent_input_status() {
        // Exercises the real /dev/input scan of this machine: either keyboards were
        // opened, or keyboards were denied, or there are none at all.
        let listener = Arc::new(Listener::default());
        let (sender, _receiver) = std::sync::mpsc::channel();
        scan_once(&sender, &listener);

        let status = crate::permissions::input_status(&listener);
        eprintln!(
            "input status: {:?} keyboards={:?} denied={:?}",
            status.state,
            status.keyboards,
            listener.denied_paths()
        );

        match status.state {
            crate::model::InputState::Ok => assert!(!status.keyboards.is_empty()),
            crate::model::InputState::NoPermission => assert!(!listener.denied_paths().is_empty()),
            crate::model::InputState::NoKeyboards => {
                assert!(status.keyboards.is_empty() && listener.denied_paths().is_empty());
            }
        }
    }

    #[test]
    fn capability_mask_classifies_keyboards_from_sysfs() {
        // KEY_A = 30 -> bit 30 of the last (low) word.
        assert!(mask_has_keyboard_bits("0 40000000"));
        // KEY_F1 = 59 -> bit 59 of the low word.
        assert!(mask_has_keyboard_bits("0 800000000000000"));
        // KEY_KP0 = 82 -> bit 18 of the second word.
        assert!(mask_has_keyboard_bits("40000 0"));
        // Button/jack style masks carry neither.
        assert!(!mask_has_keyboard_bits("10000000000000 0"));
        assert!(!mask_has_keyboard_bits("0 0"));
        // Short masks must not panic when asked about higher key codes.
        assert!(!mask_has_keyboard_bits("1"));
        assert!(!mask_has_keyboard_bits("0"));
        // An unparseable or absent mask must not hide the permission setup.
        assert!(mask_has_keyboard_bits(""));
        assert!(mask_has_keyboard_bits("nonsense"));
    }
}
