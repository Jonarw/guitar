use anyhow::{Context, Result, bail};
use protocol::{Fret, GuitarString, Message, PluckTechnique};
use std::env;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct ScheduledCommand {
    beat: f64,
    message: Message,
    line_no: usize,
}

#[derive(Debug)]
struct Sequence {
    bpm: f64,
    commands: Vec<ScheduledCommand>,
}

#[derive(Debug)]
struct Cli {
    script_path: String,
    port: String,
    baud_rate: u32,
}

fn main() -> Result<()> {
    let cli = parse_cli()?;
    let script = fs::read_to_string(&cli.script_path)
        .with_context(|| format!("failed to read script file '{}'", cli.script_path))?;
    let sequence = parse_sequence(&script)?;

    println!(
        "Loaded {} command(s) at {} BPM from {}",
        sequence.commands.len(),
        sequence.bpm,
        cli.script_path
    );

    let mut port = serialport::new(&cli.port, cli.baud_rate)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("failed to open serial port '{}'", cli.port))?;

    println!("Connected to {} @ {} baud", cli.port, cli.baud_rate);
    run_sequence(&mut *port, &sequence)?;
    println!("Sequence complete.");

    Ok(())
}

fn parse_cli() -> Result<Cli> {
    let mut args = env::args().skip(1);

    let mut script_path: Option<String> = None;
    let mut port = "/dev/ttyUSB0".to_string();
    let mut baud_rate = 115_200_u32;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--script" | "-s" => {
                script_path = Some(args.next().context("missing value after --script/-s")?);
            }
            "--port" | "-p" => {
                port = args.next().context("missing value after --port/-p")?;
            }
            "--baud" | "--baud-rate" | "-b" => {
                let value = args.next().context("missing value after --baud/--baud-rate/-b")?;
                baud_rate = value
                    .parse::<u32>()
                    .with_context(|| format!("invalid baud rate '{}', expected integer", value))?;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            unknown => bail!("unknown argument '{}'. Use --help for usage.", unknown),
        }
    }

    let script_path = script_path.context("missing required argument --script <path>")?;

    Ok(Cli {
        script_path,
        port,
        baud_rate,
    })
}

fn print_usage() {
    println!("conductor: play command sequences on RS485 guitar hardware");
    println!();
    println!("Usage:");
    println!("  cargo run -- --script <path> [--port /dev/ttyUSB0] [--baud 115200]");
    println!();
    println!("Script format:");
    println!("  tempo <bpm>");
    println!("  <beat> <action> [args...]");
    println!();
    println!("Actions:");
    println!("  pluck <string>");
    println!("  pluck_volume <string> <0-255>");
    println!("  pluck_speed <string> <0-65535>");
    println!("  pluck_technique <string> <soft|hard>");
    println!("  pluck_enable <string>");
    println!("  pluck_disable <string>");
    println!("  fret_fast <string> <0-18>");
    println!("  fret_quiet <string> <0-18>");
    println!("  fret_adaptive <string> <0-18>");
    println!("  unfret <string> <0-18>");
    println!("  dampen <string> <0-18>");
    println!();
    println!("Strings: E A D G B e");
    println!("Comments: use '#' to comment a line or line tail");
}

fn parse_sequence(script: &str) -> Result<Sequence> {
    let mut bpm: Option<f64> = None;
    let mut commands = Vec::new();
    let mut previous_beat = 0.0_f64;

    for (index, raw_line) in script.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        if bpm.is_none() {
            if parts.len() != 2 {
                bail!("line {}: first non-empty line must be 'tempo <bpm>'", line_no);
            }

            match parts[0] {
                "tempo" | "bpm" => {
                    let parsed = parts[1]
                        .parse::<f64>()
                        .with_context(|| format!("line {}: invalid bpm '{}', expected number", line_no, parts[1]))?;
                    if !(parsed.is_finite() && parsed > 0.0) {
                        bail!("line {}: bpm must be > 0", line_no);
                    }
                    bpm = Some(parsed);
                    continue;
                }
                _ => bail!("line {}: first non-empty line must be 'tempo <bpm>'", line_no),
            }
        }

        if parts.len() < 2 {
            bail!("line {}: expected '<beat> <action> [args...]'", line_no);
        }

        let beat = parts[0]
            .parse::<f64>()
            .with_context(|| format!("line {}: invalid beat '{}', expected decimal", line_no, parts[0]))?;
        if !(beat.is_finite() && beat >= 0.0) {
            bail!("line {}: beat must be >= 0", line_no);
        }
        if !commands.is_empty() && beat < previous_beat {
            bail!(
                "line {}: beat {} is earlier than previous beat {}",
                line_no,
                beat,
                previous_beat
            );
        }

        let message = parse_action(parts[1], &parts[2..], line_no)?;
        commands.push(ScheduledCommand { beat, message, line_no });
        previous_beat = beat;
    }

    let bpm = bpm.context("script is missing 'tempo <bpm>'")?;
    Ok(Sequence { bpm, commands })
}

fn strip_comment(line: &str) -> &str {
    match line.split_once('#') {
        Some((head, _)) => head,
        None => line,
    }
}

