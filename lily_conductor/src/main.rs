pub mod machine_score;
pub mod playback;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

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

    // Parse: <lily-file> [--calibration <csv-path>] [--export] [--start_bar <n>] [--end_bar <n>] [--string <strings>]
    let (file_path, calibration_path, export, start_bar, end_bar, enabled_strings) = parse_args(&bin_name, &args)?;

    let file_contents = fs::read_to_string(file_path).map_err(|err| format!("Failed to read '{file_path}': {err}"))?;

    let lily_score = parse::parse_score(file_contents.as_str())
        .map_err(|err| format!("Failed to parse LilyPond file '{file_path}':\n{err}"))?;

    let machine_score = MachineScore::from_lilyscore(lily_score);
    let mut machine_score = if start_bar.is_some() || end_bar.is_some() {
        machine_score.extract_bar_range(start_bar.unwrap_or(1), end_bar)?
    } else {
        machine_score
    };

    machine_score.pre_process();
    let volume_table = build_volume_table(calibration_path)?;

    let timeline = CommandTimeline::from_machine_score_with_string_filter(
        &machine_score,
        &volume_table,
        enabled_strings.as_deref(),
    );

    if export {
        let export_path = export_path_for(file_path);
        let script = playback::export::to_conductor_script(&timeline)?;
        fs::write(&export_path, script).map_err(|err| format!("Failed to write '{}': {err}", export_path.display()))?;
        println!(
            "Exported {} command(s) to '{}'.",
            timeline.commands.len(),
            export_path.display()
        );
        return Ok(());
    }

    let mut port = serialport::new(DEFAULT_SERIAL_PORT, BAUD_RATE)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Failed to open serial port '{DEFAULT_SERIAL_PORT}': {e}"))?;

    playback::player::play(&timeline, &mut *port).map_err(|e| format!("Playback error: {e}"))?;

    println!("Done.");
    Ok(())
}

/// Derives the export file path from the input LilyPond path by replacing its
/// extension with `.txt` (e.g. `song.ly` becomes `song.txt`).
fn export_path_for(lily_path: &str) -> PathBuf {
    Path::new(lily_path).with_extension("txt")
}

fn parse_bar_number(flag: &str, value: Option<&String>, usage: &str) -> Result<u32, String> {
    value
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&n| n >= 1)
        .ok_or_else(|| format!("{flag} requires a positive integer bar number\n{usage}"))
}

/// Parses a `--string` value such as `EAD` into the corresponding strings.
/// `e` denotes the high-E string, `E` the low-E string; `B`, `G`, `D`, `A` as expected.
fn parse_strings(value: Option<&String>, usage: &str) -> Result<Vec<protocol::GuitarString>, String> {
    use protocol::GuitarString;
    let value = value.ok_or_else(|| format!("--string requires a string argument (e.g. --string EAD)\n{usage}"))?;
    if value.is_empty() {
        return Err(format!(
            "--string requires a non-empty string argument (e.g. --string EAD)\n{usage}"
        ));
    }
    value
        .chars()
        .map(|c| {
            match c {
                'e' => Ok(GuitarString::e),
                'E' => Ok(GuitarString::E),
                'A' => Ok(GuitarString::A),
                'D' => Ok(GuitarString::D),
                'G' => Ok(GuitarString::G),
                'B' => Ok(GuitarString::B),
                _ => Err(format!(
                    "Invalid string '{c}' in --string argument; valid strings are e, B, G, D, A, E (from high to low)\n{usage}"
                )),
            }
        })
        .collect()
}

fn parse_args<'a>(
    bin_name: &str,
    args: &'a [String],
) -> Result<
    (
        &'a str,
        Option<&'a str>,
        bool,
        Option<u32>,
        Option<u32>,
        Option<Vec<protocol::GuitarString>>,
    ),
    String,
> {
    let usage = format!(
        "Usage: {bin_name} <path-to-lilypond-file> [--calibration <csv-path>] [--export] [--start_bar <n>] [--end_bar <n>] [--string <strings>]"
    );
    let mut lily_path: Option<&str> = None;
    let mut calibration_path: Option<&str> = None;
    let mut export = false;
    let mut start_bar: Option<u32> = None;
    let mut end_bar: Option<u32> = None;
    let mut enabled_strings: Option<Vec<protocol::GuitarString>> = None;
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
            "--export" => {
                export = true;
            }
            "--start_bar" => {
                i += 1;
                start_bar = Some(parse_bar_number("--start_bar", args.get(i), &usage)?);
            }
            "--end_bar" => {
                i += 1;
                end_bar = Some(parse_bar_number("--end_bar", args.get(i), &usage)?);
            }
            "--string" => {
                i += 1;
                enabled_strings = Some(parse_strings(args.get(i), &usage)?);
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
    if let (Some(start), Some(end)) = (start_bar, end_bar)
        && end < start
    {
        return Err(format!(
            "--end_bar ({end}) must not be smaller than --start_bar ({start})\n{usage}"
        ));
    }
    Ok((lily_path, calibration_path, export, start_bar, end_bar, enabled_strings))
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
