//! Creates and tears down the virtual microphone audio graph.
//!
//! Graph (PLAN.md §4.1):
//! `pw-play` → `ssb_fx` → loopback A → `ssb_mix` → remap-source → `ssb_mic`,
//! plus optional loopback B (`ssb_fx` → default output) for monitoring and
//! optional loopback C (real mic → `ssb_mix`) for voice pass-through.

use crate::audio::pactl;
use crate::model::{Settings, MAX_VOLUME};

/// Prefix of every device and module argument we create; also used to find stale
/// modules from a previous crashed run.
const NAMESPACE: &str = "ssb_";

pub const FX_SINK: &str = "ssb_fx";
pub const MIX_SINK: &str = "ssb_mix";
pub const MIC_SOURCE: &str = "ssb_mic";

const FX_DESCRIPTION: &str = "SimplySoundboard Effects";
const MIX_DESCRIPTION: &str = "SimplySoundboard Mix";
const MIC_DESCRIPTION: &str = "SimplySoundboard Mic";

/// Loopback latency: low enough to feel live, high enough to avoid dropouts.
pub const LOOPBACK_LATENCY_MS: u32 = 20;

/// A fresh loopback needs a moment before its sink-input shows up in `pactl`.
const VOLUME_LOOKUP_ATTEMPTS: u32 = 10;
const VOLUME_LOOKUP_DELAY_MS: u64 = 50;

/// Module ids of the running virtual microphone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Router {
    pub fx: u32,
    pub mix: u32,
    pub mic_src: u32,
    pub loop_fx_mix: u32,
    pub loop_monitor: Option<u32>,
    pub loop_mic: Option<u32>,
    /// Real microphone currently piped in by loopback C.
    pub mic_input: Option<String>,
}

impl Router {
    /// Unloads stale modules, then builds the whole graph. On any failure every
    /// module loaded so far is unloaded again, so the audio server is left clean.
    pub fn start(settings: &Settings) -> Result<Self, String> {
        cleanup_stale();
        warn_if_not_pipewire();

        let mut loaded: Vec<u32> = Vec::new();
        match Self::build(settings, &mut loaded) {
            Ok(router) => Ok(router),
            Err(err) => {
                rollback(&loaded);
                Err(err)
            }
        }
    }

