use clap::Parser;
use protocol::{Fret, GuitarString, Message, PluckTechnique};
use std::io::{self, Write};
use std::time::Duration as StdDuration;

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
        .timeout(StdDuration::from_millis(100))
        .open()
        .expect("Failed to open serial port");

    println!("Connected to {}.", args.port);
    println!(
        "Available commands: F[S][N], f[S][N], A[S][N], C[S][N], c[S][N], P[S], D[S][N], R[S][N], V[S][u8], E[S], I[S], S[S][u16], T[S][soft|hard], O[fret].[id].[value]"
    );

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
                let mut buffer = [0; protocol::MAX_FRAME_SIZE];
                let frame = msg.encode(&mut buffer)?;
                port.write_all(frame)?;
            }
            Err(e) => {
                eprintln!("Parse error: {e}");
            }
        }
    }

    Ok(())
}

fn parse_command(line: &str) -> Result<Message, String> {
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
            Ok(Message::FretFast(string, fret))
        }
        'f' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(Message::FretQuiet(string, fret))
        }
        'C' | 'c' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(Message::FretCalibration(string, fret))
        }
        'P' | 'p' => {
            let string = parse_string_only(rest)?;
            Ok(Message::Pluck(string))
        }
        'E' | 'e' => {
            let string = parse_string_only(rest)?;
            Ok(Message::PluckEnable(string))
        }
        'I' | 'i' => {
            let string = parse_string_only(rest)?;
            Ok(Message::PluckDisable(string))
        }
        'D' | 'd' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(Message::Dampen(string, fret))
        }
        'r' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(Message::Unfret(string, fret))
        }
        'R' => {
            let (string, fret) = parse_string_and_fret(rest)?;
            Ok(Message::UnfretFast(string, fret))
        }
        'V' | 'v' => {
            let (string, volume) = parse_string_and_volume(rest)?;
            Ok(Message::PluckVolume(string, volume.into()))
        }
        'S' | 's' => {
            let (string, speed) = parse_string_and_speed(rest)?;
            Ok(Message::PluckSpeed(string, speed))
        }
        'T' | 't' => {
            let (string, technique) = parse_string_and_technique(rest)?;
            Ok(Message::PluckTechnique(string, technique))
        }
        _ => Err(format!(
            "unknown command '{}'; expected F, f, A, C, c, P, D, R, V, E, I, S, T or O",
            command
        )),
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

fn parse_string_and_speed(rest: &str) -> Result<(GuitarString, u16), String> {
    let mut chars = rest.chars();
    let string_char = chars
        .next()
        .ok_or_else(|| "missing string; expected one of E,A,D,G,B,e".to_string())?;
    let string = parse_guitar_string(string_char)?;

    let speed_str = chars.as_str();
    if speed_str.is_empty() {
        return Err("missing speed; expected 0-65535".to_string());
    }

    let speed = speed_str
        .parse::<u16>()
        .map_err(|_| "invalid speed; expected 0-65535".to_string())?;
    Ok((string, speed))
}

fn parse_string_and_technique(rest: &str) -> Result<(GuitarString, PluckTechnique), String> {
    let mut chars = rest.chars();
    let string_char = chars
        .next()
        .ok_or_else(|| "missing string; expected one of E,A,D,G,B,e".to_string())?;
    let string = parse_guitar_string(string_char)?;

    let technique = match chars.as_str() {
        "soft" => PluckTechnique::Soft,
        "hard" => PluckTechnique::Hard,
        other => return Err(format!("invalid technique '{}'; expected soft or hard", other)),
    };
    Ok((string, technique))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fret_command() {
        let msg = parse_command("FD6").unwrap();
        assert_eq!(msg, Message::FretFast(GuitarString::D, Fret::Fret6));
    }

    #[test]
    fn parses_adaptive_fret_command() {
        let msg = parse_command("AE7").unwrap();
        assert_eq!(msg, Message::FretAdaptive(GuitarString::E, Fret::Fret7));
    }

    #[test]
    fn parses_pluck_command() {
        let msg = parse_command("Pe").unwrap();
        assert_eq!(msg, Message::Pluck(GuitarString::e));
    }

    #[test]
    fn parses_dampen_and_unfret_commands() {
        let dampen = parse_command("DB10").unwrap();
        assert_eq!(dampen, Message::Dampen(GuitarString::B, Fret::Fret10));

        let unfret = parse_command("rE18").unwrap();
        assert_eq!(unfret, Message::Unfret(GuitarString::E, Fret::Fret18));
    }

    #[test]
    fn parses_volume_command() {
        let msg = parse_command("VD255").unwrap();
        assert_eq!(msg, Message::PluckVolume(GuitarString::D, 255u8.into()));
    }

    #[test]
    fn parses_speed_command() {
        let msg = parse_command("SE1000").unwrap();
        assert_eq!(msg, Message::PluckSpeed(GuitarString::E, 1000));
    }

    #[test]
    fn parses_technique_command() {
        let soft = parse_command("TeSoft");
        assert!(soft.is_err());

        let soft = parse_command("Tesoft").unwrap();
        assert_eq!(soft, Message::PluckTechnique(GuitarString::e, PluckTechnique::Soft));

        let hard = parse_command("TAhard").unwrap();
        assert_eq!(hard, Message::PluckTechnique(GuitarString::A, PluckTechnique::Hard));
    }

    #[test]
    fn parses_config_commands() {
        let force = parse_command("O6.1.75").unwrap();
        assert_eq!(
            force,
            Message::Config(Fret::Fret6, ConfigValue::MaxForce(Percentage::new(75)))
        );

        let duration = parse_command("O12.10.420").unwrap();
        assert_eq!(
            duration,
            Message::Config(
                Fret::Fret12,
                ConfigValue::FretAdaptivePhase1Duration(protocol::Duration::new(420))
            )
        );
    }

    #[test]
    fn rejects_invalid_fret_volume_and_config() {
        assert!(parse_command("FA0").is_err());
        assert!(parse_command("VD256").is_err());
        assert!(parse_command("V256").is_err());
        assert!(parse_command("O6.16.6").is_err());
        assert!(parse_command("O6.1.160").is_err());
        assert!(parse_command("O6.1").is_err());
    }
}
