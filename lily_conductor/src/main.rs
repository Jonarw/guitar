pub mod machine_score;
pub mod playback;

use std::{env, fs, process::ExitCode};

use lilyparse::syntax::parse;
use machine_score::MachineScore;

use crate::{
    machine_score::dynamic::{StringVolumeRange, StringVolumeTable},
    playback::CommandTimeline,
};

fn run() -> Result<(), String> {
    let mut args = env::args();
    let bin_name = args.next().unwrap_or_else(|| "lily_conductor".to_owned());
    let file_path = match (args.next(), args.next()) {
        (Some(path), None) => path,
        _ => {
            return Err(format!("Usage: {bin_name} <path-to-lilypond-file>"));
        }
    };

    let file_contents = fs::read_to_string(&file_path).map_err(|err| format!("Failed to read '{file_path}': {err}"))?;

    let lily_score = parse::parse_score(file_contents.as_str())
        .map_err(|err| format!("Failed to parse LilyPond file '{file_path}':\n{err}"))?;

    let machine_score = MachineScore::from_lilyscore(lily_score);

    let volume_table = StringVolumeTable {
        ranges: [
            StringVolumeRange { min: 149, max: 205 }, // E
            StringVolumeRange { min: 149, max: 220 }, // A
            StringVolumeRange { min: 149, max: 220 }, // D
            StringVolumeRange { min: 149, max: 220 }, // G
            StringVolumeRange { min: 149, max: 220 }, // B
            StringVolumeRange { min: 149, max: 220 }, // e
        ],
    };

    let time_line = CommandTimeline::from_machine_score_with_volume_table(&machine_score, &volume_table);

    println!(
        "Processed '{}' into MachineScore (title='{}', parts={}).",
        file_path,
        machine_score.title,
        machine_score.parts.len()
    );

    Ok(())
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
