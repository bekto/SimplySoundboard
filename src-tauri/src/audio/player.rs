//! Playback engine: one `pw-play` process per sound instance.
//!
//! Instances are tracked per sound id so retrigger behaviour, `stop` and `stop_all`
//! can address them. Only the pid is kept in the map; the [`Child`] is moved into a
//! waiter thread that reaps it (no zombies) and emits the `ended` event.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::model::{Retrigger, Sound};
use crate::state::{lock, Core};

/// Sink every sound is played into.
pub const TARGET_SINK: &str = "ssb_fx";

/// Running sound instances, keyed by sound id.
#[derive(Default)]
pub struct Player {
    /// sound id → (instance id, pid)
    procs: Mutex<HashMap<String, Vec<(u64, u32)>>>,
    next_instance: AtomicU64,
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts (or restarts, or refuses to start) playback of `sound`.
    pub fn play(
        &self,
        app: &AppHandle,
        sound: &Sound,
        path: &Path,
        mode: Retrigger,
    ) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("Sound file missing: {}", sound.file));
        }

        let running = self.instances(&sound.id);
        let was_idle = running.is_empty();
        let (start, stop_existing) = trigger_plan(mode, !was_idle);

        if !start {
            for (_, pid) in &running {
                terminate(*pid);
            }
            return Ok(());
        }

        let child = spawn(sound, path)?;
        let pid = child.id();
        let instance_id = self.next_instance.fetch_add(1, Ordering::SeqCst);

        // Register the new instance before killing the old ones, so a waiter thread
        // noticing "nothing left" can never announce the end of a sound that is
        // still playing.
        lock(&self.procs)
            .entry(sound.id.clone())
            .or_default()
            .push((instance_id, pid));

        if stop_existing {
            for (_, pid) in &running {
                terminate(*pid);
            }
        }

        // "started" marks the transition from silence to sound, not every trigger.
        if was_idle {
            emit(app, &sound.id, "started");
        }

        spawn_waiter(app.clone(), sound.id.clone(), instance_id, child);
        Ok(())
    }

    /// Sends SIGTERM to every instance of a sound. The `ended` event comes from the
    /// waiter thread once the processes are actually gone.
    pub fn stop(&self, sound_id: &str) {
        for (_, pid) in self.instances(sound_id) {
            terminate(pid);
        }
    }

    /// Stops every running sound.
    pub fn stop_all(&self) {
        let ids: Vec<String> = {
            let procs = lock(&self.procs);
            procs.keys().cloned().collect()
        };
        for id in ids {
            self.stop(&id);
        }
    }

    fn instances(&self, sound_id: &str) -> Vec<(u64, u32)> {
        let procs = lock(&self.procs);
        procs.get(sound_id).cloned().unwrap_or_default()
    }

    /// Drops a finished instance; returns true when the sound went silent.
    fn remove_instance(&self, sound_id: &str, instance_id: u64) -> bool {
        let mut procs = lock(&self.procs);
        let Some(instances) = procs.get_mut(sound_id) else {
            return true;
        };

        instances.retain(|(id, _)| *id != instance_id);
        if instances.is_empty() {
            procs.remove(sound_id);
            return true;
        }
        false
    }
}

/// Whether a trigger starts a new instance, and whether running ones must die first.
fn trigger_plan(mode: Retrigger, already_playing: bool) -> (bool, bool) {
    match (mode, already_playing) {
        (_, false) => (true, false),
        (Retrigger::Restart, true) => (true, true),
        (Retrigger::Overlap, true) => (true, false),
        (Retrigger::Stop, true) => (false, true),
    }
}

fn spawn(sound: &Sound, path: &Path) -> Result<Child, String> {
    Command::new("pw-play")
        .arg("--target")
        .arg(TARGET_SINK)
        .arg("--volume")
        .arg(format!("{:.3}", sound.volume))
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Could not start playback: {err}"))
}

fn spawn_waiter(app: AppHandle, sound_id: String, instance_id: u64, mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();

        let core = app.state::<Core>();
        if core.player.remove_instance(&sound_id, instance_id) {
            emit(&app, &sound_id, "ended");
        }
    });
}

fn emit(app: &AppHandle, sound_id: &str, state: &str) {
    let payload = serde_json::json!({ "id": sound_id, "state": state });
    if let Err(err) = app.emit("playback", payload) {
        log::warn!("Could not emit playback event: {err}");
    }
}

/// SIGTERM, tolerating an already-dead process.
fn terminate(pid: u32) {
    let Ok(pid) = i32::try_from(pid) else {
        return;
    };

    match nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    ) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
        Err(err) => log::warn!("Could not stop playback process {pid}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_plan_covers_every_retrigger_mode() {
        assert_eq!(trigger_plan(Retrigger::Restart, false), (true, false));
        assert_eq!(trigger_plan(Retrigger::Overlap, false), (true, false));
        assert_eq!(trigger_plan(Retrigger::Stop, false), (true, false));

        assert_eq!(trigger_plan(Retrigger::Restart, true), (true, true));
        assert_eq!(trigger_plan(Retrigger::Overlap, true), (true, false));
        assert_eq!(trigger_plan(Retrigger::Stop, true), (false, true));
    }

    #[test]
    fn instance_bookkeeping_reports_the_last_instance() {
        let player = Player::new();
        let sound = "id".to_string();

        lock(&player.procs)
            .entry(sound.clone())
            .or_default()
            .push((7, 4242));
        lock(&player.procs)
            .entry(sound.clone())
            .or_default()
            .push((8, 4243));

        assert!(!player.remove_instance(&sound, 7), "still one left");
        assert_eq!(player.instances(&sound), vec![(8, 4243)]);
        assert!(player.remove_instance(&sound, 8), "now silent");
        assert!(player.instances(&sound).is_empty());
        assert!(player.remove_instance(&sound, 8), "already gone");
    }

    #[test]
    fn stopping_an_unknown_sound_is_a_noop() {
        let player = Player::new();
        player.stop("nope");
        player.stop_all();
        assert!(player.instances("nope").is_empty());
    }
}
