//! Matches key events against bindings and runs actions; also handles capture mode.
//!
//! The state machine ([`Dispatcher::process`]) is pure apart from its own locks, so
//! the whole matching/capture logic is unit tested by feeding [`RawKey`] sequences.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError, RwLock};

use evdev::KeyCode;

use crate::hotkeys::keymap;
use crate::hotkeys::listener::RawKey;
use crate::model::{Config, KeyBinding, Mod};

/// How long a capture may stay open before it is cancelled automatically.
pub const CAPTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// What a binding does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Play(String),
    StopAll,
}

/// Whether keys fire actions or are being recorded for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Capturing,
}

/// Result of feeding one key event into the state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Normal mode: run this action.
    Action(Action),
    /// Capture finished with a key; `None` means the user cleared the binding.
    Captured(Option<KeyBinding>),
    /// Capture cancelled with Esc.
    CaptureCancelled,
    /// Modifier bookkeeping or autorepeat: nothing to do.
    Ignored,
}

/// Key matcher and capture state, shared between the listener and the commands.
pub struct Dispatcher {
    /// Physical modifier keys currently held, so left/right merge into [`Mod`].
    pressed: Mutex<HashSet<u16>>,
    mode: Mutex<Mode>,
    bindings: RwLock<Vec<(KeyBinding, Action)>>,
    /// Bumped on every capture start/cancel so a stale timeout thread does nothing.
    capture_generation: std::sync::atomic::AtomicU64,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            pressed: Mutex::new(HashSet::new()),
            mode: Mutex::new(Mode::Normal),
            bindings: RwLock::new(Vec::new()),
            capture_generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Feeds one raw key event through the state machine.
    pub fn process(&self, raw: RawKey) -> Outcome {
        match raw.value {
            0 => {
                lock_mutex(&self.pressed).remove(&raw.code);
                Outcome::Ignored
            }
            1 => self.press(raw.code),
            // 2 = autorepeat, deliberately ignored so holding a key does not retrigger.
            _ => Outcome::Ignored,
        }
    }

    /// Current mode.
    pub fn mode(&self) -> Mode {
        *lock_mutex(&self.mode)
    }

