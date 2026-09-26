//! Keyboard (/dev/input) permission status and one-time udev setup.
//!
//! Reading `/dev/input` needs more than the default ACL, so the app ships a udev
//! rule that grants the logged-in seat user access (`uaccess`) and installs it on
//! demand through polkit. Packages install the same rule, so packaged builds never
//! prompt.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use tauri::{AppHandle, Manager};

use crate::hotkeys::listener::Listener;
use crate::model::{InputState, InputStatus};
use crate::state::{lock, Core};

/// udev rule granting the seat user access to keyboards.
pub const INPUT_RULE: &str = include_str!("../../packaging/70-simplysoundboard-input.rules");

/// Where the rule has to live for udev to pick it up.
pub const RULE_PATH: &str = "/etc/udev/rules.d/70-simplysoundboard-input.rules";

/// Whether global hotkeys can work right now, and which keyboards are being read.
pub fn input_status(listener: &Listener) -> InputStatus {
    let keyboards = listener.keyboards();

    let state = if !keyboards.is_empty() {
        InputState::Ok
    } else if listener.denied_paths().is_empty() {
        InputState::NoKeyboards
    } else {
        InputState::NoPermission
    };

    InputStatus { state, keyboards }
}

/// Stores the current status and, when it changed, tells the UI.
pub fn publish_input_status(app: &AppHandle) {
    let core = app.state::<Core>();
    let status = input_status(&core.listener);

    let changed = {
        let mut current = lock(&core.input);
        let changed = *current != status;
        *current = status.clone();
        changed
    };

    if changed {
        crate::hotkeys::emit(app, "input_status", status);
    }
}

/// Installs the udev rule through polkit and reloads udev.
///
/// Returns `"Authorization was cancelled"` when the user dismisses the password
/// dialog (pkexec exit code 126/127).
pub fn setup_input_access() -> Result<(), String> {
    let rule = std::env::temp_dir().join("70-simplysoundboard-input.rules");
    std::fs::write(&rule, INPUT_RULE)
        .map_err(|err| format!("Could not write {}: {err}", rule.display()))?;

    let argv = install_argv(&rule);
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                "pkexec not found — install polkit".to_string()
            } else {
                format!("Could not run pkexec: {err}")
            }
        })?;

    let _ = std::fs::remove_file(&rule);

    if output.status.success() {
        return Ok(());
    }

    match output.status.code() {
        Some(126) | Some(127) => Err("Authorization was cancelled".to_string()),
        Some(code) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                Err(format!("Installing the keyboard rule failed (exit {code})"))
            } else {
                Err(stderr)
            }
        }
        None => Err("Installing the keyboard rule was interrupted".to_string()),
    }
}

/// `pkexec sh -c '<install> && udevadm …' sh <rule>`
///
/// `$1` is passed as a separate argv element, so paths are never interpolated into
/// the shell script.
fn install_argv(rule: &Path) -> Vec<OsString> {
    let script = format!(
        "install -m 0644 \"$1\" {RULE_PATH} \
         && udevadm control --reload \
         && udevadm trigger --subsystem-match=input --action=change"
    );

    vec![
        OsString::from("pkexec"),
        OsString::from("sh"),
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("sh"), // $0 for the script
        rule.as_os_str().to_os_string(),
    ]
}

/// Waits until the listener has a keyboard again, or `timeout` runs out.
pub fn wait_for_keyboard(listener: &Listener, timeout: std::time::Duration) -> InputStatus {
    let deadline = std::time::Instant::now() + timeout;

    loop {
        listener.rescan_now();
        let status = input_status(listener);
        if !status.keyboards.is_empty() || std::time::Instant::now() >= deadline {
            return status;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_rule_grants_uaccess_to_keyboards() {
        assert!(INPUT_RULE.contains("SUBSYSTEM==\"input\""));
        assert!(INPUT_RULE.contains("KERNEL==\"event*\""));
        assert!(INPUT_RULE.contains("ENV{ID_INPUT_KEYBOARD}==\"1\""));
        assert!(INPUT_RULE.contains("TAG+=\"uaccess\""));
        assert_eq!(
            INPUT_RULE
                .lines()
                .filter(|line| line.starts_with("SUBSYSTEM"))
                .count(),
            1
        );
    }

    #[test]
    fn rule_in_the_package_matches_the_embedded_copy() {
        let packaged = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../packaging/70-simplysoundboard-input.rules");
        let contents = std::fs::read_to_string(packaged).expect("read the packaged rule");
        assert_eq!(contents, INPUT_RULE);
    }

    #[test]
    fn install_command_passes_the_rule_as_an_argument() {
        let argv = install_argv(Path::new("/tmp/70-simplysoundboard-input.rules"));
        let rendered: Vec<String> = argv
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(rendered[0], "pkexec");
        assert_eq!(rendered[1], "sh");
        assert_eq!(rendered[2], "-c");
        assert_eq!(rendered[4], "sh", "$0 must be set for sh -c");
        assert_eq!(rendered[5], "/tmp/70-simplysoundboard-input.rules");

        let script = &rendered[3];
        assert!(
            script.contains(RULE_PATH),
            "installs into the udev rules directory"
        );
        assert!(script.contains("install -m 0644 \"$1\""));
        assert!(script.contains("udevadm control --reload"));
        assert!(script.contains("udevadm trigger --subsystem-match=input --action=change"));
    }

    #[test]
    fn input_status_matches_the_listener_state() {
        let listener = Listener::default();

        assert_eq!(input_status(&listener).state, InputState::NoKeyboards);

        listener.note_denied(Path::new("/dev/input/event3"));
        assert_eq!(input_status(&listener).state, InputState::NoPermission);

        listener.note_opened(Path::new("/dev/input/event3"), "Gaming Keyboard");
        let status = input_status(&listener);
        assert_eq!(status.state, InputState::Ok);
        assert_eq!(status.keyboards, vec!["Gaming Keyboard".to_string()]);
    }

    #[test]
    fn waiting_returns_the_current_status_when_there_is_no_keyboard() {
        let listener = Listener::default();
        let status = wait_for_keyboard(&listener, std::time::Duration::from_millis(250));
        assert!(status.keyboards.is_empty());
    }
}
