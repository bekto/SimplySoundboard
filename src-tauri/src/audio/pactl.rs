//! Thin wrapper around the external `pactl` binary.
//!
//! No policy lives here: every function is a 1:1 mapping to a `pactl` invocation
//! and returns a plain error string meant for the UI.

use std::ffi::OsStr;
use std::process::Command;

use serde::{Deserialize, Serialize};

const PACTL: &str = "pactl";

/// Runs `pactl` with `args`. Non-zero exit yields stderr; a missing binary yields
/// an actionable install hint.
pub fn run<S: AsRef<OsStr>>(args: &[S]) -> Result<String, String> {
    let output = Command::new(PACTL).args(args).output().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "pactl not found — install pulseaudio-utils / libpulse".to_string()
        } else {
            format!("Could not run pactl: {err}")
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(format!(
                "pactl {} failed",
                args[0].as_ref().to_string_lossy()
            ));
        }
        return Err(stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Loads a module and returns its module index.
pub fn load_module(name: &str, args: &[String]) -> Result<u32, String> {
    let mut argv: Vec<String> = Vec::with_capacity(args.len() + 2);
    argv.push("load-module".to_string());
    argv.push(name.to_string());
    argv.extend(args.iter().cloned());

    let stdout = run(&argv)?;
    stdout.trim().parse::<u32>().map_err(|err| {
        format!(
            "Unexpected pactl load-module output ({err}): {}",
            stdout.trim()
        )
    })
}

pub fn unload_module(id: u32) {
    let args = ["unload-module".to_string(), id.to_string()];
    if let Err(err) = run(&args) {
        log::warn!("Could not unload module {id}: {err}");
    }
}

/// A loaded module. `id` is the module index accepted by `unload-module`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: u32,
    pub name: String,
    pub argument: Option<String>,
}

/// Loaded modules.
///
/// Parsed from `pactl list short modules` rather than JSON: PipeWire's JSON module
/// listing has no index field (the id hides in `properties["object.id"]`), and the
/// short list's first column is exactly the index `unload-module` expects. Module
/// arguments may contain newlines, so continuation lines are folded back in.
pub fn list_modules() -> Result<Vec<ModuleInfo>, String> {
    Ok(parse_module_list(&run(&["list", "short", "modules"])?))
}

fn parse_module_list(raw: &str) -> Vec<ModuleInfo> {
    let mut modules: Vec<ModuleInfo> = Vec::new();

    for line in raw.lines() {
        let record_start = line
            .split_once('\t')
            .is_some_and(|(head, _)| !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()));

        if record_start {
            let mut fields = line.splitn(3, '\t');
            let id = fields
                .next()
                .and_then(|field| field.trim().parse::<u32>().ok());
            let Some(id) = id else { continue };
            modules.push(ModuleInfo {
                id,
                name: fields.next().unwrap_or_default().to_string(),
                argument: fields.next().map(str::to_string),
            });
        } else if let Some(argument) = modules
            .last_mut()
            .and_then(|module| module.argument.as_mut())
        {
            argument.push('\n');
            argument.push_str(line);
        }
    }

    modules
}

/// A source (input) device.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SourceInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub fn list_sources() -> Result<Vec<SourceInfo>, String> {
    parse_json(&run(&["-f", "json", "list", "sources"])?)
}

/// One playback stream feeding a sink.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SinkInputInfo {
    pub index: u32,
    /// Module that created the stream (set for loopbacks).
    #[serde(default, deserialize_with = "optional_u32")]
    pub owner_module: Option<u32>,
}

pub fn list_sink_inputs() -> Result<Vec<SinkInputInfo>, String> {
    parse_json(&run(&["-f", "json", "list", "sink-inputs"])?)
}

fn parse_json<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(|err| format!("Could not parse pactl output: {err}"))
}

/// `owner_module` is a number on PulseAudio and PipeWire, but some versions
/// serialize it as a string — accept both, and `null` as "none".
fn optional_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IdOrName {
        Id(u64),
        Text(String),
    }

    Ok(match Option::<IdOrName>::deserialize(deserializer)? {
        None => None,
        Some(IdOrName::Id(id)) => u32::try_from(id).ok(),
        Some(IdOrName::Text(text)) => text.trim().parse().ok(),
    })
}

/// Name of the current default source.
pub fn default_source() -> Result<String, String> {
    Ok(run(&["get-default-source"])?.trim().to_string())
}

pub fn set_default_source(name: &str) -> Result<(), String> {
    run(&["set-default-source", name]).map(|_| ())
}

/// Sets a playback stream's volume. `volume` is linear (1.0 = 100%, max 1.5).
pub fn set_sink_input_volume(index: u32, volume: f32) -> Result<(), String> {
    let percent = format!("{}%", (volume * 100.0).round() as i64);
    run(&["set-sink-input-volume", &index.to_string(), &percent]).map(|_| ())
}

/// `Server Name:` line of `pactl info`, e.g. `PulseAudio (on PipeWire 1.6.8)`.
pub fn server_name() -> Result<String, String> {
    let info = run(&["info"])?;
    info.lines()
        .find_map(|line| line.strip_prefix("Server Name:"))
        .map(|name| name.trim().to_string())
        .ok_or_else(|| "Could not read the audio server name".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_module_list_with_multiline_arguments() {
        // Real `pactl list short modules` output: the argument column keeps its
        // newlines, so a single record spans several lines.
        let raw = concat!(
            "1\tlibpipewire-module-rt\t{\n",
            "            nice.level    = -11\n",
            "        }\n",
            "25\tmodule-null-sink\tsink_name=ssb_fx sink_properties=device.description='Effects'\n",
            "26\tmodule-loopback\tsource=ssb_fx.monitor sink=ssb_mix\n",
        );

        let modules = parse_module_list(raw);

        assert_eq!(
            modules.len(),
            3,
            "continuation lines must not start records"
        );
        assert_eq!(modules[0].id, 1);
        assert_eq!(modules[0].name, "libpipewire-module-rt");
        let argument = modules[0].argument.as_deref().expect("argument");
        assert!(argument.contains("nice.level"));
        assert!(argument.ends_with('}'), "folded argument: {argument:?}");
        assert_eq!(modules[1].id, 25);
        assert!(modules[1]
            .argument
            .as_deref()
            .expect("argument")
            .contains("sink_name=ssb_fx"));
        assert_eq!(modules[2].id, 26);
    }

    #[test]
    fn parses_module_list_without_arguments() {
        let modules = parse_module_list("");
        assert!(modules.is_empty());
    }

    #[test]
    fn parses_sources_json() {
        let raw = r#"[{"index":54,"name":"alsa_input.usb-mic.analog-stereo",
                       "description":"SteelSeries Arctis 5","monitor_source":"x"}]"#;

        let sources: Vec<SourceInfo> = parse_json(raw).expect("parse sources");

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "alsa_input.usb-mic.analog-stereo");
        assert_eq!(sources[0].description, "SteelSeries Arctis 5");
    }

    #[test]
    fn parses_sink_inputs_with_numeric_and_string_owner_module() {
        let raw = r#"[{"index":516,"owner_module":25,"corked":false},
                       {"index":517,"owner_module":"26"},
                       {"index":518,"owner_module":null}]"#;

        let inputs: Vec<SinkInputInfo> = parse_json(raw).expect("parse sink inputs");

        assert_eq!(inputs.len(), 3);
        assert_eq!(inputs[0].owner_module, Some(25));
        assert_eq!(inputs[1].owner_module, Some(26));
        assert_eq!(inputs[2].owner_module, None);
    }
}
