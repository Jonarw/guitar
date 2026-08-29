//! Real-time MIDI playback engine for the automated guitar.
//!
//! Subscribes to a system MIDI port (e.g. ALSA "Midi Through", which MuseScore
//! can send to) and plays incoming notes on the guitar over RS485.
//!
//! Usage: `midi_conductor [path/to/config.toml]`
//! Without an argument, `config.toml` next to the executable is used if it
//! exists; otherwise built-in defaults apply.

mod config;
mod engine;
mod midi_input;
mod scheduler;

use std::process::ExitCode;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use enum_iterator::all;
use protocol::{GuitarString, Message};
use string_volume::{StringVolumeRange, StringVolumeTable};

use crate::config::Config;
use crate::engine::GuitarEngine;
use crate::scheduler::CommandSink;

/// Delay between `Reset`/volumes and `PluckEnable` in the startup prologue.
const PLUCK_ENABLE_DELAY_MS: u64 = 300;
/// Near-zero pluck volume used while idle (must be > 0 for the firmware).
const IDLE_PLUCK_VOLUME: u8 = 10;

fn run() -> Result<(), String> {
    let (config, config_dir) = Config::load(std::env::args().nth(1))?;

    let volume_table = load_volume_table(&config, &config_dir);

    let port = serialport::new(&config.serial_port, config.baud_rate)
        .open()
        .map_err(|e| format!("Failed to open serial port '{}': {e}", config.serial_port))?;

    let mut scheduler = scheduler::spawn(port);

    // Startup prologue: reset, near-zero volumes, then enable the pluckers.
    let now = Instant::now();
    scheduler.schedule(now, Message::Reset);
    for string in all::<GuitarString>() {
        scheduler.schedule(now, Message::PluckVolume(string, IDLE_PLUCK_VOLUME.into()));
        scheduler.schedule(
            now + Duration::from_millis(PLUCK_ENABLE_DELAY_MS),
            Message::PluckEnable(string),
        );
    }

    // Reset the hardware on Ctrl-C.
    let shutdown_scheduler = scheduler.clone();
    ctrlc::set_handler(move || {
        eprintln!("Interrupted, resetting guitar.");
        shutdown_scheduler.send_now(Message::Reset);
        std::process::exit(0);
    })
    .map_err(|e| format!("Failed to install Ctrl-C handler: {e}"))?;

    let (tx, rx) = channel();
    let _midi = midi_input::connect(&config.midi_port_substring, tx)?;

    let mut engine = GuitarEngine::new(volume_table, config.latency_ms);
    let mut sink = scheduler;
    eprintln!("Listening (latency {} ms)...", config.latency_ms);
    for (event_time, event) in rx {
        engine.handle(event_time, event, &mut sink);
    }

    Ok(())
}

fn load_volume_table(config: &Config, config_dir: &std::path::Path) -> StringVolumeTable {
    let path = config.calibration_path(config_dir);
    match StringVolumeTable::from_csv(&path) {
        Ok(table) => {
            eprintln!("Loaded calibration from '{}'.", path.display());
            table
        }
        Err(e) => {
            eprintln!(
                "Could not load calibration from '{}' ({e}); using uniform volume table.",
                path.display()
            );
            StringVolumeTable::uniform(StringVolumeRange { min: 0, max: 255 })
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}