    fn build(settings: &Settings, loaded: &mut Vec<u32>) -> Result<Self, String> {
        let fx = load_tracked(
            loaded,
            "module-null-sink",
            &[
                format!("sink_name={FX_SINK}"),
                description("sink_properties", FX_DESCRIPTION),
            ],
        )?;

        let mix = load_tracked(
            loaded,
            "module-null-sink",
            &[
                format!("sink_name={MIX_SINK}"),
                description("sink_properties", MIX_DESCRIPTION),
            ],
        )?;

        // A remap-source rather than the raw monitor: many games and voice apps
        // hide `.monitor` sources from their device lists.
        let mic_src = load_tracked(
            loaded,
            "module-remap-source",
            &[
                format!("master={MIX_SINK}.monitor"),
                format!("source_name={MIC_SOURCE}"),
                description("source_properties", MIC_DESCRIPTION),
            ],
        )?;

        // A: effects into the mix (what other people hear).
        let loop_fx_mix = load_tracked(
            loaded,
            "module-loopback",
            &[
                format!("source={FX_SINK}.monitor"),
                format!("sink={MIX_SINK}"),
                latency(),
            ],
        )?;

        // B: effects to the default output (what I hear), no `sink=` so it follows
        // the default output device when it changes.
        let loop_monitor = if settings.monitor_enabled {
            Some(load_tracked(
                loaded,
                "module-loopback",
                &[format!("source={FX_SINK}.monitor"), latency()],
            )?)
        } else {
            None
        };

        // C: the real microphone into the mix.
        let (loop_mic, mic_input) = if settings.mic_passthrough {
            match resolve_mic_source(settings)? {
                Some(mic) => (
                    Some(load_tracked(
                        loaded,
                        "module-loopback",
                        &[
                            format!("source={mic}"),
                            format!("sink={MIX_SINK}"),
                            latency(),
                        ],
                    )?),
                    Some(mic),
                ),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        Ok(Self {
            fx,
            mix,
            mic_src,
            loop_fx_mix,
            loop_monitor,
            loop_mic,
            mic_input,
        })
    }

    /// Unloads every module of the graph, in reverse creation order.
    pub fn stop(&mut self) {
        for id in self.module_ids().into_iter().rev() {
            pactl::unload_module(id);
        }
        self.loop_mic = None;
        self.loop_monitor = None;
        self.mic_input = None;
    }

    /// Module ids in creation order.
    pub fn module_ids(&self) -> Vec<u32> {
        let mut ids = vec![self.fx, self.mix, self.mic_src, self.loop_fx_mix];
        ids.extend(self.loop_monitor);
        ids.extend(self.loop_mic);
        ids
    }

    /// Brings the live graph in line with `settings` and applies all three volumes.
    pub fn apply_all(&mut self, settings: &Settings) -> Result<(), String> {
        let mut errors = Vec::new();
        collect_error(
            &mut errors,
            self.sync_monitor_loop(settings),
            "monitor loopback",
        );
        collect_error(
            &mut errors,
            self.sync_mic_loop(settings),
            "microphone loopback",
        );
        collect_error(&mut errors, self.apply_volumes(settings), "volumes");
        join_errors(errors)
    }

    /// Loads or unloads loopback B to match `monitor_enabled`.
    pub fn sync_monitor_loop(&mut self, settings: &Settings) -> Result<(), String> {
        match (settings.monitor_enabled, self.loop_monitor) {
            (true, None) => {
                self.loop_monitor = Some(pactl::load_module(
                    "module-loopback",
                    &[format!("source={FX_SINK}.monitor"), latency()],
                )?);
            }
            (false, Some(module)) => {
                pactl::unload_module(module);
                self.loop_monitor = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Loads, unloads or repoints loopback C to match the pass-through settings.
    pub fn sync_mic_loop(&mut self, settings: &Settings) -> Result<(), String> {
        let desired = if settings.mic_passthrough {
            resolve_mic_source(settings)?
        } else {
            None
        };

        if desired == self.mic_input {
            return Ok(());
        }

        if let Some(module) = self.loop_mic.take() {
            pactl::unload_module(module);
        }
        self.mic_input = None;

        if let Some(mic) = desired {
            self.loop_mic = Some(pactl::load_module(
                "module-loopback",
                &[
                    format!("source={mic}"),
                    format!("sink={MIX_SINK}"),
                    latency(),
                ],
            )?);
            self.mic_input = Some(mic);
        }
        Ok(())
    }

    /// Pushes the three loopback volumes into their sink-inputs.
    pub fn apply_volumes(&self, settings: &Settings) -> Result<(), String> {
        let mut errors = Vec::new();

        collect_error(
            &mut errors,
            set_module_volume(self.loop_fx_mix, settings.to_mic_volume),
            "to-mic volume",
        );
        if let Some(module) = self.loop_monitor {
            collect_error(
                &mut errors,
                set_module_volume(module, settings.monitor_volume),
                "monitor volume",
            );
        }
        if let Some(module) = self.loop_mic {
            collect_error(
                &mut errors,
                set_module_volume(module, settings.mic_volume),
                "mic volume",
            );
        }

        join_errors(errors)
    }
}

/// Sets the volume of the sink-input created by `module_id`.
///
/// A freshly loaded loopback only publishes its stream a moment later, so retry
/// briefly instead of failing the whole settings change.
pub fn set_module_volume(module_id: u32, volume: f32) -> Result<(), String> {
    let clamped = volume.clamp(0.0, MAX_VOLUME);

    for attempt in 0..VOLUME_LOOKUP_ATTEMPTS {
        if let Some(input) = pactl::list_sink_inputs()?
            .into_iter()
            .find(|input| input.owner_module == Some(module_id))
        {
            return pactl::set_sink_input_volume(input.index, clamped);
        }
        if attempt + 1 < VOLUME_LOOKUP_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(VOLUME_LOOKUP_DELAY_MS));
        }
    }

    Err(format!(
        "Could not find the audio stream of module {module_id}"
    ))
}

fn collect_error(errors: &mut Vec<String>, result: Result<(), String>, label: &str) {
    if let Err(err) = result {
        errors.push(format!("{label}: {err}"));
    }
}

fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Unloads modules left over from a crashed run: ours always mention `ssb_`.
pub fn cleanup_stale() {
    let modules = match pactl::list_modules() {
        Ok(modules) => modules,
        Err(err) => {
            log::warn!("Could not list audio modules: {err}");
            return;
        }
    };

    for module in modules {
        let argument = module.argument.unwrap_or_default();
        if argument.contains(NAMESPACE) {
            log::info!("Unloading stale module {} ({})", module.id, module.name);
            pactl::unload_module(module.id);
        }
    }
}

/// Resolves the real microphone for pass-through: the configured source if it still
/// exists, otherwise the system default. `None` when the candidate is one of our own
/// devices or a raw monitor, which would feed the app back into itself.
pub fn resolve_mic_source(settings: &Settings) -> Result<Option<String>, String> {
    let sources = pactl::list_sources()?;
    let default_source = pactl::default_source().unwrap_or_default();

    let configured = settings
        .mic_source
        .as_deref()
        .filter(|name| sources.iter().any(|source| source.name == *name));
    let candidate = configured.unwrap_or(default_source.as_str());

    if candidate.is_empty() {
        return Ok(None);
    }
    if is_feedback_prone(candidate) {
        log::warn!("Refusing to loop {candidate:?} back: it is our own device or a monitor");
        return Ok(None);
    }
    Ok(Some(candidate.to_string()))
}

/// True for our own devices (`ssb_*`) and raw monitor sources.
pub fn is_feedback_prone(name: &str) -> bool {
    name.starts_with(NAMESPACE) || name.ends_with(".monitor")
}

fn warn_if_not_pipewire() {
    match pactl::server_name() {
        Ok(name) if name.contains("PipeWire") => {}
        Ok(name) => {
            log::warn!("Audio server is {name:?}, not PipeWire — the virtual mic may misbehave")
        }
        Err(err) => log::warn!("Could not read the audio server name: {err}"),
    }
}

/// `sink_properties=device.description=SimplySoundboard_Effects` as one argv
/// element.
///
/// `pactl` hands `*_properties` to PulseAudio's proplist parser, which splits on
/// spaces and honours no quoting (*verified on PipeWire 1.6: single quotes, double
/// quotes and backslashes all truncate the value at the first space*). PLAN.md §9
/// prescribes underscores as the fallback, so device labels stay complete and
/// distinguishable instead of silently becoming "SimplySoundboard".
fn description(property: &str, text: &str) -> String {
    format!("{property}=device.description={}", text.replace(' ', "_"))
}

fn latency() -> String {
    format!("latency_msec={LOOPBACK_LATENCY_MS}")
}

fn load_tracked(loaded: &mut Vec<u32>, name: &str, args: &[String]) -> Result<u32, String> {
    let id = pactl::load_module(name, args)?;
    loaded.push(id);
    Ok(id)
}

fn rollback(loaded: &[u32]) {
    for id in loaded.iter().rev() {
        pactl::unload_module(*id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_underscored_as_one_argv_element() {
        assert_eq!(
            description("sink_properties", "SimplySoundboard Effects"),
            "sink_properties=device.description=SimplySoundboard_Effects"
        );
        assert_eq!(
            description("source_properties", MIC_DESCRIPTION),
            "source_properties=device.description=SimplySoundboard_Mic"
        );
        assert_eq!(latency(), "latency_msec=20");
    }

    #[test]
    fn feedback_prone_names_are_refused() {
        assert!(is_feedback_prone(MIC_SOURCE));
        assert!(is_feedback_prone("ssb_fx"));
        assert!(is_feedback_prone("alsa_output.pci.hdmi-stereo.monitor"));
        assert!(!is_feedback_prone("alsa_input.usb-mic.analog-stereo"));
    }
}
