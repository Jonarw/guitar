use fraction::Zero;
use lilyparse::syntax::ast::Tempo;
use protocol::{Fret, GuitarString, Message};

use crate::machine_score::timing::Notes;
use crate::machine_score::{FingerTechnique, MachineScore, Note, PluckTechnique};
use crate::playback::string_volume::StringVolumeTable;

/// Guitar string configuration: (string enum, open-string MIDI pitch, max controllable frets).
///
/// Standard tuning. The high-e string has 18 controlled frets; all others have 12.
const STRING_CONFIGS: [(GuitarString, u8, u8); 6] = [
    (GuitarString::e, 64, 18),
    (GuitarString::B, 59, 12),
    (GuitarString::G, 55, 12),
    (GuitarString::D, 50, 12),
    (GuitarString::A, 45, 12),
    (GuitarString::E, 40, 12),
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
const PLUCK_VOLUME_PREP_MS: u64 = 200;
/// How long to wait after `Dampen` before issuing `Unfret`
/// (gives the string time to fully stop vibrating).
const DAMPEN_SETTLE_MS: u64 = 500;
/// Delay between the end-of-prologue and the first musical note.
/// Gives the hardware time to initialise after `Reset` + `PluckEnable`.
const INIT_DELAY_MS: u64 = 1000;
const SCORE_END_DELAY_MS: u64 = 500;
const UNFRET_QUIET_PREP_MS: u64 = 200;
const UNFRET_QUIET_DURATION_MS: u64 = 550;
const UNFRET_FAST_DURATION_MS: u64 = 10;
const FRET_TO_DAMPEN_PREP_MS: u64 = 10;

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
        return INIT_DELAY_MS;
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
    total_ms + INIT_DELAY_MS
}

pub fn start_and_end_time(note: &Note, tempo_changes: &[(Notes, Tempo)]) -> (u64, u64) {
    (
        notes_to_ms(note.start, tempo_changes),
        (notes_to_ms(note.start + note.length, tempo_changes)),
    )
}

// ---------------------------------------------------------------------------
// Timeline builder
// ---------------------------------------------------------------------------

impl CommandTimeline {
    /// Converts a [`MachineScore`] into a fully-timed command sequence ready for playback.
    ///
    /// Per-note command ordering:
    /// 1. `FretQuiet` / `FretFast` at `start_ms − fret_prep` (skipped for open strings)
    /// 2. `PluckVolume` shortly before `Pluck` (only when volume changes)
    /// 3. `Pluck` at `start_ms`
    /// 4. End-of-note handling depends on what follows on this string:
    ///    - Next note starts immediately on the same or a higher fret: nothing to do
    ///      (every string/fret has its own finger, so the finger can stay pressed)
    ///    - Next note is on a lower fret: `Dampen` and/or `Unfret`/`UnfretFast`, timed so
    ///      the string is free before that note's fret command
    ///    - Gap before the next note (or last note): `Dampen` at `end_ms` (fret 11 for open
    ///      strings), `Unfret` once the string has settled (`DAMPEN_SETTLE_MS`), brought
    ///      forward when needed to clear a lower fret in time
    pub fn from_machine_score(score: &MachineScore) -> Self {
        Self::from_machine_score_with_volume_table(score, &StringVolumeTable::default())
    }

    /// Converts a [`MachineScore`] into a command timeline with per-string
    /// runtime-calibrated pluck-volume ranges.
    pub fn from_machine_score_with_volume_table(score: &MachineScore, volume_table: &StringVolumeTable) -> Self {
        let mut commands: Vec<TimedCommand> = Vec::new();

        // --- Prologue -----------------------------------------------------------
        commands.push(TimedCommand {
            time_ms: 0,
            message: Message::Reset,
        });
        for (i, (string, _, _)) in STRING_CONFIGS.iter().enumerate() {
            commands.push(TimedCommand {
                time_ms: 0,
                message: Message::PluckVolume(
                    *string,
                    volume_table.range_for(i, Fret::NoFret).min.saturating_sub(10).into(),
                ),
            });

            commands.push(TimedCommand {
                time_ms: 300,
                message: Message::PluckEnable(*string),
            });
        }

        // --- Musical notes ------------------------------------------------------
        let tempo_changes = &score.tempo_changes;

        for (part_idx, part) in score.parts.iter().enumerate() {
            Self::build_string_commands(&mut commands, part_idx, volume_table, &part.notes, tempo_changes);
        }

        let score_end_ms = commands.iter().map(|c| c.time_ms).max().unwrap_or_default();

        // --- Epilogue -----------------------------------------------------------

        for (i, (string, _, _)) in STRING_CONFIGS.iter().enumerate() {
            commands.push(TimedCommand {
                time_ms: score_end_ms + SCORE_END_DELAY_MS,
                message: Message::PluckVolume(
                    *string,
                    volume_table.range_for(i, Fret::NoFret).min.saturating_sub(10).into(),
                ),
            });
        }

        commands.push(TimedCommand {
            time_ms: score_end_ms + SCORE_END_DELAY_MS * 2,
            message: Message::Reset,
        });

        // Stable sort preserves within-timestamp insertion order.
        commands.sort_by_key(|c| c.time_ms);

        Self { commands }
    }

