//! evdev key code to modifier / human readable label mapping.

use evdev::KeyCode;

use crate::model::Mod;

/// Modifier a physical modifier key stands for; left and right are merged.
pub fn mod_of(code: u16) -> Option<Mod> {
    match KeyCode(code) {
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => Some(Mod::Ctrl),
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => Some(Mod::Shift),
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => Some(Mod::Alt),
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => Some(Mod::Super),
        _ => None,
    }
}

/// Display name of a modifier, as used in labels.
pub fn mod_name(modifier: Mod) -> &'static str {
    match modifier {
        Mod::Ctrl => "Ctrl",
        Mod::Shift => "Shift",
        Mod::Alt => "Alt",
        Mod::Super => "Super",
    }
}

/// Human readable label for a key, e.g. `"Num 1"`, `"F13"`, `"↑"`.
pub fn key_label(code: u16) -> String {
    if let Some(label) = explicit_label(KeyCode(code)) {
        return label.to_string();
    }
    if (KeyCode::KEY_A.0..=KeyCode::KEY_Z.0).contains(&code) {
        return char::from(b'A' + (code - KeyCode::KEY_A.0) as u8).to_string();
    }
    if code == KeyCode::KEY_0.0 {
        return "0".to_string();
    }
    if (KeyCode::KEY_1.0..=KeyCode::KEY_9.0).contains(&code) {
        return (code - KeyCode::KEY_1.0 + 1).to_string();
    }
    // The function keys are laid out in three blocks in the evdev ABI:
    // F1-F10, then F11/F12, then F13-F24.
    if (KeyCode::KEY_F1.0..=KeyCode::KEY_F10.0).contains(&code) {
        return format!("F{}", code - KeyCode::KEY_F1.0 + 1);
    }
    if (KeyCode::KEY_F11.0..=KeyCode::KEY_F12.0).contains(&code) {
        return format!("F{}", code - KeyCode::KEY_F11.0 + 11);
    }
    if (KeyCode::KEY_F13.0..=KeyCode::KEY_F24.0).contains(&code) {
        return format!("F{}", code - KeyCode::KEY_F13.0 + 13);
    }
    fallback_label(KeyCode(code))
}

/// `"Ctrl + Shift + F9"` — modifiers in canonical order, then the key.
pub fn binding_label(mods: &[Mod], code: u16) -> String {
    let mut ordered = mods.to_vec();
    ordered.sort();
    ordered.dedup();

    let mut parts: Vec<String> = ordered
        .into_iter()
        .map(|modifier| mod_name(modifier).to_string())
        .collect();
    parts.push(key_label(code));
    parts.join(" + ")
}

/// Keys whose evdev name is not good enough for a UI chip.
fn explicit_label(key: KeyCode) -> Option<&'static str> {
    let label = match key {
        KeyCode::KEY_KP0 => "Num 0",
        KeyCode::KEY_KP1 => "Num 1",
        KeyCode::KEY_KP2 => "Num 2",
        KeyCode::KEY_KP3 => "Num 3",
        KeyCode::KEY_KP4 => "Num 4",
        KeyCode::KEY_KP5 => "Num 5",
        KeyCode::KEY_KP6 => "Num 6",
        KeyCode::KEY_KP7 => "Num 7",
        KeyCode::KEY_KP8 => "Num 8",
        KeyCode::KEY_KP9 => "Num 9",
        KeyCode::KEY_KPPLUS => "Num +",
        KeyCode::KEY_KPMINUS => "Num −",
        KeyCode::KEY_KPASTERISK => "Num *",
        KeyCode::KEY_KPSLASH => "Num /",
        KeyCode::KEY_KPDOT => "Num .",
        KeyCode::KEY_KPENTER => "Num Enter",
        KeyCode::KEY_SPACE => "Space",
        KeyCode::KEY_TAB => "Tab",
        KeyCode::KEY_ENTER => "Enter",
        KeyCode::KEY_ESC => "Esc",
        KeyCode::KEY_BACKSPACE => "Backspace",
        KeyCode::KEY_UP => "↑",
        KeyCode::KEY_DOWN => "↓",
        KeyCode::KEY_LEFT => "←",
        KeyCode::KEY_RIGHT => "→",
        KeyCode::KEY_PAGEUP => "PgUp",
        KeyCode::KEY_PAGEDOWN => "PgDn",
        KeyCode::KEY_INSERT => "Ins",
        KeyCode::KEY_DELETE => "Del",
        KeyCode::KEY_HOME => "Home",
        KeyCode::KEY_END => "End",
        KeyCode::KEY_GRAVE => "`",
        KeyCode::KEY_MINUS => "-",
        KeyCode::KEY_EQUAL => "=",
        KeyCode::KEY_LEFTBRACE => "[",
        KeyCode::KEY_RIGHTBRACE => "]",
        KeyCode::KEY_BACKSLASH => "\\",
        KeyCode::KEY_SEMICOLON => ";",
        KeyCode::KEY_APOSTROPHE => "'",
        KeyCode::KEY_COMMA => ",",
        KeyCode::KEY_DOT => ".",
        KeyCode::KEY_SLASH => "/",
        _ => return None,
    };
    Some(label)
}

