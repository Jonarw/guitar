use std::{panic, todo};

use fraction::{GenericFraction, Zero};
use lilyparse::syntax::ast::{self, Event, LilyPart, LilyScore, NoteDuration, Rest, Tempo, TimeSignature};

type Fraction = GenericFraction<u32>;
pub type Notes = Fraction;
type Seconds = Fraction;
type SecondsPerNote = Fraction;

fn note_duration_to_notes(note_duration: NoteDuration) -> Fraction {
    let mut fraction = Fraction::new(
        note_duration.ratio << (note_duration.augmentation - 1) - 1,
        note_duration.ratio << note_duration.augmentation,
    );

    if let Some(tuplet) = note_duration.tuplet {
        fraction /= Fraction::new(tuplet.num, tuplet.den)
    }

    fraction
}

fn time_signature_to_bar_length(time_signature: TimeSignature) -> Fraction {
    Fraction::new(time_signature.numerator, time_signature.denominator)
}

fn tempo_to_seconds_per_note(tempo: Tempo) -> SecondsPerNote {
    let beat = note_duration_to_notes(tempo.note_duration);
    let whole_notes_per_minute = Fraction::from(tempo.bpm) * beat;
    let whole_notes_per_second = Fraction::from(60) * whole_notes_per_minute;

    // result in seconds per whole note
    whole_notes_per_second.recip()
}

#[derive(Debug, PartialEq, Eq)]
pub enum TimingEvent {
    TimeSignature(TimeSignature),
    Tempo(Tempo),
}

pub struct TimedEvent {
    pub event: Event,
    pub note_stamp: Notes,
}

pub struct TimingHelper {
    time_signature_changes: Vec<(Notes, TimeSignature)>,
    time_signature_index: usize,
    tempo_changes: Vec<(Notes, Tempo)>,
    number_of_bars: Fraction,
    position_in_bar: Notes,
    note_stamp: Notes,
    bar_length: Notes,
    time_signature: TimeSignature,
    note_length: Notes,
}

struct NoteTimingInfo {
    note: ast::Note,
    time_signature: TimeSignature,
    bar_number: u32,
    position_in_bar: Notes,
    note_stamp: Notes,
    length: Notes,
}

impl TimingHelper {
    fn advance(&mut self, note_duration: Option<NoteDuration>) {
        if let Some(note_duration) = note_duration {
            self.note_length = note_duration_to_notes(note_duration);
        }

        self.note_stamp += self.note_length;
        self.number_of_bars += self.note_length / self.bar_length;
        self.position_in_bar = (self.position_in_bar + self.note_length) % self.bar_length;

        if self.time_signature_changes.len() > self.time_signature_index {
            let (note_stamp, time_signature) = self.time_signature_changes[self.time_signature_index];
            if note_stamp <= self.note_stamp {
                self.next_time_signature(time_signature);
            }
        }
    }

    pub fn reset(self) -> Self {
        TimingHelper::new(self.time_signature_changes, self.tempo_changes)
    }

    pub fn next_rest(&mut self, rest: Rest) {
        self.advance(rest.duration);
    }

    pub fn next_note(&mut self, note: ast::Note) -> NoteTimingInfo {
        let bar_number = *self.number_of_bars.trunc().numer().unwrap();
        let mut ret = NoteTimingInfo {
            note: note.clone(),
            time_signature: self.time_signature,
            bar_number: bar_number,
            position_in_bar: self.position_in_bar,
            note_stamp: self.note_stamp,
            length: Notes::default(),
        };

        self.advance(note.duration);
        ret.length = self.note_length;
        ret
    }

    fn next_time_signature(&mut self, time_signature: TimeSignature) {
        if self.position_in_bar != Notes::zero() {
            panic!("Unaligned time signature change");
        }

        self.bar_length = time_signature_to_bar_length(time_signature);
        self.time_signature = time_signature;
    }

    fn get_timed_events(part: LilyPart) -> Vec<TimedEvent> {
        let mut time = Notes::default();
        let mut current_duration = NoteDuration::default();
        let mut ret = Vec::new();

        for event in part.events.iter() {
            ret.push(TimedEvent {
                event: event.clone(),
                note_stamp: time,
            });

            match event {
                Event::Note(note) => {
                    if let Some(nd) = note.duration {
                        current_duration = nd;
                    }

                    time += note_duration_to_notes(current_duration);
                }
                Event::Rest(rest) => {
                    if let Some(nd) = rest.duration {
                        current_duration = nd;
                    }

                    time += note_duration_to_notes(current_duration);
                }
                _ => {}
            }
        }

        ret
    }

    fn extract_time_signature_and_tempo_changes(
        score: &LilyScore,
    ) -> (Vec<(Notes, TimeSignature)>, Vec<(Notes, Tempo)>) {
        let mut tempo_changes = Vec::new();
        let mut time_signature_changes = Vec::new();

        for part in &score.parts {
            let mut time = Notes::default();
            let mut current_duration = NoteDuration::default();
            for event in part.events.iter() {
                match event {
                    Event::Note(note) => {
                        if let Some(nd) = note.duration {
                            current_duration = nd;
                        }

                        time += note_duration_to_notes(current_duration);
                    }
                    Event::Rest(rest) => {
                        if let Some(nd) = rest.duration {
                            current_duration = nd;
                        }

                        time += note_duration_to_notes(current_duration);
                    }
                    Event::TimeSignature(time_signature) => time_signature_changes.push((time, *time_signature)),
                    Event::Tempo(tempo) => tempo_changes.push((time, *tempo)),
                }
            }
        }

        time_signature_changes.sort_unstable_by_key(|t| t.0);
        time_signature_changes.dedup();

        tempo_changes.sort_unstable_by_key(|t| t.0);
        tempo_changes.dedup();
        (time_signature_changes, tempo_changes)
    }

    fn new(time_signature_changes: Vec<(Notes, TimeSignature)>, tempo_changes: Vec<(Notes, Tempo)>) -> Self {
        Self {
            time_signature_changes,
            time_signature_index: 0,
            tempo_changes,
            number_of_bars: Fraction::zero(),
            position_in_bar: Notes::zero(),
            note_stamp: Notes::zero(),
            time_signature: TimeSignature::default(),
            bar_length: time_signature_to_bar_length(TimeSignature::default()),
            note_length: note_duration_to_notes(NoteDuration::default()),
        }
    }

    pub fn from_score(score: &LilyScore) -> Self {
        let (time_signature_changes, tempo_changes) = Self::extract_time_signature_and_tempo_changes(score);
        Self::new(time_signature_changes, tempo_changes)
    }
}
