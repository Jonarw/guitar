use fraction::Zero;
use lilyparse::syntax::ast::Tempo;
use protocol::{Fret, GuitarString, Message};

use crate::machine_score::timing::Notes;
use crate::machine_score::{FingerTechnique, MachineScore, MidiVolume, Note};

/// Guitar string configuration: (string enum, open-string MIDI pitch, max controllable frets).
///
/// Standard tuning. The high-e string has 18 controlled frets; all others have 12.
const STRING_CONFIGS: [(GuitarString, u8, u8); 6] = [
    (GuitarString::E, 40, 12),
    (GuitarString::A, 45, 12),
    (GuitarString::D, 50, 12),
    (GuitarString::G, 55, 12),
    (GuitarString::B, 59, 12),
    (GuitarString::e, 64, 18),
];

const ALL_STRINGS: [GuitarString; 6] = [
    GuitarString::E,
    GuitarString::A,
    GuitarString::D,
    GuitarString::G,
    GuitarString::B,
    GuitarString::e,
];

// ---------------------------------------------------------------------------
// Timing constants (all in milliseconds)
// ---------------------------------------------------------------------------

/// Delay between issuing `FretQuiet` and the subsequent `Pluck`.
const FRET_QUIET_PREP_MS: u64 = 20;
/// Delay between issuing `FretFast` and the subsequent `Pluck`.
const FRET_FAST_PREP_MS: u64 = 20;
/// How early `PluckVolume` is sent before the `Pluck` it applies to.
/// If the gap since the previous pluck is smaller than this, the command is placed
/// at the midpoint between the two plucks instead.
const PLUCK_VOLUME_PREP_MS: u64 = 50;
/// How long to wait after `Dampen` before issuing `Unfret`
/// (gives the string time to fully stop vibrating).
const DAMPEN_SETTLE_MS: u64 = 500;
/// Delay between the end-of-prologue and the first musical note.
/// Gives the hardware time to initialise after `Reset` + `PluckEnable`.
const INIT_DELAY_MS: u64 = 500;

/// Fret used to mute an open string via `Dampen`/`Unfret`.
/// (Open strings have no actuated fret, so we use a nearby one to stop vibration.)
const OPEN_STRING_DAMPEN_FRET: Fret = Fret::Fret11;

/// A protocol message paired with the wall-clock offset (from playback start) at which it
/// should be transmitted.
#[derive(Debug, Clone)]
pub struct TimedCommand {
    pub time_ms: u64,
    pub message: Message,
}