fn parse_action(action: &str, args: &[&str], line_no: usize) -> Result<Message> {
    match action {
        "pluck" => {
            ensure_len(args, 1, line_no, action)?;
            Ok(Message::Pluck(parse_guitar_string(args[0], line_no)?))
        }
        "pluck_volume" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let volume = parse_u8(args[1], line_no, "volume")?;
            Ok(Message::PluckVolume(string, volume.into()))
        }
        "pluck_technique" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let technique = parse_pluck_technique(args[1], line_no)?;
            Ok(Message::PluckTechnique(string, technique))
        }
        "pluck_enable" => {
            ensure_len(args, 1, line_no, action)?;
            Ok(Message::PluckEnable(parse_guitar_string(args[0], line_no)?))
        }
        "pluck_disable" => {
            ensure_len(args, 1, line_no, action)?;
            Ok(Message::PluckDisable(parse_guitar_string(args[0], line_no)?))
        }
        "fret_fast" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let fret = parse_fret(args[1], line_no)?;
            Ok(Message::FretFast(string, fret))
        }
        "fret_quiet" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let fret = parse_fret(args[1], line_no)?;
            Ok(Message::FretQuiet(string, fret))
        }
        "unfret" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let fret = parse_fret(args[1], line_no)?;
            Ok(Message::Unfret(string, fret))
        }
        "unfret_fast" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let fret = parse_fret(args[1], line_no)?;
            Ok(Message::UnfretFast(string, fret))
        }
        "dampen" => {
            ensure_len(args, 2, line_no, action)?;
            let string = parse_guitar_string(args[0], line_no)?;
            let fret = parse_fret(args[1], line_no)?;
            Ok(Message::Dampen(string, fret))
        }
        "reset" => {
            ensure_len(args, 0, line_no, action)?;
            Ok(Message::Reset)
        }
        _ => bail!(
            "line {}: unknown action '{}'; expected one of pluck, pluck_volume, pluck_speed, pluck_technique, pluck_enable, pluck_disable, fret_fast, fret_quiet, fret_adaptive, unfret, dampen",
            line_no,
            action
        ),
    }
}

fn ensure_len(args: &[&str], expected: usize, line_no: usize, action: &str) -> Result<()> {
    if args.len() != expected {
        bail!(
            "line {}: action '{}' expects {} argument(s), got {}",
            line_no,
            action,
            expected,
            args.len()
        );
    }
    Ok(())
}

fn parse_guitar_string(value: &str, line_no: usize) -> Result<GuitarString> {
    match value {
        "E" => Ok(GuitarString::E),
        "A" => Ok(GuitarString::A),
        "D" => Ok(GuitarString::D),
        "G" => Ok(GuitarString::G),
        "B" => Ok(GuitarString::B),
        "e" => Ok(GuitarString::e),
        _ => bail!(
            "line {}: invalid string '{}'; expected one of E, A, D, G, B, e",
            line_no,
            value
        ),
    }
}

fn parse_fret(value: &str, line_no: usize) -> Result<Fret> {
    let fret_number = value
        .parse::<u8>()
        .with_context(|| format!("line {}: invalid fret '{}', expected 0-18", line_no, value))?;

    let fret = match fret_number {
        0 => Fret::NoFret,
        1 => Fret::Fret1,
        2 => Fret::Fret2,
        3 => Fret::Fret3,
        4 => Fret::Fret4,
        5 => Fret::Fret5,
        6 => Fret::Fret6,
        7 => Fret::Fret7,
        8 => Fret::Fret8,
        9 => Fret::Fret9,
        10 => Fret::Fret10,
        11 => Fret::Fret11,
        12 => Fret::Fret12,
        13 => Fret::Fret13,
        14 => Fret::Fret14,
        15 => Fret::Fret15,
        16 => Fret::Fret16,
        17 => Fret::Fret17,
        18 => Fret::Fret18,
        _ => bail!("line {}: invalid fret '{}', expected 0-18", line_no, value),
    };

    Ok(fret)
}

fn parse_u8(value: &str, line_no: usize, field_name: &str) -> Result<u8> {
    value
        .parse::<u8>()
        .with_context(|| format!("line {}: invalid {} '{}', expected 0-255", line_no, field_name, value))
}

fn parse_pluck_technique(value: &str, line_no: usize) -> Result<PluckTechnique> {
    match value {
        "soft" => Ok(PluckTechnique::Soft),
        "hard" => Ok(PluckTechnique::Hard),
        _ => bail!("line {}: invalid technique '{}'; expected soft or hard", line_no, value),
    }
}

fn run_sequence(port: &mut dyn serialport::SerialPort, sequence: &Sequence) -> Result<()> {
    let seconds_per_beat = 60.0 / sequence.bpm;
    let started = Instant::now();

    for command in &sequence.commands {
        let due = Duration::from_secs_f64(command.beat * seconds_per_beat);
        let now = started.elapsed();
        if due > now {
            thread::sleep(due - now);
        }

        let mut buffer = [0_u8; protocol::MAX_FRAME_SIZE];
        let frame = command.message.encode(&mut buffer).with_context(|| {
            format!(
                "line {}: failed to encode command at beat {}",
                command.line_no, command.beat
            )
        })?;
        port.write_all(frame).with_context(|| {
            format!(
                "line {}: failed to write frame for command at beat {}",
                command.line_no, command.beat
            )
        })?;

        println!(
            "t={:.3}s beat={:.3} sent {:?}",
            started.elapsed().as_secs_f64(),
            command.beat,
            command.message
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script() {
        let input = r#"
            # Quarter note = 500ms
            tempo 120

            0.0 fret_fast E 3
            0.0 pluck E
            1.0 unfret E 3
            1.0 dampen E 0
        "#;

        let sequence = parse_sequence(input).unwrap();
        assert_eq!(sequence.commands.len(), 4);
        assert_eq!(sequence.commands[0].beat, 0.0);
        assert_eq!(sequence.commands[1].beat, 0.0);
        assert_eq!(sequence.commands[2].beat, 1.0);
    }

    #[test]
    fn rejects_non_chronological_script() {
        let input = r#"
            tempo 100
            1.0 pluck E
            0.5 pluck A
        "#;

        let err = parse_sequence(input).unwrap_err().to_string();
        assert!(err.contains("earlier than previous beat"));
    }
}
