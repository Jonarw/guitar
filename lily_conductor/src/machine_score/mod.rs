use std::{mem, panic};

use fraction::Zero;
use lilyparse::syntax::ast::{self, Articulation, LilyPart, LilyScore, NoteOrRest, Rest, Slur, Tempo, TimeSignature};

use crate::machine_score::{
    dynamic::{DynamicBuilder, DynamicHelper},
    event_timer::{EventTimer, Notes, TempoChanges, TimeSignatureChanges},
    note_timer::{NoteTimer, NoteTimingInfo},
};

pub mod dynamic;
pub mod event_timer;
pub mod note_timer;

/// MIDI pitch value in range `0..=127`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiPitch {
    pub pitch: u8,
}

impl MidiPitch {
    /// Creates a validated MIDI pitch value.
    pub fn new(pitch: u8) -> Self {
        if pitch > 127 {
            panic!("MIDI pitch outside of allowed range");
        }

        Self { pitch }
    }
}

/// MIDI note velocity in range `0..=127`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiVolume {
    pub volume: u8,
}

impl MidiVolume {
    pub(crate) const MAX_VALUE: u8 = 127;

    /// Creates a validated MIDI volume value.
    pub fn new(volume: u8) -> Self {
        if volume > Self::MAX_VALUE {
            panic!("MIDI volume outside of allowed range");
        }

        Self { volume }
    }
}

/// One machine-playable note instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub pitch: MidiPitch,
    pub volume: MidiVolume,
    pub length: Notes,
    pub start: Notes,
    pub pluck_technique: PluckTechnique,
    pub finger_technique: FingerTechnique,
    /// 0-based index of the bar in which the note starts.
    pub bar_number: u32,
}

/// Plucking actuator strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluckTechnique {
    Soft,
    Hard,
    None,
}

/// Fretting pressure strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerTechnique {
    Quiet,
    Loud,
}

/// Converted score containing per-string machine note instructions.
#[derive(Debug, PartialEq, Eq)]
pub struct MachineScore {
    pub title: String,
    pub parts: [MachineScorePart; 6],
    pub tempo_changes: TempoChanges,
    pub time_signature_changes: TimeSignatureChanges,
    /// Total number of bars in the score (maximum across parts).
    pub bar_count: u32,
}

/// Converted events for one part/string.
#[derive(Debug, PartialEq, Eq)]
pub struct MachineScorePart {
    pub notes: Vec<Note>,
}

/// Stateful converter for one LilyPond part.
struct LilyPartConverter {
    timed_notes: Vec<NoteTimingInfo>,
    bar_count: u32,
    dynamic_helper: DynamicHelper,
    notes: Vec<Note>,
    articulation: Articulation,
    next_note_tied: bool,
    slur_in_progress: bool,
}

impl LilyPartConverter {
    /// Creates a converter with fresh timing/dynamic state.
    pub fn new(lily_part: &LilyPart, time_signature_changes: &TimeSignatureChanges) -> Self {
        let dynamic_helper = DynamicBuilder::build(lily_part);
        let (timed_notes, bar_count) = NoteTimer::get_notes(lily_part, time_signature_changes);

        Self {
            timed_notes,
            bar_count,
            dynamic_helper,
            notes: Vec::new(),
            articulation: Articulation::Staccato,
            next_note_tied: false,
            slur_in_progress: false,
        }
    }

    /// Maps articulation marks to hardware techniques.
    fn current_technique(&self) -> (PluckTechnique, FingerTechnique) {
        if self.articulation.contains(Articulation::Portato) {
            (PluckTechnique::Soft, FingerTechnique::Loud)
        } else if self.articulation.contains(Articulation::Staccato) {
            (PluckTechnique::Hard, FingerTechnique::Quiet)
        } else if self.articulation.contains(Articulation::Tenuto) {
            (PluckTechnique::Soft, FingerTechnique::Quiet)
        } else if self.articulation.contains(Articulation::Staccatissimo) {
            (PluckTechnique::Hard, FingerTechnique::Loud)
        } else if self.articulation.contains(Articulation::Marcato) {
            (PluckTechnique::None, FingerTechnique::Loud)
        } else {
            panic!("Articulation should never be none")
        }
    }

