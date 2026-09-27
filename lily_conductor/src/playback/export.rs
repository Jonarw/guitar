use protocol::{Fret, GuitarString, Message, PluckTechnique};

use super::timeline::CommandTimeline;

/// The exported script uses a fixed tempo of 60 BPM, so one beat equals exactly one
/// second.  This preserves the timeline's absolute millisecond timing (including any
/// tempo changes in the score) in `conductor`'s single-tempo script format.
const EXPORT_BPM: u32 = 60;

/// Converts a [`CommandTimeline`] into a script in the text format used by `conductor`:
///
/// ```text
/// tempo <bpm>
/// <beat> <action> [args...]
/// ```
///
/// The result can be played back offline with `conductor --script <path>`.
pub fn to_conductor_script(timeline: &CommandTimeline) -> Result<String, String> {
    let mut script = format!("tempo {EXPORT_BPM}\n");

    for cmd in &timeline.commands {
        let beat = cmd.time_ms as f64 / 1000.0;
        script.push_str(&format!("{beat:.3} {}\n", format_action(&cmd.message)?));
    }

    Ok(script)
}

fn format_action(message: &Message) -> Result<String, String> {
    Ok(match message {
        Message::Reset => "reset".to_owned(),
        Message::Pluck(s) => format!("pluck {}", string_name(*s)),
        Message::PluckVolume(s, v) => format!("pluck_volume {} {}", string_name(*s), v.volume()),
        Message::PluckEnable(s) => format!("pluck_enable {}", string_name(*s)),
        Message::PluckDisable(s) => format!("pluck_disable {}", string_name(*s)),
        Message::FretFast(s, f) => format!("fret_fast {} {}", string_name(*s), fret_number(*f)),
        Message::FretQuiet(s, f) => format!("fret_quiet {} {}", string_name(*s), fret_number(*f)),
        Message::Unfret(s, f) => format!("unfret {} {}", string_name(*s), fret_number(*f)),
        Message::UnfretFast(s, f) => format!("unfret_fast {} {}", string_name(*s), fret_number(*f)),
        Message::Dampen(s, f) => format!("dampen {} {}", string_name(*s), fret_number(*f)),
        Message::PluckTechnique(s, t) => format!("pluck_technique {} {}", string_name(*s), pluck_technique(*t)),
        other => {
            return Err(format!(
                "message {other:?} cannot be represented in the conductor script format"
            ));
        }
    })
}

fn string_name(s: GuitarString) -> &'static str {
    match s {
        GuitarString::E => "E",
        GuitarString::A => "A",
        GuitarString::D => "D",
        GuitarString::G => "G",
        GuitarString::B => "B",
        GuitarString::e => "e",
    }
}

fn fret_number(f: Fret) -> u8 {
    f as u8
}

fn pluck_technique(t: PluckTechnique) -> &'static str {
    match t {
        PluckTechnique::Soft => "soft",
        PluckTechnique::Hard => "hard",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::timeline::TimedCommand;

    #[test]
    fn exports_conductor_script() {
        let timeline = CommandTimeline {
            commands: vec![
                TimedCommand {
                    time_ms: 0,
                    message: Message::Reset,
                },
                TimedCommand {
                    time_ms: 1000,
                    message: Message::PluckEnable(GuitarString::E),
                },
                TimedCommand {
                    time_ms: 1480,
                    message: Message::FretQuiet(GuitarString::E, Fret::Fret5),
                },
                TimedCommand {
                    time_ms: 1500,
                    message: Message::PluckVolume(GuitarString::E, 200.into()),
                },
                TimedCommand {
                    time_ms: 1700,
                    message: Message::Pluck(GuitarString::E),
                },
                TimedCommand {
                    time_ms: 2200,
                    message: Message::Unfret(GuitarString::E, Fret::Fret5),
                },
                TimedCommand {
                    time_ms: 2250,
                    message: Message::Dampen(GuitarString::e, Fret::Fret11),
                },
            ],
        };

        let script = to_conductor_script(&timeline).unwrap();
        let expected = "tempo 60\n\
             0.000 reset\n\
             1.000 pluck_enable E\n\
             1.480 fret_quiet E 5\n\
             1.500 pluck_volume E 200\n\
             1.700 pluck E\n\
             2.200 unfret E 5\n\
             2.250 dampen e 11\n";
        assert_eq!(script, expected);
    }

    #[test]
    fn rejects_unrepresentable_messages() {
        let timeline = CommandTimeline {
            commands: vec![TimedCommand {
                time_ms: 0,
                message: Message::ConfirmPresence,
            }],
        };

        assert!(to_conductor_script(&timeline).is_err());
    }
}