/// A chronologically-sorted sequence of [`TimedCommand`]s that fully describes one
/// playback session, from the initial reset through the final reset.
pub struct CommandTimeline {
    pub commands: Vec<TimedCommand>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn fret_from_number(n: u8) -> Fret {
    match n {
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
        _ => panic!("fret number {n} out of range"),
    }
}

/// Maps a MIDI pitch to the fret that must be pressed on a string whose open pitch is
/// `open_pitch`.  Panics if the pitch cannot be played on this string.
pub fn midi_pitch_to_fret(pitch_value: u8, open_pitch: u8, max_frets: u8) -> Fret {
    assert!(
        pitch_value >= open_pitch,
        "MIDI pitch {pitch_value} is below the open-string pitch {open_pitch}",
    );
    let fret_num = pitch_value - open_pitch;
    assert!(
        fret_num <= max_frets,
        "Fret {fret_num} exceeds the {max_frets} controllable frets on this string",
    );
    fret_from_number(fret_num)
}

/// Scales a MIDI velocity (0–127) to the hardware `PluckVolume` range (0–255).
fn midi_volume_to_pluck_volume(v: MidiVolume) -> u8 {
    (v.volume as u16 * 255 / MidiVolume::MAX_VALUE as u16) as u8
}

/// Returns the fret-preparation delay in ms for a given finger technique.
fn fret_prep_ms(technique: FingerTechnique) -> u64 {
    match technique {
        FingerTechnique::Quiet => FRET_QUIET_PREP_MS,
        FingerTechnique::Loud => FRET_FAST_PREP_MS,
    }
}

/// Returns the duration in milliseconds for a note-fraction at a constant tempo.
///
/// `duration` is expressed in whole notes.  The tempo says "`note_duration` = `bpm` BPM",
/// so one whole note lasts `ratio × 60 000 / bpm` ms.
fn fraction_to_ms(duration: Notes, tempo: &Tempo) -> u64 {
    let (Some(&numer), Some(&denom)) = (duration.numer(), duration.denom()) else {
        return 0;
    };
    if numer == 0 || denom == 0 {
        return 0;
    }
    let ratio = tempo.note_duration.ratio as u64;
    let bpm = tempo.bpm as u64;
    // ms = numer/denom * ratio * 60_000 / bpm
    numer as u64 * ratio * 60_000 / (denom as u64 * bpm)
}

/// Converts a score position (in whole notes) to an absolute wall-clock offset in
/// milliseconds, honouring all tempo changes in `tempo_changes` (sorted by position).
pub fn notes_to_ms(position: Notes, tempo_changes: &[(Notes, Tempo)]) -> u64 {
    if position == Notes::zero() {
        return 0;
    }

    let default_tempo = Tempo::default();
    let mut total_ms = 0u64;
    let mut cursor = Notes::zero();
    let mut current_tempo = default_tempo;

    for &(change_pos, new_tempo) in tempo_changes {
        if change_pos >= position {
            break;
        }
        total_ms += fraction_to_ms(change_pos - cursor, &current_tempo);
        cursor = change_pos;
        current_tempo = new_tempo;
    }

    total_ms += fraction_to_ms(position - cursor, &current_tempo);
    total_ms
}

// ---------------------------------------------------------------------------
// Timeline builder
// ---------------------------------------------------------------------------

impl CommandTimeline {
    /// Converts a [`MachineScore`] into a fully-timed command sequence ready for playback.
    ///
    /// Per-note command ordering:
    /// 1. `FretQuiet` / `FretFast` at `start_ms − fret_prep` (skipped for open strings)
    /// 2. `PluckVolume` at `start_ms` (only when volume changes)
    /// 3. `Pluck` at `start_ms`
    /// 4. If the *next* note on this string starts immediately:
    ///    - No `Dampen`
    ///    - `Unfret` only if the current fret would interfere with the next note
    ///      (i.e. next note is open, or next fret has a lower ordinal)
    /// 5. Otherwise (gap before next note, or last note):
    ///    - `Dampen` at `end_ms`; for open strings, `OPEN_STRING_DAMPEN_FRET` is used
    ///    - `Unfret` at `end_ms + DAMPEN_SETTLE_MS`, but brought forward to just before the
    ///      next note's fret command whenever the next fret has a lower ordinal
    pub fn from_machine_score(score: &MachineScore) -> Self {
        let mut commands: Vec<TimedCommand> = Vec::new();

        // --- Prologue -----------------------------------------------------------
        commands.push(TimedCommand {
            time_ms: 0,
            message: Message::Reset,
        });
        for string in ALL_STRINGS {
            commands.push(TimedCommand {
                time_ms: 0,
                message: Message::PluckEnable(string),
            });
        }

        // --- Musical notes ------------------------------------------------------
        let tempo_changes = &score.tempo_changes;
        let mut score_end_ms = 0u64;

        for (part_idx, part) in score.parts.iter().enumerate() {
            let (guitar_string, open_pitch, max_frets) = STRING_CONFIGS[part_idx];
            Self::build_string_commands(
                &mut commands,
                guitar_string,
                open_pitch,
                max_frets,
                &part.notes,
                tempo_changes,
                &mut score_end_ms,
            );
        }

        // --- Epilogue -----------------------------------------------------------
        commands.push(TimedCommand {
            time_ms: score_end_ms + 1_000,
            message: Message::Reset,
        });

        // Stable sort preserves within-timestamp insertion order.
        commands.sort_by_key(|c| c.time_ms);

        Self { commands }
    }

