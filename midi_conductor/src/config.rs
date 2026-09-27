//! TOML-based configuration for midi_conductor.
//!
//! Discovery order:
//! 1. Path given as the single optional positional CLI argument.
//! 2. `config.toml` next to the executable.
//! 3. Built-in defaults (no file required).

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub enum MidiChannelMode {
    // Ch0 -> e, Ch1 -> B ...
    Fixed,
    // engine decides which string to play each note on
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Serial device of the RS485 interface.
    pub serial_port: String,
    /// Serial baud rate.
    pub baud_rate: u32,
    /// Fixed lookahead latency in milliseconds. All hardware commands are
    /// scheduled this far after the MIDI event that triggers them, leaving
    /// room for fret/volume lead times.
    pub latency_ms: u64,
    /// Pluck-volume calibration CSV (as produced by `pluck_calibration`).
    /// Relative paths are resolved against the directory of the config file.
    pub calibration_csv: PathBuf,
    /// Substring of the MIDI input port to subscribe to.
    pub midi_port_substring: String,

    pub midi_channel_mode: MidiChannelMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            serial_port: "/dev/ttyUSB0".to_owned(),
            baud_rate: 115_200,
            latency_ms: 250,
            calibration_csv: PathBuf::from("calibration.csv"),
            midi_port_substring: "Midi Through Port-0".to_owned(),
            midi_channel_mode: MidiChannelMode::Auto,
        }
    }
}

impl Config {
    /// Loads the configuration. `arg` is the optional positional CLI argument
    /// (path to a config file). Returns the config and the directory relative
    /// paths inside it are resolved against.
    pub fn load(arg: Option<String>) -> Result<(Self, PathBuf), String> {
        let path = match arg {
            Some(p) => Some(PathBuf::from(p)),
            None => default_config_path().filter(|p| p.exists()),
        };

        match path {
            Some(path) => {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read config '{}': {e}", path.display()))?;
                let config: Config = toml::from_str(&content)
                    .map_err(|e| format!("Failed to parse config '{}': {e}", path.display()))?;
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(PathBuf::new);
                eprintln!("Loaded config from '{}'.", path.display());
                Ok((config, dir))
            }
            None => {
                eprintln!("No config file found, using defaults.");
                Ok((Self::default(), PathBuf::from(".")))
            }
        }
    }

    /// Resolves `calibration_csv` against the config file's directory.
    pub fn calibration_path(&self, config_dir: &Path) -> PathBuf {
        if self.calibration_csv.is_absolute() {
            self.calibration_csv.clone()
        } else {
            config_dir.join(&self.calibration_csv)
        }
    }
}

fn default_config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|dir| dir.join("config.toml"))
}
