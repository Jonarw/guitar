use lilyparse::syntax::ast::{self, Articulation, Event, LilyPart, LilyScore, Rest};
use timing::TimingHelper;

use crate::machine_score::{
    dynamic::{DynamicBuilder, DynamicHelper},
    timing::Notes,
};

pub mod dynamic;
pub mod timing;

/// MIDI pitch value in range `0..=127`.
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
pub struct Note {
    pub pitch: MidiPitch,
    pub volume: MidiVolume,
    pub length: Notes,
    pub start: Notes,
    pub pluck_technique: PluckTechnique,
    pub finger_technique: FingerTechnique,
}

/// Plucking actuator strategy.
pub enum PluckTechnique {
    Soft,
    Hard,
    None,
}

/// Fretting pressure strategy.
pub enum FingerTechnique {
    Quiet,
    Loud,
}

/// Converted score containing per-string machine note instructions.
pub struct MachineScore {
    pub title: String,
    pub parts: [MachineScorePart; 6],
}

/// Converted events for one part/string.
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
            articulation: Articulation::Portato,
        }
    }

    /// Maps articulation marks to hardware techniques.
    fn current_technique(&self) -> (PluckTechnique, FingerTechnique) {
        match self.articulation {
            Articulation::Tenuto => (PluckTechnique::Soft, FingerTechnique::Quiet),
            Articulation::Portato => (PluckTechnique::Soft, FingerTechnique::Loud),
            Articulation::Staccato => (PluckTechnique::Hard, FingerTechnique::Quiet),
            Articulation::Staccatissimo => (PluckTechnique::Hard, FingerTechnique::Loud),
            Articulation::Marcato => (PluckTechnique::None, FingerTechnique::Loud),
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
        self.timing_helper.next_rest(rest);
    }

    /// Converts one LilyPond note event.
    fn process_note(&mut self, note: &ast::Note) {
        if let Some(articulation) = note.articulation {
            self.articulation = articulation;
        }

        let timing_info = self.timing_helper.next_note(note);
        let volume = self.dynamic_helper.next_note(note, &timing_info);

        let (pluck_technique, finger_technique) = self.current_technique();
        self.notes.push(Note {
            pitch: Self::note_to_midi_pitch(note),
            volume,
            length: timing_info.length,
            start: timing_info.note_stamp,
            pluck_technique,
            finger_technique,
        });
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
        }
    }
}