    fn build_string_commands(
        commands: &mut Vec<TimedCommand>,
        guitar_string: GuitarString,
        open_pitch: u8,
        max_frets: u8,
        notes: &[Note],
        tempo_changes: &[(Notes, Tempo)],
        score_end_ms: &mut u64,
    ) {
        let mut last_pluck_volume: Option<u8> = None;
        let mut prev_pluck_ms: Option<u64> = None;

        for (i, note) in notes.iter().enumerate() {
            let next = notes.get(i + 1);
            let start_ms = notes_to_ms(note.start, tempo_changes) + INIT_DELAY_MS;
            let end_ms = notes_to_ms(note.start + note.length, tempo_changes) + INIT_DELAY_MS;
            *score_end_ms = (*score_end_ms).max(end_ms);

            let fret = midi_pitch_to_fret(note.pitch.pitch, open_pitch, max_frets);
            let prep = fret_prep_ms(note.finger_technique);

            // --- Fret command (before pluck) ------------------------------------
            if fret != Fret::NoFret {
                let fret_msg = match note.finger_technique {
                    FingerTechnique::Quiet => Message::FretQuiet(guitar_string, fret),
                    FingerTechnique::Loud => Message::FretFast(guitar_string, fret),
                };
                commands.push(TimedCommand {
                    time_ms: start_ms.saturating_sub(prep),
                    message: fret_msg,
                });
            }

            // --- Volume (only when it changes) ----------------------------------
            let pluck_vol = midi_volume_to_pluck_volume(note.volume);
            if last_pluck_volume != Some(pluck_vol) {
                // Issue PluckVolume PLUCK_VOLUME_PREP_MS before the pluck so the hardware
                // has time to apply it.  If the previous pluck was closer than that, place
                // it at the midpoint between the two plucks.
                let volume_ms = match prev_pluck_ms {
                    Some(prev) => {
                        let ideal = start_ms.saturating_sub(PLUCK_VOLUME_PREP_MS);
                        if ideal > prev { ideal } else { (prev + start_ms) / 2 }
                    }
                    None => start_ms.saturating_sub(PLUCK_VOLUME_PREP_MS),
                };
                commands.push(TimedCommand {
                    time_ms: volume_ms,
                    message: Message::PluckVolume(guitar_string, pluck_vol.into()),
                });
                last_pluck_volume = Some(pluck_vol);
            }

            // --- Pluck ----------------------------------------------------------
            commands.push(TimedCommand {
                time_ms: start_ms,
                message: Message::Pluck(guitar_string),
            });
            prev_pluck_ms = Some(start_ms);

            // --- End-of-note: dampen / unfret -----------------------------------
            let immediately_followed = next.map(|n| n.start == note.start + note.length).unwrap_or(false);

            if immediately_followed {
                // No damping; but if the current fret would interfere with the next
                // note, we must unfret it before that note's fret command.
                let next = next.unwrap();
                let next_fret = midi_pitch_to_fret(next.pitch.pitch, open_pitch, max_frets);
                let current_interferes =
                    fret != Fret::NoFret && (next_fret == Fret::NoFret || (next_fret as u8) < (fret as u8));

                if current_interferes {
                    let next_start_ms = notes_to_ms(next.start, tempo_changes) + INIT_DELAY_MS;
                    // Unfret just before the next fret command (or next pluck if open).
                    let unfret_ms = if next_fret != Fret::NoFret {
                        next_start_ms.saturating_sub(fret_prep_ms(next.finger_technique))
                    } else {
                        next_start_ms
                    };
                    commands.push(TimedCommand {
                        time_ms: unfret_ms,
                        message: Message::Unfret(guitar_string, fret),
                    });
                }
            } else {
                // Dampen to stop the string vibrating.
                let dampen_fret = if fret == Fret::NoFret {
                    OPEN_STRING_DAMPEN_FRET
                } else {
                    fret
                };
                commands.push(TimedCommand {
                    time_ms: end_ms,
                    message: Message::Dampen(guitar_string, dampen_fret),
                });

                // Unfret after the string has had time to stop, but not so late
                // that it would block a next note.
                let default_unfret_ms = end_ms + DAMPEN_SETTLE_MS;
                let unfret_ms = if let Some(next) = next {
                    let next_start_ms = notes_to_ms(next.start, tempo_changes) + INIT_DELAY_MS;
                    let next_fret_cmd_ms = next_start_ms.saturating_sub(fret_prep_ms(next.finger_technique));
                    default_unfret_ms.min(next_fret_cmd_ms)
                } else {
                    default_unfret_ms
                };

                commands.push(TimedCommand {
                    time_ms: unfret_ms,
                    message: Message::Unfret(guitar_string, dampen_fret),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine_score::timing::Fraction;
    use lilyparse::syntax::ast::{NoteDuration, Tempo};

    fn quarter_tempo(bpm: u16) -> Tempo {
        Tempo {
            note_duration: NoteDuration {
                ratio: 4,
                augmentation: 0,
                tuplet: None,
            },
            bpm,
        }
    }

    #[test]
    fn open_string_is_no_fret() {
        assert_eq!(midi_pitch_to_fret(40, 40, 12), Fret::NoFret);
    }

    #[test]
    fn fret_offset_from_open_pitch() {
        assert_eq!(midi_pitch_to_fret(45, 40, 12), Fret::Fret5);
        assert_eq!(midi_pitch_to_fret(52, 40, 12), Fret::Fret12);
        assert_eq!(midi_pitch_to_fret(82, 64, 18), Fret::Fret18);
    }

    #[test]
    fn notes_to_ms_no_tempo_changes() {
        // Default: quarter = 90 BPM → one quarter note = 60_000/90 = 666 ms
        let ms = notes_to_ms(Fraction::new(1u32, 4u32), &[]);
        assert_eq!(ms, 666);
    }

    #[test]
    fn notes_to_ms_with_tempo_change() {
        let changes = vec![
            (Fraction::new(0u32, 1u32), quarter_tempo(60)),
            (Fraction::new(1u32, 2u32), quarter_tempo(120)),
        ];
        // 0.5 whole at 60 BPM (4000 ms/whole) + 0.25 whole at 120 BPM (2000 ms/whole)
        // = 2000 + 500 = 2500 ms
        let ms = notes_to_ms(Fraction::new(3u32, 4u32), &changes);
        assert_eq!(ms, 2500);
    }

    // --- Test helpers ---------------------------------------------------------

    fn make_score_parts(first_events: Vec<lilyparse::syntax::ast::Event>) -> lilyparse::syntax::ast::LilyScore {
        use lilyparse::syntax::ast::{Global, Header, LilyPart, LilyScore};
        fn empty(name: &str) -> LilyPart {
            LilyPart {
                name: name.to_owned(),
                events: vec![],
            }
        }
        LilyScore {
            header: Some(Header {
                title: Some("Test".to_owned()),
            }),
            global: Global::default(),
            parts: [
                LilyPart {
                    name: "stringOne".to_owned(),
                    events: first_events,
                },
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                empty("stringSix"),
            ],
        }
    }

    fn lily_note(
        pitch: lilyparse::syntax::ast::PitchClass,
        octave: i8,
        ratio: u16,
        articulation: lilyparse::syntax::ast::Articulation,
    ) -> lilyparse::syntax::ast::Event {
        lily_note_with_dynamic(pitch, octave, ratio, articulation, lilyparse::syntax::ast::Dynamic::MF)
    }

    fn lily_note_with_dynamic(
        pitch: lilyparse::syntax::ast::PitchClass,
        octave: i8,
        ratio: u16,
        articulation: lilyparse::syntax::ast::Articulation,
        dynamic: lilyparse::syntax::ast::Dynamic,
    ) -> lilyparse::syntax::ast::Event {
        use lilyparse::syntax::ast::*;
        Event::Note(Note {
            class: pitch,
            accidental: Accidental::None,
            octave,
            duration: Some(NoteDuration { ratio, augmentation: 0, tuplet: None }),
            dynamic: Some(dynamic),
            articulation,
            crescendo: None,
            tie: false,
        })
    }

    /// Builds a MachineScore directly from notes + tempo changes, bypassing LilyPond parsing.
    /// This lets tests set up precise ms-level timing without needing a full LilyScore.
    fn make_machine_score(
        notes: Vec<crate::machine_score::Note>,
        tempo_changes: Vec<(crate::machine_score::timing::Notes, lilyparse::syntax::ast::Tempo)>,
    ) -> crate::machine_score::MachineScore {
        use crate::machine_score::{MachineScore, MachineScorePart};
        fn empty(name: &str) -> MachineScorePart {
            MachineScorePart { name: name.to_owned(), notes: vec![] }
        }
        MachineScore {
            title: "Test".to_owned(),
            parts: [
                MachineScorePart { name: "stringOne".to_owned(), notes },
                empty("stringTwo"), empty("stringThree"),
                empty("stringFour"), empty("stringFive"), empty("stringSix"),
            ],
            tempo_changes,
        }
    }

    // --- Basic note layout ----------------------------------------------------

    #[test]
    fn fret_command_precedes_pluck_by_prep_delay() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        // A-1 = MIDI 45 → Fret5 on E string; Staccato → FretQuiet
        let score = make_score_parts(vec![lily_note(PitchClass::A, -1, 4, Articulation::Staccato)]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        let fret_cmd = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::FretQuiet(GuitarString::E, Fret::Fret5)))
            .expect("FretQuiet missing");
        let pluck_cmd = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Pluck(GuitarString::E)))
            .expect("Pluck missing");

        assert_eq!(pluck_cmd.time_ms - fret_cmd.time_ms, FRET_QUIET_PREP_MS);
    }

    #[test]
    fn first_note_starts_after_init_delay() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        let score = make_score_parts(vec![lily_note(PitchClass::A, -1, 4, Articulation::Staccato)]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        let pluck = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Pluck(_)))
            .expect("Pluck missing");
        assert_eq!(pluck.time_ms, INIT_DELAY_MS);
    }

