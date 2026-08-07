pub mod machine_score;
pub mod playback;

use std::{env, fs, process::ExitCode};

use lilyparse::syntax::parse;
use machine_score::MachineScore;

use crate::playback::{CommandTimeline, string_volume::StringVolumeTable};

/// Default serial port for the guitar RS485 interface.
const DEFAULT_SERIAL_PORT: &str = "/dev/ttyUSB0";
/// Serial baud rate.
const BAUD_RATE: u32 = 115_200;

fn build_volume_table(calibration_path: Option<&str>) -> Result<StringVolumeTable, String> {
    match calibration_path {
        Some(path) => {
            let table = StringVolumeTable::from_csv(path).map_err(|e| format!("Calibration CSV error: {e}"))?;
            eprintln!("Loaded calibration from '{path}'.");
            Ok(table)
        }
        None => {
            return Err("No path".to_owned());
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args();
    let bin_name = args.next().unwrap_or_else(|| "lily_conductor".to_owned());
    let args: Vec<String> = args.collect();

    // Parse: <lily-file> [--calibration <csv-path>]
    let (file_path, calibration_path) = parse_args(&bin_name, &args)?;

    let file_contents = fs::read_to_string(file_path).map_err(|err| format!("Failed to read '{file_path}': {err}"))?;

    let lily_score = parse::parse_score(file_contents.as_str())
        .map_err(|err| format!("Failed to parse LilyPond file '{file_path}':\n{err}"))?;

    let machine_score = MachineScore::from_lilyscore(lily_score);
    let volume_table = build_volume_table(calibration_path)?;

    let timeline = CommandTimeline::from_machine_score_with_volume_table(&machine_score, &volume_table);

    let mut port = serialport::new(DEFAULT_SERIAL_PORT, BAUD_RATE)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Failed to open serial port '{DEFAULT_SERIAL_PORT}': {e}"))?;

    playback::player::play(&timeline, &mut *port).map_err(|e| format!("Playback error: {e}"))?;

    println!("Done.");
    Ok(())
}

fn parse_args<'a>(bin_name: &str, args: &'a [String]) -> Result<(&'a str, Option<&'a str>), String> {
    let usage = format!("Usage: {bin_name} <path-to-lilypond-file> [--calibration <csv-path>]");
    let mut lily_path: Option<&str> = None;
    let mut calibration_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--calibration" => {
                i += 1;
                calibration_path = Some(
                    args.get(i)
                        .map(|s| s.as_str())
                        .ok_or_else(|| format!("--calibration requires a path argument\n{usage}"))?,
                );
            }
            arg if !arg.starts_with('-') => {
                if lily_path.is_some() {
                    return Err(format!("Unexpected argument '{arg}'\n{usage}"));
                }
                lily_path = Some(arg);
            }
            arg => return Err(format!("Unknown argument '{arg}'\n{usage}")),
        }
        i += 1;
    }
    let lily_path = lily_path.ok_or_else(|| usage.clone())?;
    Ok((lily_path, calibration_path))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