    /// Converts a LilyPond note to a checked MIDI pitch.
    fn note_to_midi_pitch(note: &ast::Note) -> MidiPitch {
        const MIDI_C: i8 = 48;
        const SEMITONES_PER_OCTAVE: i8 = 12;

        let pitch = i16::from(
            MIDI_C
                + match note.class {
                    ast::PitchClass::C => 0,
                    ast::PitchClass::D => 2,
                    ast::PitchClass::E => 4,
                    ast::PitchClass::F => 5,
                    ast::PitchClass::G => 7,
                    ast::PitchClass::A => 9,
                    ast::PitchClass::B => 11,
                }
                + match note.accidental {
                    ast::Accidental::DoubleFlat => -2,
                    ast::Accidental::Flat => -1,
                    ast::Accidental::None => 0,
                    ast::Accidental::Sharp => 1,
                    ast::Accidental::DoubleSharp => 2,
                }
                + note.octave * SEMITONES_PER_OCTAVE,
        );

        if !(0..=127).contains(&pitch) {
            panic!("Converted pitch outside of MIDI range");
        }

        MidiPitch::new(pitch as u8)
    }

    /// Applies rest timing progression.
    fn process_rest(&mut self, rest: &Rest) {
        self.dynamic_helper.next_rest(rest);
    }

    /// Converts one LilyPond note event.
    fn process_note(&mut self, note: &ast::Note, timing_info: &NoteTimingInfo) {
        if !note.articulation.is_none() {
            self.articulation = note.articulation;
        }

        let volume = self.dynamic_helper.next_note(note, &timing_info);
        let (mut pluck_technique, finger_technique) = self.current_technique();

        if self.slur_in_progress {
            pluck_technique = PluckTechnique::None;
        }

        if let Some(slur) = note.slur {
            match slur {
                Slur::Start => self.slur_in_progress = true,
                Slur::End => self.slur_in_progress = false,
            }
        }

        if self.next_note_tied {
            let last_note = self
                .notes
                .last_mut()
                .expect("When the last note was tied, there must be at least one note in self.notes");
            last_note.length += timing_info.length;
        } else {
            self.notes.push(Note {
                pitch: Self::note_to_midi_pitch(note),
                volume,
                length: timing_info.length,
                start: timing_info.note_stamp,
                pluck_technique,
                finger_technique,
                bar_number: timing_info.bar_number,
            });
        }

        self.next_note_tied = note.tie;
    }

    /// Converts all note-like events in the part.
    pub fn convert(mut self) -> MachineScorePart {
        let timed_notes = mem::take(&mut self.timed_notes);
        for timing_info in timed_notes {
            match &timing_info.note_or_rest {
                NoteOrRest::Note(note) => {
                    self.process_note(&note, &timing_info);
                }
                NoteOrRest::Rest(rest) => {
                    self.process_rest(&rest);
                }
            }
        }

        MachineScorePart { notes: self.notes }
    }
}

impl MachineScore {
    /// Converts a parsed LilyPond score into machine-playable parts.
    pub fn from_lilyscore(score: LilyScore) -> Self {
        let score_title = score
            .header
            .as_ref()
            .and_then(|header| header.title.clone())
            .unwrap_or_default();

        let (time_signature_changes, tempo_changes) = EventTimer::extract_time_signature_and_tempo_changes(&score);
        let mut bar_count = 0;
        let parts = score.parts.map(|p| {
            let converter = LilyPartConverter::new(&p, &time_signature_changes);
            bar_count = bar_count.max(converter.bar_count);
            converter.convert()
        });

        Self {
            title: score_title,
            parts,
            tempo_changes,
            time_signature_changes,
            bar_count,
        }
    }

