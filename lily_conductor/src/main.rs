pub mod machine_score;
pub mod playback;

use std::{env, fs, process::ExitCode};

use lilyparse::syntax::parse;
use machine_score::MachineScore;

use crate::playback::{
    CommandTimeline,
    string_volume::{StringVolumeRange, StringVolumeTable},
};

/// Volume span applied on top of each calibrated minimum to derive the maximum.
/// Tune this after running pluck_calibration to taste.
const CALIBRATION_VOLUME_SPAN: u8 = 70;

/// Fallback range used for any (string, fret) not present in the calibration CSV,
/// and for the entire table when no CSV is provided.
const FALLBACK_RANGE: StringVolumeRange = StringVolumeRange { min: 60, max: 100 };

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

    let _time_line = CommandTimeline::from_machine_score_with_volume_table(&machine_score, &volume_table);

    println!(
        "Processed '{}' into MachineScore (title='{}', parts={}).",
        file_path,
        machine_score.title,
        machine_score.parts.len()
    );

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