    #[test]
    fn pluck_volume_issued_before_pluck_by_prep_delay() {
        use crate::machine_score::{FingerTechnique, MidiPitch, MidiVolume, Note, PluckTechnique};
        use crate::machine_score::timing::Fraction;
        // Single note: PluckVolume should be at start_ms - PLUCK_VOLUME_PREP_MS.
        let note = Note {
            pitch: MidiPitch::new(45), // Fret5 on E string
            volume: MidiVolume::new(80),
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(0u32, 1u32),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
        };
        let ms = CommandTimeline::from_machine_score(&make_machine_score(vec![note], vec![]));

        let vol_cmd = ms.commands.iter()
            .find(|c| matches!(c.message, Message::PluckVolume(GuitarString::E, _)))
            .expect("PluckVolume missing");
        let pluck_cmd = ms.commands.iter()
            .find(|c| matches!(c.message, Message::Pluck(GuitarString::E)))
            .expect("Pluck missing");

        assert_eq!(pluck_cmd.time_ms - vol_cmd.time_ms, PLUCK_VOLUME_PREP_MS);
    }

    #[test]
    fn pluck_volume_at_midpoint_when_plucks_are_close() {
        use crate::machine_score::{FingerTechnique, MidiPitch, MidiVolume, Note, PluckTechnique};
        use crate::machine_score::timing::Fraction;
        use lilyparse::syntax::ast::{NoteDuration, Tempo};
        // Tempo: quarter = 3000 BPM → 20 ms per quarter note.
        // Notes 1 and 2 are immediately consecutive → pluck gap = 20 ms < PLUCK_VOLUME_PREP_MS.
        // PluckVolume for note 2 must be at the midpoint between the two plucks.
        let fast_tempo = Tempo { note_duration: NoteDuration { ratio: 4, augmentation: 0, tuplet: None }, bpm: 3000 };
        let note1 = Note {
            pitch: MidiPitch::new(45),
            volume: MidiVolume::new(64),
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(0u32, 1u32),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
        };
        let note2 = Note {
            pitch: MidiPitch::new(45),
            volume: MidiVolume::new(127), // different volume → PluckVolume emitted
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(1u32, 4u32),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
        };
        let tempo_changes = vec![(Fraction::new(0u32, 1u32), fast_tempo)];
        let ms = CommandTimeline::from_machine_score(
            &make_machine_score(vec![note1, note2], tempo_changes),
        );

        // Pluck 1 at INIT_DELAY_MS, pluck 2 at INIT_DELAY_MS + 20ms.
        let pluck1_ms = INIT_DELAY_MS;
        let pluck2_ms = INIT_DELAY_MS + 20;
        let expected_vol2_ms = (pluck1_ms + pluck2_ms) / 2;

        let vol_cmds: Vec<_> = ms.commands.iter()
            .filter(|c| matches!(c.message, Message::PluckVolume(GuitarString::E, _)))
            .collect();
        // Two PluckVolume commands: one for note1 (at start - prep) and one for note2 (midpoint).
        assert_eq!(vol_cmds.len(), 2, "expected two PluckVolume commands");
        assert_eq!(vol_cmds[1].time_ms, expected_vol2_ms,
            "second PluckVolume should be at midpoint ({expected_vol2_ms}ms)");
    }