    /// Returns a new score containing only the notes that start within the given
    /// 1-indexed, inclusive bar range. Notes starting before `start_bar` are dropped;
    /// notes starting within the range but extending past `end_bar` are kept in full.
    ///
    /// Note start positions, tempo changes and time signature changes are rebased so
    /// that the first selected bar begins at position zero, i.e. playback of the
    /// extracted score starts immediately (preserving any rests at its beginning).
    pub fn extract_bar_range(&self, start_bar: u32, end_bar: Option<u32>) -> Result<Self, String> {
        if start_bar == 0 {
            return Err("start_bar must be at least 1".to_owned());
        }
        if let Some(end) = end_bar
            && end < start_bar
        {
            return Err(format!(
                "end_bar ({end}) must not be smaller than start_bar ({start_bar})"
            ));
        }
        let start_idx = start_bar - 1;
        if start_idx >= self.bar_count {
            return Err(format!(
                "start_bar {start_bar} is beyond the last bar of the score ({})",
                self.bar_count
            ));
        }
        let end_idx = end_bar.map(|e| (e - 1).min(self.bar_count.saturating_sub(1)));
        let in_range = |bar: u32| bar >= start_idx && end_idx.is_none_or(|end| bar <= end);

        let offset = note_timer::bar_start_position(start_idx, &self.time_signature_changes);

        let parts = self.parts.each_ref().map(|part| MachineScorePart {
            notes: part
                .notes
                .iter()
                .filter(|note| in_range(note.bar_number))
                .map(|note| Note {
                    start: if note.start >= offset {
                        note.start - offset
                    } else {
                        Notes::zero()
                    },
                    ..note.clone()
                })
                .collect(),
        });

        // Rebase tempo and time signature changes: drop changes before the selection,
        // pin the values effective at the selection start to position zero.
        let tempo_changes = rebase_timed_changes(&self.tempo_changes, offset, Tempo::default);
        let time_signature_changes = rebase_timed_changes(&self.time_signature_changes, offset, TimeSignature::default);

        let bar_count = match end_idx {
            Some(end) => end + 1 - start_idx,
            None => self.bar_count - start_idx,
        };

        Ok(Self {
            title: self.title.clone(),
            parts,
            tempo_changes,
            time_signature_changes,
            bar_count,
        })
    }
}

/// Rebases a sorted list of position-tagged changes to a new origin: changes at or
/// before `offset` collapse into a single entry at position zero holding the value
/// effective at `offset`; later changes are shifted. Empty lists stay empty so that
/// downstream default handling is preserved.
fn rebase_timed_changes<T: Copy>(changes: &[(Notes, T)], offset: Notes, default: impl Fn() -> T) -> Vec<(Notes, T)> {
    if changes.is_empty() {
        return Vec::new();
    }

    let mut effective = default();
    for (position, value) in changes {
        if *position > offset {
            break;
        }
        effective = *value;
    }

    let mut rebased = vec![(Notes::zero(), effective)];
    rebased.extend(
        changes
            .iter()
            .filter(|(position, _)| *position > offset)
            .map(|(position, value)| (*position - offset, *value)),
    );
    rebased
}

#[cfg(test)]
mod tests {
    use crate::machine_score::event_timer::Fraction;

    use super::*;
    use lilyparse::syntax::ast::{
        Accidental, Crescendo, Dynamic, Event, Header, Note as LilyNote, NoteDuration, PitchClass,
    };

    fn lily_note(
        class: PitchClass,
        octave: i8,
        duration: Option<NoteDuration>,
        dynamic: Option<Dynamic>,
        articulation: Articulation,
        crescendo: Option<Crescendo>,
    ) -> LilyNote {
        let mut ret = LilyNote::default();
        ret.class = class;
        ret.octave = octave;
        ret.duration = duration;
        ret.dynamic = dynamic;
        ret.articulation = articulation;
        ret.crescendo = crescendo;
        ret
    }

    fn lily_rest(duration: Option<NoteDuration>, dynamic: Option<Dynamic>) -> Rest {
        let mut ret = Rest::default();
        ret.duration = duration;
        ret.dynamic = dynamic;
        ret
    }

    fn empty_part(name: &str) -> LilyPart {
        LilyPart {
            name: name.to_owned(),
            events: Vec::new(),
        }
    }

    fn score_with_first_part(events: Vec<Event>) -> LilyScore {
        LilyScore {
            header: Some(Header {
                title: Some("Study".to_owned()),
            }),
            global: ast::Global::default(),
            parts: [
                LilyPart {
                    name: "stringOne".to_owned(),
                    events,
                },
                empty_part("stringTwo"),
                empty_part("stringThree"),
                empty_part("stringFour"),
                empty_part("stringFive"),
                empty_part("stringSix"),
            ],
        }
    }

    #[test]
    fn converts_lily_notes_to_midi_pitch() {
        assert_eq!(
            LilyPartConverter::note_to_midi_pitch(
                &lily_note(PitchClass::C, 0, None, None, Articulation::none(), None,)
            ),
            MidiPitch { pitch: 48 }
        );
        assert_eq!(
            LilyPartConverter::note_to_midi_pitch(&LilyNote {
                accidental: Accidental::Sharp,
                ..lily_note(PitchClass::F, 1, None, None, Articulation::none(), None)
            }),
            MidiPitch { pitch: 66 }
        );
    }

