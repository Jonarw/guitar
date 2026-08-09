use std::panic;

use lilyparse::syntax::ast::{self, Articulation, Event, LilyPart, LilyScore, Rest, Slur, Tempo};
use timing::TimingHelper;

use crate::machine_score::{
    dynamic::{DynamicBuilder, DynamicHelper},
    timing::Notes,
};

pub mod dynamic;
pub mod timing;

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
#[derive(Debug, PartialEq, Eq)]
pub struct Note {
    pub pitch: MidiPitch,
    pub volume: MidiVolume,
    pub length: Notes,
    pub start: Notes,
    pub pluck_technique: PluckTechnique,
    pub finger_technique: FingerTechnique,
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
    pub tempo_changes: Vec<(Notes, Tempo)>,
}

/// Converted events for one part/string.
#[derive(Debug, PartialEq, Eq)]
pub struct MachineScorePart {
    pub name: String,
    pub notes: Vec<Note>,
}

/// Stateful converter for one LilyPond part.
struct LilyPartConverter<'a> {
    lily_part: &'a LilyPart,
    timing_helper: &'a mut TimingHelper,
    dynamic_helper: DynamicHelper,
    notes: Vec<Note>,
    articulation: Articulation,
    next_note_tied: bool,
    slur_in_progress: bool,
}

impl<'a> LilyPartConverter<'a> {
    /// Creates a converter with fresh timing/dynamic state.
    pub fn new(lily_part: &'a LilyPart, timing_helper: &'a mut TimingHelper) -> Self {
        let dynamic_helper = DynamicBuilder::build(lily_part);
        timing_helper.reset();
        Self {
            lily_part,
            timing_helper,
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
        self.timing_helper.next_rest(rest);
    }

    /// Converts one LilyPond note event.
    fn process_note(&mut self, note: &ast::Note) {
        if !note.articulation.is_none() {
            self.articulation = note.articulation;
        }

        let timing_info = self.timing_helper.next_note(note);
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
            });
        }

        self.next_note_tied = note.tie;
    }

    /// Converts all note-like events in the part.
    pub fn convert(mut self) -> MachineScorePart {
        for event in &self.lily_part.events {
            match event {
                Event::Note(note) => {
                    self.process_note(note);
                }
                Event::Rest(rest) => {
                    self.process_rest(rest);
                }
                _ => {}
            }
        }

        MachineScorePart {
            name: self.lily_part.name.clone(),
            notes: self.notes,
        }
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

        let mut timing_helper = TimingHelper::from_score(&score);
        let parts = score
            .parts
            .map(|p| LilyPartConverter::new(&p, &mut timing_helper).convert());

        Self {
            title: score_title,
            parts,
            tempo_changes: timing_helper.get_tempo_changes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyparse::syntax::ast::{Accidental, Crescendo, Dynamic, Header, Note as LilyNote, NoteDuration, PitchClass};

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
                    tuplet: None,
                }),
                Some(Dynamic::MF),
                Articulation::Tenuto,
                None,
            )),
            Event::Rest(lily_rest(
                Some(NoteDuration {
                    ratio: 4,
                    augmentation: 0,
                    tuplet: None,
                }),
                Some(Dynamic::P),
            )),
            Event::Note(lily_note(PitchClass::D, 0, None, None, Articulation::Staccato, None)),
        ]);

        let machine_score = MachineScore::from_lilyscore(score);

        assert_eq!(machine_score.title, "Study");
        assert_eq!(machine_score.parts[0].notes.len(), 2);
        assert_eq!(machine_score.parts[0].notes[0].start, timing::Fraction::new(0u32, 1u32));
        assert_eq!(
            machine_score.parts[0].notes[0].length,
            timing::Fraction::new(1u32, 4u32)
        );
        assert_eq!(machine_score.parts[0].notes[0].pitch, MidiPitch { pitch: 48 });
        // MF base is 80, boosted by +10 for the primary beat stress on bar 1 beat 1.
        assert_eq!(machine_score.parts[0].notes[0].volume, MidiVolume { volume: 90 });
        assert_eq!(machine_score.parts[0].notes[0].pluck_technique, PluckTechnique::Soft);
        assert_eq!(machine_score.parts[0].notes[0].finger_technique, FingerTechnique::Quiet);

        assert_eq!(machine_score.parts[0].notes[1].start, timing::Fraction::new(1u32, 2u32));
        assert_eq!(
            machine_score.parts[0].notes[1].length,
            timing::Fraction::new(1u32, 4u32)
        );
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
            tuplet: None,
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

        assert_eq!(machine_score.parts[0].notes[0].start, timing::Fraction::new(0u32, 1u32));
        assert_eq!(machine_score.parts[1].notes[0].start, timing::Fraction::new(0u32, 1u32));
    }
}
