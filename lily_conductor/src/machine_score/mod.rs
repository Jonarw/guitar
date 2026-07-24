use std::{
    cmp::Ordering,
    matches,
    ops::{Add, AddAssign, Sub},
    panic,
    time::Duration,
    todo,
};

use fraction::{GenericFraction, One};
use itertools::Itertools;
use lilyparse::syntax::ast::{
    self, Articulation, Crescendo, Dynamic, Event, LilyPart, LilyScore, NoteDuration, Rest, Tempo, TimeSignature,
};

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
    const MAX_VALUE: u8 = 127;
    pub fn new(volume: u8) -> Self {
        if volume > Self::MAX_VALUE {
            panic!("MIDI pitch outside of allowed range");
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
    lily_part: &'a LilyPart,
    notes: Vec<Note>,
    time: Duration,
    note_duration: NoteDuration,
    dynamic: Dynamic,
    crescendo: Option<CrescendoBlock>,
    articulation: Articulation,
    event_index: usize,
    time_signature: TimeSignature,
    tempo: Tempo,
    position_in_bar: f64,
}

impl<'a> LilyPartConverter<'a> {
    pub fn new(lily_part: &'a LilyPart) -> Self {
        Self {
            lily_part,
            notes: Vec::new(),
            time: Duration::default(),
            note_duration: NoteDuration {
                ratio: 4,
                augmentation: 0,
            },
            dynamic: Dynamic::MF,
            crescendo: None,
            articulation: Articulation::Portato,
            event_index: 0,
            tempo: Tempo {
                note_duration: NoteDuration {
                    ratio: 4,
                    augmentation: 0,
                },
                bpm: 90,
            },
            position_in_bar: 0.0,
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
        }
    }

    fn current_technique(&self) -> (PluckTechnique, bool) {
        match self.articulation {
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

    pub fn note_duration_to_whole_note_fraction(duration: ast::NoteDuration) -> f64 {
        let note_fraction = 1.0 / duration.ratio as f64;
        let augmentation = 1.0 - 1.0 / (1 << duration.augmentation) as f64;

        note_fraction + augmentation
    }

    fn note_duration_to_duration(&self, note_duration: NoteDuration) -> Duration {
        let fraction = Self::note_duration_to_whole_note_fraction(note_duration);
        let whole_note_duration = Duration::from_secs_f64(
            Self::note_duration_to_whole_note_fraction(self.tempo.note_duration) / self.tempo.bpm as f64,
        );

        whole_note_duration.mul_f64(fraction)
    }

    fn process_rest(&mut self, rest: &Rest) {
        if let Some(duration) = rest.duration {
            self.note_duration = duration;
        }

        self.increment_time();
    }

    fn get_inferred_end_dynamic(&self, crescendo: Crescendo) -> Dynamic {
        match (crescendo, self.dynamic) {
            (Crescendo::CrescendoStart, Dynamic::PPP) => Dynamic::PP,
            (Crescendo::CrescendoStart, Dynamic::PP) => Dynamic::P,
            (Crescendo::CrescendoStart, Dynamic::P) => Dynamic::MP,
            (Crescendo::CrescendoStart, Dynamic::MP) => Dynamic::MF,
            (Crescendo::CrescendoStart, Dynamic::MF) => Dynamic::F,
            (Crescendo::CrescendoStart, Dynamic::F) => Dynamic::FF,
            (Crescendo::CrescendoStart, Dynamic::FF) => Dynamic::FFF,
            (Crescendo::CrescendoStart, Dynamic::FFF) => Dynamic::FFF,
            (Crescendo::DecrescendoStart, Dynamic::PPP) => Dynamic::PPP,
            (Crescendo::DecrescendoStart, Dynamic::PP) => Dynamic::PPP,
            (Crescendo::DecrescendoStart, Dynamic::P) => Dynamic::PP,
            (Crescendo::DecrescendoStart, Dynamic::MP) => Dynamic::P,
            (Crescendo::DecrescendoStart, Dynamic::MF) => Dynamic::MP,
            (Crescendo::DecrescendoStart, Dynamic::F) => Dynamic::MF,
            (Crescendo::DecrescendoStart, Dynamic::FF) => Dynamic::F,
            (Crescendo::DecrescendoStart, Dynamic::FFF) => Dynamic::FF,
            (Crescendo::End, _) => self.dynamic,
        }
    }

    fn dynamic_to_volume(dynamic: Dynamic) -> f64 {
        match dynamic {
            Dynamic::PPP => 1.0 / 8.0,
            Dynamic::PP => 2.0 / 8.0,
            Dynamic::P => 3.0 / 8.0,
            Dynamic::MP => 4.0 / 8.0,
            Dynamic::MF => 5.0 / 8.0,
            Dynamic::F => 6.0 / 8.0,
            Dynamic::FF => 7.0 / 8.0,
            Dynamic::FFF => 8.0 / 8.0,
        }
    }

    fn get_volume(&self, note: &ast::Note) -> MidiVolume {
        let volume = if let Some(dynamic) = note.dynamic {
            Self::dynamic_to_volume(dynamic)
        } else if let Some(crescendo) = &self.crescendo {
            let y1 = Self::dynamic_to_volume(crescendo.start_dynamic);
            let y2 = Self::dynamic_to_volume(crescendo.end_dynamic);
            let x1 = crescendo.start_time;
            let x2 = crescendo.end_time;
            let x = self.time;

            (x - x1).div_duration_f64(x2 - x1) * (y2 - y1) + y1
        } else {
            Self::dynamic_to_volume(self.dynamic)
        };

        MidiVolume::from_f64(volume)
    }

    fn update_crescendo_block(&mut self, crescendo: Crescendo) {
        let mut end_index = self.event_index + 1;
        let mut end_time = self.time;
        let mut end_dynamic = None;
        let mut local_current_note_duration = self.note_duration;

        loop {
            match &self.lily_part.events[end_index] {
                Event::Note(note) => {
                    if let Some(dynamic) = note.dynamic {
                        end_dynamic = Some(dynamic);
                        break;
                    }

                    if let Some(_) = note.crescendo {
                        break;
                    }

                    if let Some(note_duration) = note.duration {
                        local_current_note_duration = note_duration;
                    }
                }
                Event::Rest(rest) => {
                    if let Some(dynamic) = rest.dynamic {
                        end_dynamic = Some(dynamic);
                        break;
                    }

                    if let Some(_) = rest.crescendo {
                        break;
                    }

                    if let Some(note_duration) = rest.duration {
                        local_current_note_duration = note_duration;
                    }
                }
                _ => {}
            }

            end_time += self.note_duration_to_duration(local_current_note_duration);

            end_index += 1;
            if end_index >= self.lily_part.events.len() {
                self.crescendo = None;
                return;
            }
        }

        self.crescendo = Some(CrescendoBlock {
            start_time: self.time,
            end_time,
            start_dynamic: self.dynamic,
            end_dynamic: end_dynamic.unwrap_or_else(|| self.get_inferred_end_dynamic(crescendo)),
        })
    }

    fn increment_time(&mut self) -> Duration {
        let duration = self.note_duration_to_duration(self.note_duration);
        self.time += duration;
        self.position_in_bar += Self::note_duration_to_whole_note_fraction(self.note_duration);

        let whole_notes_in_bar = Self::note_duration_to_whole_note_fraction(self.tempo.note_duration);
        if self.position_in_bar > whole_notes_in_bar - 1e-10 {
            self.position_in_bar -= whole_notes_in_bar;
        }

        duration
    }

    fn process_note(&mut self, note: &ast::Note) {
        if let Some(duration) = note.duration {
            self.note_duration = duration;
        }

        if let Some(articulation) = note.articulation {
            self.articulation = articulation;
        }

        if let Some(dynamic) = note.dynamic {
            self.dynamic = dynamic;
        }

        if let Some(crescendo) = note.crescendo
            && matches!(crescendo, Crescendo::CrescendoStart | Crescendo::DecrescendoStart)
        {
            self.update_crescendo_block(crescendo);
        }

        let (pluck_technique, finger_slap) = self.current_technique();
        self.notes.push(Note {
            pitch: MidiPitch::new(Self::note_to_midi_pitch(note)),
            volume: self.get_volume(note),
            duration: self.note_duration_to_duration(self.note_duration),
            start: self.time,
            pluck_technique,
            finger_slap,
        });

        self.increment_time();
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
                Event::TimeSignature(time_signature) => {
                    self.time_signature = *time_signature;
                    self.position_in_bar = 0.0;
                }
                Event::Tempo(tempo) => self.tempo = *tempo,
            }

            self.event_index += 1;
        }

        MachineScorePart {
            name: self.lily_part.name.clone(),
            notes: self.notes,
        }
    }
}

type FractionType = GenericFraction<u32>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
struct MusicalDuration {
    fraction: FractionType,
}

impl Default for MusicalDuration {
    fn default() -> Self {
        Self {
            fraction: Default::default(),
        }
    }
}

impl MusicalDuration {
    pub fn new(fraction: FractionType) -> Self {
        Self { fraction }
    }
}

impl Add for MusicalDuration {
    type Output = MusicalDuration;

    fn add(self, rhs: Self) -> Self::Output {
        MusicalDuration::new(self.fraction + rhs.fraction)
    }
}

impl AddAssign for MusicalDuration {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for MusicalDuration {
    type Output = MusicalDuration;

    fn sub(self, rhs: Self) -> Self::Output {
        MusicalDuration::new(self.fraction - rhs.fraction)
    }
}

impl From<NoteDuration> for MusicalDuration {
    fn from(value: NoteDuration) -> Self {
        let mut fraction = FractionType::new(
            value.ratio << (value.augmentation - 1) - 1,
            value.ratio << value.augmentation,
        );

        if let Some(tuplet) = value.tuplet {
            fraction /= FractionType::new(tuplet.num, tuplet.den)
        }

        MusicalDuration::new(fraction)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TimingEvent {
    TimeSignature(TimeSignature),
    Tempo(Tempo),
}

struct TimingHelper {
    timing_events: Vec<(MusicalDuration, TimingEvent)>,
}

struct NoteTimingInfo {
    time: Duration,
    time_signature: TimeSignature,
    bar_number: u32,
    position_in_bar: MusicalDuration,
}

impl TimingHelper {
    pub fn get_timing_info(&self, md: MusicalDuration) -> NoteTimingInfo {
        let mut tempo = Tempo::default();
        let mut time_signature = TimeSignature::default();
        let mut time = Duration::default();
        let mut musical_time = MusicalDuration::default();
        let mut number_of_bars = 0;

        for (md, event) in self.timing_events.iter() {
            let delta = *md - musical_time;
            

            match event {
                TimingEvent::TimeSignature(time_signature) => todo!(),
                TimingEvent::Tempo(tempo) => todo!(),
            }
        }

        todo!()
    }
}

impl MachineScore {
    fn extract_timing(score: &LilyScore) -> Vec<(MusicalDuration, TimingEvent)> {
        let mut ret: Vec<_> = score
            .parts
            .iter()
            .flat_map(|p| {
                let mut ret = Vec::new();
                let mut time = MusicalDuration::default();
                let mut current_duration = NoteDuration::default();

                let advance = |time: &mut MusicalDuration, current_duration: &mut NoteDuration, opt_duration| {
                    if let Some(d) = opt_duration {
                        *current_duration = d;
                    }

                    *time += (*current_duration).into();
                };

                for event in p.events.iter() {
                    match event {
                        Event::Note(note) => advance(&mut time, &mut current_duration, note.duration),
                        Event::Rest(rest) => advance(&mut time, &mut current_duration, rest.duration),
                        Event::TimeSignature(time_signature) => {
                            ret.push((time, TimingEvent::TimeSignature(*time_signature)))
                        }
                        Event::Tempo(tempo) => ret.push((time, TimingEvent::Tempo(*tempo))),
                    }
                }

                ret
            })
            .into_iter()
            .collect();

        ret.sort_unstable_by_key(|t| t.0);
        ret.dedup();
        ret
    }

    pub fn from_lilyscore(score: LilyScore) -> Self {
        let mut score_title = "".to_owned();
        if let Some(header) = score.header
            && let Some(title) = header.title
        {
            score_title = title;
        }

        let parts = score.parts.map(|p| LilyPartConverter::new(&p).convert());

        Self {
            title: score_title,
            parts,
        }
    }
}