    fn build_string_commands(
        commands: &mut Vec<TimedCommand>,
        string_index: usize,
        volume_table: &StringVolumeTable,
        notes: &[Note],
        tempo_changes: &[(Notes, Tempo)],
    ) {
        let (guitar_string, open_pitch, max_frets) = STRING_CONFIGS[string_index];

        let mut last_pluck_volume: Option<u8> = None;
        let mut prev_pluck_ms: Option<u64> = None;

        for (i, note) in notes.iter().enumerate() {
            let (start_ms, end_ms) = start_and_end_time(note, tempo_changes);

            let fret = midi_pitch_to_fret(note.pitch.pitch, open_pitch, max_frets);
            let prep = fret_prep_ms(note.finger_technique);

            // --- Fret command (before pluck) ------------------------------------
            if fret != Fret::NoFret {
                let fret_msg = match note.finger_technique {
                    FingerTechnique::Quiet => Message::FretQuiet(guitar_string, fret),
                    FingerTechnique::Loud => Message::FretFast(guitar_string, fret),
                };
                commands.push(TimedCommand {
                    time_ms: start_ms - prep,
                    message: fret_msg,
                });
            }

            if note.pluck_technique != PluckTechnique::None {
                // --- Volume (only when it changes) ----------------------------------
                let pluck_vol = volume_table.range_for(string_index, fret).map_midi_volume(note.volume);
                if last_pluck_volume != Some(pluck_vol) {
                    // Issue PluckVolume PLUCK_VOLUME_PREP_MS before the pluck so the hardware
                    // has time to apply it. If the previous pluck was closer than that, place
                    // it at the midpoint between the two plucks.
                    let volume_ms = match prev_pluck_ms {
                        Some(prev) => {
                            let ideal = start_ms.saturating_sub(PLUCK_VOLUME_PREP_MS);
                            if ideal > prev + PLUCK_VOLUME_PREP_MS / 2 {
                                ideal
                            } else {
                                (prev + start_ms) / 2
                            }
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
            }

            // --- End-of-note: dampen / unfret -----------------------------------
            if fret == Fret::NoFret {
                const DAMPEN_FRET: Fret = Fret::Fret11;
                // current note is an open string
                if let Some(next_note) = notes.get(i + 1) {
                    // not the last note on the string -> depending on when the next note is, dampen the open string
                    let next_start_ms = notes_to_ms(next_note.start, tempo_changes);
                    let delta = next_start_ms - end_ms;

                    const DAMPEN_THRESHOLD: u64 = FRET_TO_DAMPEN_PREP_MS + UNFRET_QUIET_PREP_MS;
                    match delta {
                        0..DAMPEN_THRESHOLD => {} // very little time -> do nothing
                        _ => {
                            // dampen the open string until we play another note or DAMPEN_SETTLE_MS
                            commands.push(TimedCommand {
                                time_ms: end_ms,
                                message: Message::Dampen(guitar_string, DAMPEN_FRET),
                            });

                            let dampen_end = (end_ms + DAMPEN_SETTLE_MS)
                                .min(next_start_ms - UNFRET_QUIET_DURATION_MS - FRET_QUIET_PREP_MS - 1);
                            commands.push(TimedCommand {
                                time_ms: dampen_end,
                                message: Message::Dampen(guitar_string, DAMPEN_FRET),
                            });
                        }
                    }
                } else {
                    // last note on this string -> dampen and unfret
                    commands.push(TimedCommand {
                        time_ms: end_ms,
                        message: Message::Dampen(guitar_string, DAMPEN_FRET),
                    });

                    commands.push(TimedCommand {
                        time_ms: end_ms + DAMPEN_SETTLE_MS,
                        message: Message::Unfret(guitar_string, DAMPEN_FRET),
                    });
                }
            } else {
                // current note is not an open string
                let next_note_same_or_lower_fret = notes.iter().skip(i + 1).find(|n| n.pitch.pitch <= note.pitch.pitch);
                if let Some(next_note_same_or_lower_fret) = next_note_same_or_lower_fret {
                    // There are still notes left with same or lower fret. We need to take these into account
                    // when planning our dampen / unfret sequence.
                    let next_start_ms = notes_to_ms(next_note_same_or_lower_fret.start, tempo_changes);
                    let delta = next_start_ms - end_ms;
                    if next_note_same_or_lower_fret.pitch == note.pitch {
                        // next note that is relevant for us is on the same fret
                        // -> depending on when that is we potentially dampen and unfret
                        const DAMPEN_THRESHOLD: u64 = DAMPEN_SETTLE_MS + UNFRET_QUIET_DURATION_MS + FRET_QUIET_PREP_MS;
                        match delta {
                            0..FRET_TO_DAMPEN_PREP_MS => {} // no or very little time until next note, we just stay fretted
                            FRET_TO_DAMPEN_PREP_MS..DAMPEN_THRESHOLD => {
                                // we have some time to dampen, but not enough time to unfret
                                commands.push(TimedCommand {
                                    time_ms: end_ms,
                                    message: Message::Dampen(guitar_string, fret),
                                });
                            }
                            _ => {
                                // we have so much time that we can dampen and completely unfret until we need to do something again
                                commands.push(TimedCommand {
                                    time_ms: end_ms,
                                    message: Message::Dampen(guitar_string, fret),
                                });

                                commands.push(TimedCommand {
                                    time_ms: end_ms + DAMPEN_SETTLE_MS,
                                    message: Message::Unfret(guitar_string, fret),
                                });
                            }
                        }
                    } else {
                        // next note that is relevant for us is on a lower fret
                        // -> we need to be clear of the string by the time this is played
                        match delta {
                            0..UNFRET_FAST_DURATION_MS => {
                                // no or very little time until next note, unfret as fast as we can
                                commands.push(TimedCommand {
                                    time_ms: end_ms - UNFRET_FAST_DURATION_MS,
                                    message: Message::UnfretFast(guitar_string, fret),
                                });
                            }
                            UNFRET_FAST_DURATION_MS..UNFRET_QUIET_PREP_MS => {
                                // we have a little bit of time, but not enough to unfret quietly, so dampen for a bit and then unfret fast
                                commands.push(TimedCommand {
                                    time_ms: end_ms,
                                    message: Message::Dampen(guitar_string, fret),
                                });

                                commands.push(TimedCommand {
                                    time_ms: end_ms + delta - UNFRET_FAST_DURATION_MS,
                                    message: Message::UnfretFast(guitar_string, fret),
                                });
                            }
                            _ => {
                                // we have enough time to unfret quietly, dampen until then or until DAMPEN_SETTLE_MS
                                commands.push(TimedCommand {
                                    time_ms: end_ms,
                                    message: Message::Dampen(guitar_string, fret),
                                });

                                let dampen_end_time =
                                    (end_ms + DAMPEN_SETTLE_MS).min(next_start_ms - UNFRET_QUIET_PREP_MS);

                                commands.push(TimedCommand {
                                    time_ms: dampen_end_time,
                                    message: Message::Unfret(guitar_string, fret),
                                });
                            }
                        }
                    }
                } else {
                    // no more notes left with a same or lower fret -> dampen and unfret
                    commands.push(TimedCommand {
                        time_ms: end_ms,
                        message: Message::Dampen(guitar_string, fret),
                    });

                    commands.push(TimedCommand {
                        time_ms: end_ms + DAMPEN_SETTLE_MS,
                        message: Message::Unfret(guitar_string, fret),
                    });
                }
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
    use crate::{machine_score::timing::Fraction, playback::string_volume::StringVolumeRange};
    use lilyparse::syntax::ast::{self, *};

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
    fn notes_to_ms_position_zero_is_init_delay() {
        assert_eq!(notes_to_ms(Fraction::new(0u32, 1u32), &[]), INIT_DELAY_MS);
    }

    #[test]
    fn notes_to_ms_no_tempo_changes() {
        // Default: quarter = 90 BPM → one quarter note = 60_000/90 = 666 ms,
        // plus the INIT_DELAY_MS offset that all wall-clock times carry.
        let ms = notes_to_ms(Fraction::new(1u32, 4u32), &[]);
        assert_eq!(ms, INIT_DELAY_MS + 666);
    }

    #[test]
    fn notes_to_ms_with_tempo_change() {
        let changes = vec![
            (Fraction::new(0u32, 1u32), quarter_tempo(60)),
            (Fraction::new(1u32, 2u32), quarter_tempo(120)),
        ];
        // 0.5 whole at 60 BPM (4000 ms/whole) + 0.25 whole at 120 BPM (2000 ms/whole)
        // = 2000 + 500 = 2500 ms, plus the INIT_DELAY_MS offset.
        let ms = notes_to_ms(Fraction::new(3u32, 4u32), &changes);
        assert_eq!(ms, INIT_DELAY_MS + 2500);
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
                empty("stringOne"),
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                LilyPart {
                    name: "stringSix".to_owned(),
                    events: first_events,
                },
            ],
        }
    }

    fn lily_note(pitch: PitchClass, octave: i8, ratio: u16, articulation: Articulation) -> Event {
        lily_note_with_dynamic(pitch, octave, ratio, articulation, Dynamic::MF)
    }

    fn lily_note_with_dynamic(
        pitch: PitchClass,
        octave: i8,
        ratio: u16,
        articulation: Articulation,
        dynamic: Dynamic,
    ) -> Event {
        let mut ret = ast::Note::default();
        ret.class = pitch;
        ret.octave = octave;
        ret.duration = Some(NoteDuration {
            ratio,
            augmentation: 0,
            tuplet: None,
        });
        ret.articulation = articulation;
        ret.dynamic = Some(dynamic);
        Event::Note(ret)
    }

    /// Builds a MachineScore directly from notes + tempo changes, bypassing LilyPond parsing.
    /// This lets tests set up precise ms-level timing without needing a full LilyScore.
    fn make_machine_score(
        notes: Vec<crate::machine_score::Note>,
        tempo_changes: Vec<(crate::machine_score::timing::Notes, lilyparse::syntax::ast::Tempo)>,
    ) -> crate::machine_score::MachineScore {
        use crate::machine_score::{MachineScore, MachineScorePart};
        fn empty(name: &str) -> MachineScorePart {
            MachineScorePart {
                name: name.to_owned(),
                notes: vec![],
            }
        }
        MachineScore {
            title: "Test".to_owned(),
            parts: [
                empty("stringOne"),
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                MachineScorePart {
                    name: "stringSix".to_owned(),
                    notes,
                },
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
        use crate::machine_score::timing::Fraction;
        use crate::machine_score::{FingerTechnique, MidiPitch, MidiVolume, Note, PluckTechnique};
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

        let pluck_cmd = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Pluck(GuitarString::E)))
            .expect("Pluck missing");
        // The timeline also contains prologue/epilogue PluckVolume commands; take the one
        // directly associated with this pluck, i.e. the last one issued before it.
        let vol_cmd = ms
            .commands
            .iter()
            .filter(|c| matches!(c.message, Message::PluckVolume(GuitarString::E, _)))
            .filter(|c| c.time_ms <= pluck_cmd.time_ms)
            .max_by_key(|c| c.time_ms)
            .expect("PluckVolume missing");

        assert_eq!(pluck_cmd.time_ms - vol_cmd.time_ms, PLUCK_VOLUME_PREP_MS);
    }

    #[test]
    fn pluck_volume_at_midpoint_when_plucks_are_close() {
        use crate::machine_score::timing::Fraction;
        use crate::machine_score::{FingerTechnique, MidiPitch, MidiVolume, Note, PluckTechnique};
        use lilyparse::syntax::ast::{NoteDuration, Tempo};
        // Tempo: quarter = 3000 BPM → 20 ms per quarter note.
        // Notes 1 and 2 are immediately consecutive → pluck gap = 20 ms < PLUCK_VOLUME_PREP_MS.
        // PluckVolume for note 2 must be at the midpoint between the two plucks.
        let fast_tempo = Tempo {
            note_duration: NoteDuration {
                ratio: 4,
                augmentation: 0,
                tuplet: None,
            },
            bpm: 3000,
        };
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
        let ms = CommandTimeline::from_machine_score(&make_machine_score(vec![note1, note2], tempo_changes));

        // Pluck 1 at INIT_DELAY_MS, pluck 2 at INIT_DELAY_MS + 20ms.
        let pluck1_ms = INIT_DELAY_MS;
        let pluck2_ms = INIT_DELAY_MS + 20;
        let expected_vol2_ms = (pluck1_ms + pluck2_ms) / 2;

        let vol_cmds: Vec<_> = ms
            .commands
            .iter()
            // Skip prologue (time 0) and epilogue (after the last pluck) PluckVolumes.
            .filter(|c| c.time_ms > 0 && c.time_ms <= pluck2_ms)
            .filter(|c| matches!(c.message, Message::PluckVolume(GuitarString::E, _)))
            .collect();
        // Two PluckVolume commands: one for note1 (at start - prep) and one for note2 (midpoint).
        assert_eq!(vol_cmds.len(), 2, "expected two PluckVolume commands");
        assert_eq!(
            vol_cmds[1].time_ms, expected_vol2_ms,
            "second PluckVolume should be at midpoint ({expected_vol2_ms}ms)"
        );
    }

    #[test]
    fn pluck_volume_uses_calibrated_string_range() {
        use crate::machine_score::timing::Fraction;
        use crate::machine_score::{FingerTechnique, MidiPitch, MidiVolume, Note, PluckTechnique};

        let volume_table = StringVolumeTable::uniform(StringVolumeRange { min: 149, max: 220 });

        let note1 = Note {
            pitch: MidiPitch::new(45),
            volume: MidiVolume::new(0),
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(0u32, 1u32),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
        };
        let note2 = Note {
            pitch: MidiPitch::new(45),
            volume: MidiVolume::new(127),
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(1u32, 4u32),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
        };

        let timeline = CommandTimeline::from_machine_score_with_volume_table(
            &make_machine_score(vec![note1, note2], vec![]),
            &volume_table,
        );

        let volumes: Vec<u8> = timeline
            .commands
            .iter()
            .filter_map(|c| match c.message {
                Message::PluckVolume(GuitarString::E, v) => Some(v.volume()),
                _ => None,
            })
            // Prologue and epilogue send min - 10; keep only the note volumes.
            .filter(|v| *v >= volume_table.ranges[5][0].min)
            .collect();

        assert_eq!(volumes, vec![149, 220]);
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
                empty("stringOne"),
                empty("stringTwo"),
                empty("stringThree"),
                empty("stringFour"),
                empty("stringFive"),
                LilyPart {
                    name: "stringSix".to_owned(),
                    events: vec![
                        lily_note(PitchClass::B, -1, 4, Articulation::Staccato), // Fret7
                        Event::Rest(Rest {
                            duration: Some(NoteDuration {
                                ratio: 8,
                                augmentation: 0,
                                tuplet: None,
                            }),
                            dynamic: None,
                            articulation: Articulation::none(),
                            crescendo: None,
                            slur: None,
                            multipliers: Vec::new(),
                            dividers: Vec::new(),
                        }),
                        lily_note(PitchClass::A, -1, 4, Articulation::Staccato), // Fret5
                    ],
                },
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
    fn unfret_fast_when_lower_fret_follows_immediately() {
        use lilyparse::syntax::ast::{Articulation, PitchClass};
        // Two consecutive quarter notes on E string, no gap: B-1 (Fret7) then A-1 (Fret5).
        // The next note is on a lower fret, so the finger on Fret7 must be released
        // as fast as possible; there is no time to dampen first.
        let score = make_score_parts(vec![
            lily_note(PitchClass::B, -1, 4, Articulation::Staccato), // Fret7
            lily_note(PitchClass::A, -1, 4, Articulation::Staccato), // Fret5
        ]);
        let ms = CommandTimeline::from_machine_score(&crate::machine_score::MachineScore::from_lilyscore(score));

        let unfret_fast = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::UnfretFast(GuitarString::E, Fret::Fret7)))
            .expect("UnfretFast Fret7 missing");
        let pluck = ms
            .commands
            .iter()
            .find(|c| matches!(c.message, Message::Pluck(GuitarString::E)))
            .expect("Pluck missing");
        // Note 1 is a quarter at the default 90 BPM → ends 666 ms after the first pluck.
        assert_eq!(unfret_fast.time_ms, pluck.time_ms + 666);
        assert!(
            !ms.commands
                .iter()
                .any(|c| matches!(c.message, Message::Dampen(GuitarString::E, Fret::Fret7))),
            "no time to dampen before an immediately-following lower fret"
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

        // Prologue: Reset, then one baseline PluckVolume per string, then one
        // PluckEnable per string. Epilogue: final Reset.
        assert!(matches!(ms.commands[0].message, Message::Reset));
        assert_eq!(ms.commands[0].time_ms, 0);
        let volumes: Vec<_> = ms.commands[1..=6].iter().collect();
        assert!(volumes.iter().all(|c| matches!(c.message, Message::PluckVolume(_, _))));
        let enables: Vec<_> = ms.commands[7..=12].iter().collect();
        assert!(enables.iter().all(|c| matches!(c.message, Message::PluckEnable(_))));
        assert!(enables.iter().all(|c| c.time_ms == 300));
        assert!(matches!(ms.commands.last().unwrap().message, Message::Reset));
    }
}
