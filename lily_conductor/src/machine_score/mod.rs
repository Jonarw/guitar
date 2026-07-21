use std::{panic, time::Duration, todo};

use lilyparse::syntax::ast::{self, Articulation, Dynamic, Event, LilyPart, LilyScore, NoteDuration, Rest};

pub struct MidiPitch {
    pitch: u8,
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
    volume: u8,
}

impl MidiVolume {
    pub fn new(volume: u8) -> Self {
        if volume > 127 {
            panic!("MIDI pitch outside of allowed range");
        }

        Self { volume }
    }
}

pub struct Note {
    pub pitch: MidiPitch,
    pub volume: MidiVolume,
    pub duration: Duration,
    pub start: Duration,
    pub pluck_technique: PluckTechnique,
    pub finger_slap: bool,
}

pub enum PluckTechnique {
    Soft,
    Hard,
    None,
}

pub struct MachineScore {
    title: String,
    parts: [MachineScorePart; 6],
}

pub struct MachineScorePart {
    name: String,
    notes: Vec<Note>,
}

struct CrescendoBlock {
    start_time: Duration,
    end_time: Duration,
    start_dynamic: Dynamic,
    end_dynamic: Dynamic,
}

struct LilyPartConverter<'a> {
    whole_note_duration: Duration,
    lily_part: &'a LilyPart,
    notes: Vec<Note>,
    current_time: Duration,
    current_note_duration: NoteDuration,
    current_dynamic: Dynamic,
    current_crescendo: Option<CrescendoBlock>,
    current_articulation: Articulation,
    current_event_index: u32,
}

impl<'a> LilyPartConverter<'a> {
    pub fn new(whole_note_duration: Duration, lily_part: &'a LilyPart) -> Self {
        Self {
            whole_note_duration,
            lily_part,
            notes: Vec::new(),
            current_time: Duration::default(),
            current_note_duration: NoteDuration {
                ratio: 4,
                augmentation: 0,
            },
            current_dynamic: Dynamic::MF,
            current_crescendo: None,
            current_articulation: Articulation::Portato,
            current_event_index: 0,
        }
    }

    fn current_technique(&self) -> (PluckTechnique, bool) {
        match self.current_articulation {
            Articulation::Tenuto => (PluckTechnique::Soft, false),
            Articulation::Portato => (PluckTechnique::Soft, true),
            Articulation::Staccato => (PluckTechnique::Hard, false),
            Articulation::Staccatissimo => (PluckTechnique::Hard, true),
            Articulation::Marcato => (PluckTechnique::None, true),
        }
    }

    fn note_to_midi_pitch(note: &ast::Note) -> u8 {
        let mut ret = 48; // midi pitch of c
        ret += match note.class {
            ast::PitchClass::C => 0,
            ast::PitchClass::D => 2,
            ast::PitchClass::E => 4,
            ast::PitchClass::F => 5,
            ast::PitchClass::G => 7,
            ast::PitchClass::A => 9,
            ast::PitchClass::B => 11,
        };

        ret += match note.accidental {
            ast::Accidental::DoubleFlat => -2,
            ast::Accidental::Flat => -1,
            ast::Accidental::None => 0,
            ast::Accidental::Sharp => 1,
            ast::Accidental::DoubleSharp => 2,
        };

        ret += note.octave * 12;

        ret as u8
    }

    fn current_duration(&self) -> Duration {
        let fraction = MachineScore::note_duration_to_whole_note_fraction(&self.current_note_duration);
        self.whole_note_duration.mul_f64(fraction)
    }

    fn process_rest(&mut self, rest: &Rest) {
        if let Some(duration) = rest.duration {
            self.current_note_duration = duration;
        }

        self.current_time += self.current_duration();
    }

    fn process_note(&mut self, note: &ast::Note) {
        if let Some(duration) = note.duration {
            self.current_note_duration = duration;
        }

        if let Some(articulation) = note.articulation {
            self.current_articulation = articulation;
        }

        if let Some(dynamic) = note.dynamic {
            self.current_dynamic = dynamic;
        }

        if let Some(crescendo) = note.crescendo {
            self.current_crescendo = Some(CrescendoBlock {
                start_time: todo!(),
                end_time: todo!(),
                start_dynamic: todo!(),
                end_dynamic: todo!(),
            });
        }

        let duration = self.current_duration();
        let (pluck_technique, finger_slap) = self.current_technique();
        self.notes.push(Note {
            pitch: MidiPitch::new(Self::note_to_midi_pitch(&note)),
            volume: MidiVolume::new(0),
            duration,
            start: self.current_time,
            pluck_technique,
            finger_slap,
        });

        self.current_time += duration;
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
            }

            self.current_event_index += 1;
        }

        MachineScorePart {
            name: self.lily_part.name.clone(),
            notes: self.notes,
        }
    }
}

impl MachineScorePart {
    pub fn from_lilypart(part: LilyPart, whole_note_duration: Duration) -> Self {
        let name = part.name;
        let mut notes = Vec::new();
        let mut current_offset = Duration::from_secs(0);
        let mut current_dynamic = Dynamic::MF;
        let mut current_duration = ast::NoteDuration {
            ratio: 4,
            augmentation: 0,
        };

        let mut current_crescendo = None;
        let mut current_articulation = Articulation::Tenuto;

        for (i, event) in part.events.iter().enumerate() {
            match event {
                Event::Note(note) => {
                    if let Some(duration) = note.duration {
                        current_duration = duration;
                    }

                    if let Some(articulation) = note.articulation {
                        current_articulation = articulation;
                    }

                    if let Some(dynamic) = note.dynamic {
                        current_dynamic = dynamic;
                    }

                    if let Some(crescendo) = note.crescendo {
                        current_crescendo = Some(CrescendoBlock {
                            start_time: todo!(),
                            end_time: todo!(),
                            start_dynamic: todo!(),
                            end_dynamic: todo!(),
                        });
                    }

                    let duration = MachineScore::note_duration_to_duration(&current_duration, whole_note_duration);
                    let (pluck_technique, finger_slap) = Self::articulation_to_technique(&current_articulation);
                    notes.push(Note {
                        pitch: MidiPitch::new(Self::note_to_midi_pitch(&note)),
                        volume: MidiVolume::new(0),
                        duration,
                        start: current_offset,
                        pluck_technique,
                        finger_slap,
                    });

                    current_offset += duration;
                }
                Event::Rest(rest) => {
                    if let Some(duration) = rest.duration {
                        current_duration = duration;
                    }

                    let duration = MachineScore::note_duration_to_duration(&current_duration, whole_note_duration);
                    current_offset += duration;
                }
            }
        }

        Self { name, notes }
    }
}

impl MachineScore {
    pub fn note_duration_to_whole_note_fraction(duration: &ast::NoteDuration) -> f64 {
        let note_fraction = 1.0 / duration.ratio as f64;
        let augmentation = 1.0 - 1.0 / (1 << duration.augmentation) as f64;

        note_fraction + augmentation
    }

    pub fn from_lilyscore(score: LilyScore) -> Self {
        let mut score_title = "".to_owned();
        if let Some(header) = score.header
            && let Some(title) = header.title
        {
            score_title = title;
        }

        let mut whole_note_duration = Duration::from_secs(2);
        if let Some(tempo) = score.global.tempo {
            whole_note_duration = Duration::from_secs_f64(
                Self::note_duration_to_whole_note_fraction(&tempo.note_duration) / tempo.bpm as f64,
            );
        }

        let parts = score
            .parts
            .map(|p| MachineScorePart::from_lilypart(p, whole_note_duration));

        Self {
            title: score_title,
            parts,
        }
    }
}