    #[test]
    fn machine_score_conversion_preserves_timing_and_expression() {
        let score = score_with_first_part(vec![
            Event::TimeSignature(ast::TimeSignature {
                numerator: 3,
                denominator: 4,
            }),
            Event::Note(lily_note(
                PitchClass::C,
                0,
                Some(NoteDuration {
                    ratio: 4,
                    augmentation: 0,
                }),
                Some(Dynamic::MF),
                Articulation::Tenuto,
                None,
            )),
            Event::Rest(lily_rest(
                Some(NoteDuration {
                    ratio: 4,
                    augmentation: 0,
                }),
                Some(Dynamic::P),
            )),
            Event::Note(lily_note(PitchClass::D, 0, None, None, Articulation::Staccato, None)),
        ]);

        let machine_score = MachineScore::from_lilyscore(score);

        assert_eq!(machine_score.title, "Study");
        assert_eq!(machine_score.parts[0].notes.len(), 2);
        assert_eq!(machine_score.parts[0].notes[0].start, Fraction::new(0u32, 1u32));
        assert_eq!(machine_score.parts[0].notes[0].length, Fraction::new(1u32, 4u32));
        assert_eq!(machine_score.parts[0].notes[0].pitch, MidiPitch { pitch: 48 });
        // MF base is 80, boosted by +10 for the primary beat stress on bar 1 beat 1.
        assert_eq!(machine_score.parts[0].notes[0].volume, MidiVolume { volume: 90 });
        assert_eq!(machine_score.parts[0].notes[0].pluck_technique, PluckTechnique::Soft);
        assert_eq!(machine_score.parts[0].notes[0].finger_technique, FingerTechnique::Quiet);

        assert_eq!(machine_score.parts[0].notes[1].start, Fraction::new(1u32, 2u32));
        assert_eq!(machine_score.parts[0].notes[1].length, Fraction::new(1u32, 4u32));
        assert_eq!(machine_score.parts[0].notes[1].pitch, MidiPitch { pitch: 50 });
        assert_eq!(machine_score.parts[0].notes[1].volume, MidiVolume { volume: 48 });
        assert_eq!(machine_score.parts[0].notes[1].pluck_technique, PluckTechnique::Hard);
        assert_eq!(machine_score.parts[0].notes[1].finger_technique, FingerTechnique::Quiet);
    }

    #[test]
    fn conversion_resets_timing_for_each_part() {
        let quarter = Some(NoteDuration {
            ratio: 4,
            augmentation: 0,
        });
        let score = LilyScore {
            header: None,
            global: ast::Global::default(),
            parts: [
                LilyPart {
                    name: "one".to_owned(),
                    events: vec![Event::Note(lily_note(
                        PitchClass::C,
                        0,
                        quarter,
                        Some(Dynamic::MF),
                        Articulation::none(),
                        None,
                    ))],
                },
                LilyPart {
                    name: "two".to_owned(),
                    events: vec![Event::Note(lily_note(
                        PitchClass::E,
                        0,
                        quarter,
                        Some(Dynamic::MF),
                        Articulation::none(),
                        None,
                    ))],
                },
                empty_part("three"),
                empty_part("four"),
                empty_part("five"),
                empty_part("six"),
            ],
        };

        let machine_score = MachineScore::from_lilyscore(score);

        assert_eq!(machine_score.parts[0].notes[0].start, Fraction::new(0u32, 1u32));
        assert_eq!(machine_score.parts[1].notes[0].start, Fraction::new(0u32, 1u32));
    }

    #[test]
    fn bar_number_refers_to_the_bar_the_note_starts_in() {
        // 4/4: a whole note fills bar 1 (0-indexed bar 0) exactly; the following
        // quarter note starts the second bar.
        let score = score_with_first_part(vec![
            Event::Note(lily_note(
                PitchClass::C,
                0,
                Some(NoteDuration {
                    ratio: 1,
                    augmentation: 0,
                }),
                Some(Dynamic::MF),
                Articulation::none(),
                None,
            )),
            Event::Note(lily_note(PitchClass::D, 0, None, None, Articulation::none(), None)),
        ]);

        let machine_score = MachineScore::from_lilyscore(score);

        assert_eq!(machine_score.parts[0].notes[0].bar_number, 0);
        assert_eq!(machine_score.parts[0].notes[1].bar_number, 1);
        assert_eq!(machine_score.bar_count, 2);
    }

    fn machine_note(bar_number: u32, start: (u32, u32)) -> Note {
        Note {
            pitch: MidiPitch::new(60),
            volume: MidiVolume::new(80),
            length: Fraction::new(1u32, 4u32),
            start: Fraction::new(start.0, start.1),
            pluck_technique: PluckTechnique::Hard,
            finger_technique: FingerTechnique::Quiet,
            bar_number,
        }
    }