/// evdev name without the `KEY_` prefix, first letter upper cased
/// (`KEY_PAGEUP` -> `Pageup`); unknown codes get a numeric label.
fn fallback_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    let Some(rest) = name.strip_prefix("KEY_") else {
        return format!("Key {}", key.0);
    };

    let lowered = rest.to_ascii_lowercase();
    let mut chars = lowered.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => format!("Key {}", key.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_cover_numpad_letters_and_function_keys() {
        assert_eq!(key_label(KeyCode::KEY_KP1.0), "Num 1");
        assert_eq!(key_label(KeyCode::KEY_A.0), "A");
        assert_eq!(key_label(KeyCode::KEY_KPDOT.0), "Num .");
        assert_eq!(key_label(KeyCode::KEY_1.0), "1");
        assert_eq!(key_label(KeyCode::KEY_0.0), "0");
    }

    #[test]
    fn function_key_labels_follow_the_three_evdev_blocks() {
        assert_eq!(key_label(KeyCode::KEY_F1.0), "F1");
        assert_eq!(key_label(KeyCode::KEY_F10.0), "F10");
        assert_eq!(key_label(KeyCode::KEY_F11.0), "F11");
        assert_eq!(key_label(KeyCode::KEY_F12.0), "F12");
        assert_eq!(key_label(KeyCode::KEY_F13.0), "F13");
        assert_eq!(key_label(KeyCode::KEY_F24.0), "F24");
        // KEY_NUMLOCK sits between F10 and F11; it must not become a function key.
        assert_ne!(key_label(KeyCode::KEY_NUMLOCK.0), "F11");
    }

    #[test]
    fn named_keys_use_short_labels() {
        assert_eq!(key_label(KeyCode::KEY_PAGEUP.0), "PgUp");
        assert_eq!(key_label(KeyCode::KEY_PAGEDOWN.0), "PgDn");
        assert_eq!(key_label(KeyCode::KEY_INSERT.0), "Ins");
        assert_eq!(key_label(KeyCode::KEY_DELETE.0), "Del");
        assert_eq!(key_label(KeyCode::KEY_HOME.0), "Home");
        assert_eq!(key_label(KeyCode::KEY_END.0), "End");
        assert_eq!(key_label(KeyCode::KEY_GRAVE.0), "`");
        assert_eq!(key_label(KeyCode::KEY_SPACE.0), "Space");
        assert_eq!(key_label(KeyCode::KEY_UP.0), "↑");
    }

    #[test]
    fn unknown_keys_fall_back_to_a_titled_evdev_name() {
        assert_eq!(key_label(KeyCode::KEY_SYSRQ.0), "Sysrq");
        assert_eq!(key_label(65000), "Key 65000");
    }

    #[test]
    fn binding_labels_order_modifiers_canonically() {
        assert_eq!(
            binding_label(&[Mod::Shift, Mod::Ctrl], KeyCode::KEY_F9.0),
            "Ctrl + Shift + F9"
        );
        assert_eq!(
            binding_label(
                &[Mod::Super, Mod::Alt, Mod::Ctrl, Mod::Shift],
                KeyCode::KEY_F9.0
            ),
            "Ctrl + Shift + Alt + Super + F9"
        );
        assert_eq!(binding_label(&[], KeyCode::KEY_KP5.0), "Num 5");
        assert_eq!(
            binding_label(&[Mod::Ctrl, Mod::Ctrl], KeyCode::KEY_KP5.0),
            "Ctrl + Num 5"
        );
    }

    #[test]
    fn modifiers_merge_left_and_right() {
        assert_eq!(mod_of(KeyCode::KEY_LEFTCTRL.0), Some(Mod::Ctrl));
        assert_eq!(mod_of(KeyCode::KEY_RIGHTCTRL.0), Some(Mod::Ctrl));
        assert_eq!(mod_of(KeyCode::KEY_LEFTSHIFT.0), Some(Mod::Shift));
        assert_eq!(mod_of(KeyCode::KEY_RIGHTALT.0), Some(Mod::Alt));
        assert_eq!(mod_of(KeyCode::KEY_LEFTMETA.0), Some(Mod::Super));
        assert_eq!(mod_of(KeyCode::KEY_RIGHTMETA.0), Some(Mod::Super));
        assert_eq!(mod_of(KeyCode::KEY_A.0), None);
    }
}