    #[test]
    fn dampen_uses_fret11_for_open_string() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        // E-1 = MIDI 40 → open E string (NoFret); octave -1 in this codebase maps to E2=40
        let score = make_score_parts(vec![lily_note(PitchClass::E, -1, 4, Articulation::Staccato)]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        assert!(
            ms.commands
                .iter()
                .any(|c| matches!(c.message, Message::Dampen(GuitarString::E, Fret::Fret11))),
            "expected Dampen with Fret11 for open string"
        );
        assert!(
            ms.commands
                .iter()
                .any(|c| matches!(c.message, Message::Unfret(GuitarString::E, Fret::Fret11))),
            "expected Unfret with Fret11 for open string"
        );
    }

    #[test]
    fn unfret_delayed_by_dampen_settle() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        let score = make_score_parts(vec![lily_note(PitchClass::A, -1, 4, Articulation::Staccato)]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        let dampen = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Dampen(GuitarString::E, Fret::Fret5)))
            .expect("Dampen missing");
        let unfret = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Unfret(GuitarString::E, Fret::Fret5)))
            .expect("Unfret missing");

        assert_eq!(unfret.time_ms - dampen.time_ms, DAMPEN_SETTLE_MS);
    }

    #[test]
    fn no_dampen_when_immediately_followed() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        // Two consecutive quarter notes on E string, no gap.
        let score = make_score_parts(vec![
            lily_note(PitchClass::A, -1, 4, Articulation::Staccato), // Fret5
            lily_note(PitchClass::B, -1, 4, Articulation::Staccato), // Fret7
        ]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        // No Dampen should appear for the first note (only a Dampen for the second)
        let dampens: Vec<_> = ms
            .commands
            .iter()
            .filter(|c| matches!(c.message, Message::Dampen(GuitarString::E, _)))
            .collect();
        assert_eq!(dampens.len(), 1, "expected exactly one Dampen (for the last note)");
    }

    #[test]
    fn unfret_brought_forward_when_next_fret_is_lower() {
        use lilyparse::syntax::ast::{
            Articulation, Event, Global, Header, LilyPart, LilyScore, NoteDuration, PitchClass, Rest,
        };
        fn empty(name: &str) -> LilyPart {
            LilyPart {
                name: name.to_owned(),
                events: vec![],
            }
        }
        // Note 1: B-1 = MIDI 47 → Fret7 on E string; rest (gap); Note 2: A-1 → Fret5.
        // Fret5 < Fret7 in ordinal, so Unfret(Fret7) must arrive before FretQuiet(Fret5).
        let score = LilyScore {
            header: Some(Header { title: None }),
            global: Global::default(),
            parts: [
                LilyPart {
                    name: "stringOne".to_owned(),
                    events: vec![
                        lily_note(PitchClass::B, -1, 4, Articulation::Staccato), // Fret7
                        Event::Rest(Rest {
                            // 1/8 rest (~333 ms at 90 BPM) — short enough that
                            // DAMPEN_SETTLE_MS would overshoot the next fret command.
                            duration: Some(NoteDuration {
                                ratio: 8,
                                augmentation: 0,
                                tuplet: None,
                            }),
                            dynamic: None,
                            articulation: Articulation::none(),
                            crescendo: None,
                            multiplier: None,
                        }),
                        lily_note(PitchClass::A, -1, 4, Articulation::Staccato), // Fret5
                    ],
                },
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                empty("stringSix"),
            ],
        };

        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        let unfret7 = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Unfret(GuitarString::E, Fret::Fret7)))
            .expect("Unfret Fret7 missing");
        let fret5_cmd = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::FretQuiet(GuitarString::E, Fret::Fret5)))
            .expect("FretQuiet Fret5 missing");

        // Unfret of the higher fret must happen no later than the lower fret's command.
        assert!(
            unfret7.time_ms <= fret5_cmd.time_ms,
            "Unfret Fret7 ({}) should be ≤ FretQuiet Fret5 ({})",
            unfret7.time_ms,
            fret5_cmd.time_ms
        );
        // And it must be earlier than the full settle delay would have produced.
        let dampen7 = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Dampen(GuitarString::E, Fret::Fret7)))
            .expect("Dampen Fret7 missing");
        assert!(
            unfret7.time_ms < dampen7.time_ms + DAMPEN_SETTLE_MS,
            "Unfret should be brought forward (got {}ms, settle would give {}ms)",
            unfret7.time_ms,
            dampen7.time_ms + DAMPEN_SETTLE_MS
        );
    }

    #[test]
    fn timeline_starts_with_reset_and_pluck_enables() {
        use lilyparse::syntax::ast::{Global, Header, LilyPart, LilyScore};
        fn empty(name: &str) -> LilyPart {
            LilyPart {
                name: name.to_owned(),
                events: vec![],
            }
        }
        let score = LilyScore {
            header: Some(Header {
                title: Some("Test".to_owned()),
            }),
            global: Global::default(),
            parts: [
                empty("stringOne"),
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                empty("stringSix"),
            ],
        };
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        assert!(matches!(ms.commands[0].message, Message::Reset));
        assert_eq!(ms.commands[0].time_ms, 0);
        let enables: Vec<_> = ms.commands[1..=6].iter().collect();
        assert!(enables.iter().all(|c| matches!(c.message, Message::PluckEnable(_))));
        assert!(matches!(ms.commands.last().unwrap().message, Message::Reset));
    }
}
