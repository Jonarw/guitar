use clap::Parser;
use protocol::{Fret, GuitarString, MessageAction, MessageFrame};
use std::io::{self, Write};
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    port: String,

    #[arg(short, long)]
    baud_rate: u32,
}

fn main() -> anyhow::Result<()> {
    let args = Args {
        port: "/dev/ttyUSB0".to_string(),
        baud_rate: 115200,
    };
    // let args = Args::parse();

    let mut port = serialport::new(&args.port, args.baud_rate)
        .timeout(Duration::from_millis(100))
        .open()
        .expect("Failed to open serial port");

    println!("Connected to {}.", args.port);
    println!("Available commands: F[S][N], P[S], D[S][N], R[S][N], V[S][u8], E[S], I[S]");

    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            break;
        }

        match parse_command(input) {
            Ok(msg) => {
                let buffer = msg.cobs_encode();
                port.write_all(&buffer)?;
            }
            Err(e) => {
                eprintln!("Parse error: {e}");
            }
        }
    }

    Ok(())
}

fn parse_command(line: &str) -> Result<MessageFrame, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("empty command".to_string());
    }

    let mut chars = trimmed.chars();
    let command = chars.next().unwrap();
    let rest = chars.as_str();

    match command {
        'F' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(MessageFrame::new(MessageAction::FretFast, string, fret, 0.into()))
        }
        'f' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(MessageFrame::new(MessageAction::FretQuiet, string, fret, 0.into()))
        }
        'C' | 'c' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(MessageFrame::new(
                MessageAction::FretCalibration,
                string,
                fret,
                0.into(),
            ))
        }
        'P' | 'p' => {
            let string = parse_string_only(rest)?;
            Ok(MessageFrame::new(MessageAction::Pluck, string, Fret::NoFret, 0.into()))
        }
        'E' | 'e' => {
            let string = parse_string_only(rest)?;
            Ok(MessageFrame::new(
                MessageAction::PluckEnable,
                string,
                Fret::NoFret,
                0.into(),
            ))
        }
        'I' | 'i' => {
            let string = parse_string_only(rest)?;
            Ok(MessageFrame::new(
                MessageAction::PluckDisable,
                string,
                Fret::NoFret,
                0.into(),
            ))
        }
        'D' | 'd' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(MessageFrame::new(MessageAction::Dampen, string, fret, 0.into()))
        }
        'R' | 'r' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(MessageFrame::new(MessageAction::Unfret, string, fret, 0.into()))
        }
        'V' | 'v' => {
            let (string, volume) = parse_string_and_volume(rest)?;
            Ok(MessageFrame::new(
                MessageAction::PluckVolume,
                string,
                Fret::NoFret,
                volume.into(),
            ))
        }
        _ => Err(format!("unknown command '{}'; expected F, P, D, R, C or V", command)),
    }
}

fn parse_string_only(rest: &str) -> Result<GuitarString, String> {
    let mut chars = rest.chars();
    let string_char = chars
        .next()
        .ok_or_else(|| "missing string; expected one of E,A,D,G,B,e".to_string())?;

    if chars.next().is_some() {
        return Err("unexpected extra input after string".to_string());
    }

    parse_guitar_string(string_char)
}

fn parse_string_and_fret(rest: &str) -> Result<(GuitarString, Fret), String> {
    let mut chars = rest.chars();
    let string_char = chars
        .next()
        .ok_or_else(|| "missing string; expected one of E,A,D,G,B,e".to_string())?;
    let string = parse_guitar_string(string_char)?;

    let fret_str = chars.as_str();
    if fret_str.is_empty() {
        return Err("missing fret number; expected 1-18".to_string());
    }

    let fret_number = fret_str
        .parse::<u8>()
        .map_err(|_| "invalid fret number; expected 1-18".to_string())?;
    let fret = parse_fret(fret_number)?;

    Ok((string, fret))
}

fn parse_guitar_string(value: char) -> Result<GuitarString, String> {
    match value {
        'E' => Ok(GuitarString::E),
        'A' => Ok(GuitarString::A),
        'D' => Ok(GuitarString::D),
        'G' => Ok(GuitarString::G),
        'B' => Ok(GuitarString::B),
        'e' => Ok(GuitarString::e),
        _ => Err(format!("invalid string '{}'; expected one of E,A,D,G,B,e", value)),
    }
}

fn parse_fret(value: u8) -> Result<Fret, String> {
    match value {
        1 => Ok(Fret::Fret1),
        2 => Ok(Fret::Fret2),
        3 => Ok(Fret::Fret3),
        4 => Ok(Fret::Fret4),
        5 => Ok(Fret::Fret5),
        6 => Ok(Fret::Fret6),
        7 => Ok(Fret::Fret7),
        8 => Ok(Fret::Fret8),
        9 => Ok(Fret::Fret9),
        10 => Ok(Fret::Fret10),
        11 => Ok(Fret::Fret11),
        12 => Ok(Fret::Fret12),
        13 => Ok(Fret::Fret13),
        14 => Ok(Fret::Fret14),
        15 => Ok(Fret::Fret15),
        16 => Ok(Fret::Fret16),
        17 => Ok(Fret::Fret17),
        18 => Ok(Fret::Fret18),
        _ => Err(format!("invalid fret {}; expected 1-18", value)),
    }
}

fn parse_volume(rest: &str) -> Result<u8, String> {
    if rest.is_empty() {
        return Err("missing volume; expected 0-255".to_string());
    }

    rest.parse::<u8>()
        .map_err(|_| "invalid volume; expected 0-255".to_string())
}

fn parse_string_and_volume(rest: &str) -> Result<(GuitarString, u8), String> {
    let mut chars = rest.chars();
    let string_char = chars
        .next()
        .ok_or_else(|| "missing string; expected one of E,A,D,G,B,e".to_string())?;
    let string = parse_guitar_string(string_char)?;

    let volume_str = chars.as_str();
    if volume_str.is_empty() {
        return Err("missing volume; expected 0-255".to_string());
    }

    let volume = parse_volume(volume_str)?;
    Ok((string, volume))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fret_command() {
        let frame = parse_command("FD6").unwrap();
        assert_eq!(frame.action, MessageAction::Fret);
        assert_eq!(frame.string, GuitarString::D);
        assert_eq!(frame.fret, Fret::Fret6);
    }

    #[test]
    fn parses_pluck_command() {
        let frame = parse_command("Pe").unwrap();
        assert_eq!(frame.action, MessageAction::Pluck);
        assert_eq!(frame.string, GuitarString::e);
        assert_eq!(frame.fret, Fret::NoFret);
    }

    #[test]
    fn parses_dampen_and_unfret_commands() {
        let dampen = parse_command("DB10").unwrap();
        assert_eq!(dampen.action, MessageAction::Dampen);
        assert_eq!(dampen.string, GuitarString::B);
        assert_eq!(dampen.fret, Fret::Fret10);

        let unfret = parse_command("RE18").unwrap();
        assert_eq!(unfret.action, MessageAction::Unfret);
        assert_eq!(unfret.string, GuitarString::E);
        assert_eq!(unfret.fret, Fret::Fret18);
    }

    #[test]
    fn parses_volume_command() {
        let frame = parse_command("VD255").unwrap();
        assert_eq!(frame.action, MessageAction::Volume);
        assert_eq!(frame.string, GuitarString::D);
        assert_eq!(frame.pluck_volume.volume(), 255);
    }

    #[test]
    fn rejects_invalid_fret_and_volume() {
        assert!(parse_command("FA0").is_err());
        assert!(parse_command("VD256").is_err());
        assert!(parse_command("V256").is_err());
    }
}