    /// Enters capture mode and returns the generation token for the timeout check.
    pub fn begin_capture(&self) -> u64 {
        *lock_mutex(&self.mode) = Mode::Capturing;
        self.capture_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    /// Leaves capture mode.
    pub fn cancel_capture(&self) {
        *lock_mutex(&self.mode) = Mode::Normal;
        self.capture_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// True while `generation` is still the active capture, i.e. nothing happened
    /// since it started.
    pub fn capture_still_open(&self, generation: u64) -> bool {
        self.mode() == Mode::Capturing
            && self
                .capture_generation
                .load(std::sync::atomic::Ordering::SeqCst)
                == generation
    }

    /// Replaces the binding table from the config (sounds first, then stop-all).
    pub fn rebuild(&self, config: &Config) {
        let mut bindings: Vec<(KeyBinding, Action)> = config
            .sounds
            .iter()
            .filter_map(|sound| {
                sound
                    .hotkey
                    .clone()
                    .map(|hotkey| (hotkey.normalized(), Action::Play(sound.id.clone())))
            })
            .collect();

        if let Some(hotkey) = &config.settings.stop_all_hotkey {
            bindings.push((hotkey.clone().normalized(), Action::StopAll));
        }

        let mut table = self
            .bindings
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *table = bindings;
    }

    /// Number of active bindings (used by tests and diagnostics).
    pub fn binding_count(&self) -> usize {
        self.bindings
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn press(&self, code: u16) -> Outcome {
        if let Some(modifier) = keymap::mod_of(code) {
            lock_mutex(&self.pressed).insert(code);
            log::trace!("Modifier {modifier:?} down");
            return Outcome::Ignored;
        }

        let mods = self.current_mods();
        match self.mode() {
            Mode::Capturing => self.finish_capture(code, &mods),
            Mode::Normal => match self.find(code, &mods) {
                Some(action) => Outcome::Action(action),
                None => Outcome::Ignored,
            },
        }
    }

    /// Ends capture mode and reports what the UI should store.
    fn finish_capture(&self, code: u16, mods: &[Mod]) -> Outcome {
        self.cancel_capture();

        if mods.is_empty() && code == KeyCode::KEY_ESC.0 {
            return Outcome::CaptureCancelled;
        }
        if mods.is_empty() && code == KeyCode::KEY_BACKSPACE.0 {
            return Outcome::Captured(None);
        }

        let mut normalized = mods.to_vec();
        normalized.sort();
        normalized.dedup();

        Outcome::Captured(Some(KeyBinding {
            code,
            label: keymap::binding_label(&normalized, code),
            mods: normalized,
        }))
    }

    /// A binding matches when the key code and the modifier set are both equal.
    fn find(&self, code: u16, mods: &[Mod]) -> Option<Action> {
        let table = self.bindings.read().unwrap_or_else(PoisonError::into_inner);
        table
            .iter()
            .find(|(binding, _)| binding.code == code && binding.mods.as_slice() == mods)
            .map(|(_, action)| action.clone())
    }

    /// Modifier set derived from the held physical keys, in canonical order.
    fn current_mods(&self) -> Vec<Mod> {
        let mut mods: Vec<Mod> = lock_mutex(&self.pressed)
            .iter()
            .filter_map(|code| keymap::mod_of(*code))
            .collect();
        mods.sort();
        mods.dedup();
        mods
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Settings, Sound};

    const KEY_A: u16 = KeyCode::KEY_A.0;
    const KEY_F9: u16 = KeyCode::KEY_F9.0;
    const KEY_KP3: u16 = KeyCode::KEY_KP3.0;
    const KEY_KP5: u16 = KeyCode::KEY_KP5.0;
    const CTRL: u16 = KeyCode::KEY_LEFTCTRL.0;
    const RIGHT_CTRL: u16 = KeyCode::KEY_RIGHTCTRL.0;
    const SHIFT: u16 = KeyCode::KEY_LEFTSHIFT.0;
    const BACKSPACE: u16 = KeyCode::KEY_BACKSPACE.0;
    const ESC: u16 = KeyCode::KEY_ESC.0;

    fn down(code: u16) -> RawKey {
        RawKey { code, value: 1 }
    }
    fn up(code: u16) -> RawKey {
        RawKey { code, value: 0 }
    }
    fn repeat(code: u16) -> RawKey {
        RawKey { code, value: 2 }
    }

    fn dispatcher_with(bindings: Vec<(KeyBinding, Action)>) -> Dispatcher {
        let dispatcher = Dispatcher::new();
        *dispatcher
            .bindings
            .write()
            .unwrap_or_else(PoisonError::into_inner) = bindings;
        dispatcher
    }

    fn binding(code: u16, mods: Vec<Mod>) -> KeyBinding {
        KeyBinding {
            code,
            label: keymap::binding_label(&mods, code),
            mods,
        }
    }

    #[test]
    fn plain_key_fires_and_autorepeat_does_not() {
        let dispatcher = dispatcher_with(vec![(
            binding(KEY_KP3, Vec::new()),
            Action::Play("sound".to_string()),
        )]);

        assert_eq!(
            dispatcher.process(down(KEY_KP3)),
            Outcome::Action(Action::Play("sound".to_string()))
        );
        assert_eq!(dispatcher.process(repeat(KEY_KP3)), Outcome::Ignored);
        assert_eq!(dispatcher.process(up(KEY_KP3)), Outcome::Ignored);
        assert_eq!(
            dispatcher.process(down(KEY_KP3)),
            Outcome::Action(Action::Play("sound".to_string()))
        );
    }

    #[test]
    fn modifier_combinations_must_match_exactly() {
        let dispatcher = dispatcher_with(vec![
            (
                binding(KEY_F9, vec![Mod::Ctrl]),
                Action::Play("with-ctrl".to_string()),
            ),
            (
                binding(KEY_F9, Vec::new()),
                Action::Play("plain".to_string()),
            ),
        ]);

        assert_eq!(
            dispatcher.process(down(KEY_F9)),
            Outcome::Action(Action::Play("plain".to_string()))
        );

        dispatcher.process(down(CTRL));
        assert_eq!(
            dispatcher.process(down(KEY_F9)),
            Outcome::Action(Action::Play("with-ctrl".to_string()))
        );

        // Releasing Ctrl must restore the plain binding.
        dispatcher.process(up(CTRL));
        assert_eq!(
            dispatcher.process(down(KEY_F9)),
            Outcome::Action(Action::Play("plain".to_string()))
        );
    }

    #[test]
    fn right_hand_modifiers_count_as_ctrl() {
        let dispatcher = dispatcher_with(vec![(
            binding(KEY_A, vec![Mod::Ctrl]),
            Action::Play("ctrl-a".to_string()),
        )]);

        dispatcher.process(down(RIGHT_CTRL));
        assert_eq!(
            dispatcher.process(down(KEY_A)),
            Outcome::Action(Action::Play("ctrl-a".to_string()))
        );
    }

    #[test]
    fn holding_both_ctrls_keeps_ctrl_while_one_is_released() {
        let dispatcher = dispatcher_with(vec![(
            binding(KEY_A, vec![Mod::Ctrl, Mod::Shift]),
            Action::Play("combo".to_string()),
        )]);

        dispatcher.process(down(CTRL));
        dispatcher.process(down(RIGHT_CTRL));
        dispatcher.process(down(SHIFT));
        dispatcher.process(up(CTRL));

        assert_eq!(
            dispatcher.process(down(KEY_A)),
            Outcome::Action(Action::Play("combo".to_string())),
            "right ctrl is still held"
        );
    }

    #[test]
    fn a_binding_that_needs_no_modifier_does_not_fire_while_one_is_held() {
        let dispatcher = dispatcher_with(vec![(
            binding(KEY_F9, Vec::new()),
            Action::Play("plain".to_string()),
        )]);

        dispatcher.process(down(CTRL));
        assert_eq!(dispatcher.process(down(KEY_F9)), Outcome::Ignored);
    }

    #[test]
    fn capture_returns_a_binding_for_shift_numpad() {
        let dispatcher = Dispatcher::new();
        dispatcher.begin_capture();

        dispatcher.process(down(SHIFT));
        let outcome = dispatcher.process(down(KEY_KP5));

        assert_eq!(
            outcome,
            Outcome::Captured(Some(KeyBinding {
                code: KEY_KP5,
                mods: vec![Mod::Shift],
                label: "Shift + Num 5".to_string(),
            }))
        );
        assert_eq!(dispatcher.mode(), Mode::Normal);
    }

    #[test]
    fn capture_clears_on_backspace_and_cancels_on_escape() {
        let dispatcher = Dispatcher::new();

        dispatcher.begin_capture();
        assert_eq!(dispatcher.process(down(BACKSPACE)), Outcome::Captured(None));

        dispatcher.begin_capture();
        assert_eq!(dispatcher.process(down(ESC)), Outcome::CaptureCancelled);
        assert_eq!(dispatcher.mode(), Mode::Normal);
    }

    #[test]
    fn modifiers_alone_do_not_end_capture() {
        let dispatcher = Dispatcher::new();
        dispatcher.begin_capture();

        assert_eq!(dispatcher.process(down(SHIFT)), Outcome::Ignored);
        assert_eq!(dispatcher.process(up(SHIFT)), Outcome::Ignored);
        assert_eq!(dispatcher.mode(), Mode::Capturing);
    }

    #[test]
    fn no_action_fires_while_capturing() {
        let dispatcher = dispatcher_with(vec![(
            binding(KEY_KP5, Vec::new()),
            Action::Play("sound".to_string()),
        )]);
        dispatcher.begin_capture();

        assert_eq!(
            dispatcher.process(down(KEY_KP5)),
            Outcome::Captured(Some(binding(KEY_KP5, Vec::new())))
        );
    }

    #[test]
    fn escape_with_a_modifier_is_recorded_instead_of_cancelling() {
        let dispatcher = Dispatcher::new();
        dispatcher.begin_capture();
        dispatcher.process(down(CTRL));

        assert_eq!(
            dispatcher.process(down(ESC)),
            Outcome::Captured(Some(KeyBinding {
                code: ESC,
                mods: vec![Mod::Ctrl],
                label: "Ctrl + Esc".to_string(),
            }))
        );
    }

    #[test]
    fn stale_capture_timeouts_are_ignored() {
        let dispatcher = Dispatcher::new();

        let first = dispatcher.begin_capture();
        assert!(dispatcher.capture_still_open(first));

        // A finished capture bumps the generation, so the pending timeout no-ops.
        dispatcher.process(down(KEY_KP5));
        assert!(!dispatcher.capture_still_open(first));

        let second = dispatcher.begin_capture();
        dispatcher.cancel_capture();
        assert!(!dispatcher.capture_still_open(second));
    }

    #[test]
    fn rebuild_reads_sounds_and_stop_all_from_the_config() {
        let mut config = Config::default();
        config.sounds.push(Sound {
            id: "s1".to_string(),
            name: "Airhorn".to_string(),
            file: "airhorn.ogg".to_string(),
            emoji: "🔊".to_string(),
            color: "#f97316".to_string(),
            volume: 1.0,
            duration_ms: None,
            hotkey: Some(KeyBinding {
                code: KEY_KP3,
                mods: vec![Mod::Shift, Mod::Ctrl, Mod::Shift],
                label: "stale label".to_string(),
            }),
        });
        config.sounds.push(Sound {
            id: "s2".to_string(),
            name: "No key".to_string(),
            file: "none.ogg".to_string(),
            emoji: "🔊".to_string(),
            color: "#f97316".to_string(),
            volume: 1.0,
            duration_ms: None,
            hotkey: None,
        });
        config.settings.stop_all_hotkey = Some(KeyBinding {
            code: KEY_F9,
            mods: Vec::new(),
            label: "F9".to_string(),
        });

        let dispatcher = Dispatcher::new();
        dispatcher.rebuild(&config);
        assert_eq!(dispatcher.binding_count(), 2);

        dispatcher.process(down(CTRL));
        dispatcher.process(down(SHIFT));
        assert_eq!(
            dispatcher.process(down(KEY_KP3)),
            Outcome::Action(Action::Play("s1".to_string())),
            "stored modifier order must not matter"
        );

        dispatcher.process(up(CTRL));
        dispatcher.process(up(SHIFT));
        assert_eq!(
            dispatcher.process(down(KEY_F9)),
            Outcome::Action(Action::StopAll)
        );

        dispatcher.rebuild(&Config::default());
        assert_eq!(dispatcher.binding_count(), 1, "default stop-all key only");
    }

    #[test]
    fn settings_without_stop_all_key_leave_no_binding() {
        let config = Config {
            settings: Settings {
                stop_all_hotkey: None,
                ..Settings::default()
            },
            ..Config::default()
        };

        let dispatcher = Dispatcher::new();
        dispatcher.rebuild(&config);

        assert_eq!(dispatcher.binding_count(), 0);
    }
}
