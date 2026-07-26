use std::panic;

use lilyparse::syntax::ast::{self, Articulation, Event, LilyPart, LilyScore, Rest};
use timing::TimingHelper;

use crate::machine_score::{
    dynamic::{DynamicBuilder, DynamicHelper},
    timing::Notes,
};

pub mod dynamic;
pub mod timing;

pub struct MidiPitch {
    pub pitch: u8,
}

impl MidiPitch {
    pub fn new(pitch: u8) -> Self {
        if pitch > 127 {
            panic!("MIDI pitch outside of allowed range");
        }

        Self { pitch }
    }
}

pub struct MidiVolume {
    pub volume: u8,
}

impl MidiVolume {
    const MAX_VALUE: u8 = 127;
    pub fn new(volume: u8) -> Self {
        if volume > Self::MAX_VALUE {
            panic!("MIDI volume outside of allowed range");
        }

        Self { volume }
    }

    pub fn from_f64(volume: f64) -> Self {
        if volume >= 1.0 || volume < 0.0 {
            panic!("MIDI pitch outside of allowed range");
        }

        Self {
            volume: (volume * 128.0).floor() as u8,
        }
    }
}

pub struct Note {
    pub pitch: MidiPitch,
    pub volume: MidiVolume,
    pub length: Notes,
    pub start: Notes,
    pub pluck_technique: PluckTechnique,
    pub finger_technique: FingerTechnique,
}

pub enum PluckTechnique {
    Soft,
    Hard,
    None,
}

pub enum FingerTechnique {
    Quiet,
    Loud,
}

pub struct MachineScore {
    pub title: String,
    pub parts: [MachineScorePart; 6],
}

pub struct MachineScorePart {
    pub name: String,
    pub notes: Vec<Note>,
}

struct LilyPartConverter<'a> {
    lily_part: &'a LilyPart,
    timing_helper: &'a mut TimingHelper,
    dynamic_helper: DynamicHelper,
    notes: Vec<Note>,
    articulation: Articulation,
}

impl<'a> LilyPartConverter<'a> {
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

    fn current_technique(&self) -> (PluckTechnique, FingerTechnique) {
        match self.articulation {
            Articulation::Tenuto => (PluckTechnique::Soft, FingerTechnique::Quiet),
            Articulation::Portato => (PluckTechnique::Soft, FingerTechnique::Loud),
            Articulation::Staccato => (PluckTechnique::Hard, FingerTechnique::Quiet),
            Articulation::Staccatissimo => (PluckTechnique::Hard, FingerTechnique::Loud),
            Articulation::Marcato => (PluckTechnique::None, FingerTechnique::Loud),
        }
    }

    fn note_to_midi_pitch(note: &ast::Note) -> u8 {
        const MIDI_C: i8 = 48;
        const SEMITONES_PER_OCTAVE: i8 = 12;

        (MIDI_C
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
            + note.octave * SEMITONES_PER_OCTAVE) as u8
    }

    fn process_rest(&mut self, rest: &Rest) {
        self.timing_helper.next_rest(rest);
    }

    fn process_note(&mut self, note: &ast::Note) {
        if let Some(articulation) = note.articulation {
            self.articulation = articulation;
        }

        let timing_info = self.timing_helper.next_note(note);
        let volume = self.dynamic_helper.next_note(note, &timing_info);

        let (pluck_technique, finger_technique) = self.current_technique();
        self.notes.push(Note {
            pitch: MidiPitch::new(Self::note_to_midi_pitch(note)),
            volume,
            length: timing_info.length,
            start: timing_info.note_stamp,
            pluck_technique,
            finger_technique,
        });
    }

    pub fn convert(mut self) -> MachineScorePart {
        for event in self.lily_part.events.iter() {
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
    pub fn from_lilyscore(score: LilyScore) -> Self {
        let mut score_title = "".to_owned();
        if let Some(header) = &score.header
            && let Some(title) = &header.title
        {
            score_title = title.clone();
        }

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