    fn multi_bar_machine_score() -> MachineScore {
        fn empty() -> MachineScorePart {
            MachineScorePart { notes: vec![] }
        }
        // 4 bars of 4/4, one quarter note at the start of each bar.
        let notes = (0..4).map(|bar| machine_note(bar, (bar, 1))).collect();
        MachineScore {
            title: "Test".to_owned(),
            parts: [empty(), empty(), empty(), empty(), empty(), MachineScorePart { notes }],
            tempo_changes: vec![
                (
                    Fraction::new(0u32, 1u32),
                    Tempo {
                        note_duration: NoteDuration {
                            ratio: 4,
                            augmentation: 0,
                        },
                        bpm: 60,
                    },
                ),
                (
                    Fraction::new(2u32, 1u32),
                    Tempo {
                        note_duration: NoteDuration {
                            ratio: 4,
                            augmentation: 0,
                        },
                        bpm: 120,
                    },
                ),
            ],
            time_signature_changes: vec![],
            bar_count: 4,
        }
    }

    #[test]
    fn extract_bar_range_filters_and_rebases() {
        let score = multi_bar_machine_score();
        let extracted = score.extract_bar_range(2, Some(3)).unwrap();

        let notes = &extracted.parts[5].notes;
        assert_eq!(notes.len(), 2);
        // Bar 2 (index 1) starts at whole-note position 1; starts are rebased to zero.
        assert_eq!(notes[0].bar_number, 1);
        assert_eq!(notes[0].start, Fraction::new(0u32, 1u32));
        assert_eq!(notes[1].bar_number, 2);
        assert_eq!(notes[1].start, Fraction::new(1u32, 1u32));
        assert_eq!(extracted.bar_count, 2);

        // Tempo at the selection start (bar 2) is 60 BPM, pinned at position zero;
        // the 120 BPM change at position 2 is shifted to position 1.
        assert_eq!(extracted.tempo_changes.len(), 2);
        assert_eq!(
            extracted.tempo_changes[0],
            (
                Fraction::new(0u32, 1u32),
                Tempo {
                    note_duration: NoteDuration {
                        ratio: 4,
                        augmentation: 0
                    },
                    bpm: 60,
                }
            )
        );
        assert_eq!(
            extracted.tempo_changes[1],
            (
                Fraction::new(1u32, 1u32),
                Tempo {
                    note_duration: NoteDuration {
                        ratio: 4,
                        augmentation: 0
                    },
                    bpm: 120,
                }
            )
        );
    }

    #[test]
    fn extract_bar_range_open_end_plays_to_end_of_score() {
        let score = multi_bar_machine_score();
        let extracted = score.extract_bar_range(3, None).unwrap();

        let notes = &extracted.parts[5].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].bar_number, 2);
        assert_eq!(notes[0].start, Fraction::new(0u32, 1u32));
        assert_eq!(notes[1].bar_number, 3);
        assert_eq!(extracted.bar_count, 2);
    }

    #[test]
    fn extract_bar_range_end_beyond_score_is_clamped() {
        let score = multi_bar_machine_score();
        let extracted = score.extract_bar_range(1, Some(100)).unwrap();
        assert_eq!(extracted.parts[5].notes.len(), 4);
        assert_eq!(extracted.bar_count, 4);
    }

    #[test]
    fn extract_bar_range_rejects_invalid_ranges() {
        let score = multi_bar_machine_score();
        assert!(score.extract_bar_range(0, None).is_err());
        assert!(score.extract_bar_range(3, Some(2)).is_err());
        assert!(score.extract_bar_range(5, None).is_err());
    }

    #[test]
    fn bar_start_position_honours_time_signature_changes() {
        use crate::machine_score::note_timer::bar_start_position;
        // Bar 0 is 2/4; bar 1 onwards switches to 4/4 at position 1/2.
        let changes = vec![
            (
                Fraction::new(0u32, 1u32),
                ast::TimeSignature {
                    numerator: 2,
                    denominator: 4,
                },
            ),
            (
                Fraction::new(1u32, 2u32),
                ast::TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
            ),
        ];
        assert_eq!(bar_start_position(0, &changes), Fraction::new(0u32, 1u32));
        assert_eq!(bar_start_position(1, &changes), Fraction::new(1u32, 2u32));
        assert_eq!(bar_start_position(2, &changes), Fraction::new(3u32, 2u32));
        assert_eq!(bar_start_position(3, &changes), Fraction::new(5u32, 2u32));
    }
}
