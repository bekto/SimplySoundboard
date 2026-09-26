//! Shared application state (config, router, input status) managed by Tauri.

use std::sync::{Mutex, MutexGuard};

use crate::config;
use crate::model::{Config, InputStatus, RouterStatus};

/// Locks a mutex, recovering from poisoning: a panicked thread must not take the
/// whole app down.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Long-lived state shared by every command and background thread.
pub struct Core {
    pub config: Mutex<Config>,
    pub router_status: Mutex<RouterStatus>,
    pub input: Mutex<InputStatus>,
}

impl Core {
    pub fn new(config: Config) -> Self {
        Self {
            config: Mutex::new(config),
            router_status: Mutex::new(RouterStatus::default()),
            input: Mutex::new(InputStatus::default()),
        }
    }

    /// Snapshot of the current config.
    pub fn config(&self) -> Config {
        lock(&self.config).clone()
    }

    /// Applies `f` to a draft copy of the config and persists it. The in-memory
    /// config is only updated once the write succeeded, so a failing `f` or a
    /// failing save leaves both memory and disk untouched.
    pub fn mutate_config<T>(
        &self,
        f: impl FnOnce(&mut Config) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut current = lock(&self.config);
        let mut draft = current.clone();
        let result = f(&mut draft)?;
        config::save(&draft)?;
        *current = draft;
        Ok(result)
    }
}
