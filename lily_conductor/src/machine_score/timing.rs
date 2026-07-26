use fraction::{GenericFraction, Zero};
use lilyparse::syntax::ast::{self, Event, LilyPart, LilyScore, NoteDuration, Rest, Tempo, TimeSignature};

/// Rational number type used for musical time calculations.
pub type Fraction = GenericFraction<u32>;
/// Time unit measured in whole-note fractions.
pub type Notes = Fraction;

/// Converts a parsed LilyPond duration to a fractional note length.
fn note_duration_to_notes(note_duration: NoteDuration) -> Fraction {
    let mut dotted_numerator = 1u32;
    let mut dotted_denominator = 1u32;
    // Dot series: 1, 3/2, 7/4, ... = (2^(dots+1)-1)/2^dots
    for _ in 0..note_duration.augmentation {
        dotted_numerator = (dotted_numerator * 2) + 1;
        dotted_denominator *= 2;
    }

    let mut fraction = Fraction::new(dotted_numerator, u32::from(note_duration.ratio) * dotted_denominator);

    if let Some(tuplet) = note_duration.tuplet {
        fraction /= Fraction::new(tuplet.num, tuplet.den)
    }

    fraction
}

/// Computes a full bar length from a time signature.
fn time_signature_to_bar_length(time_signature: TimeSignature) -> Fraction {
    Fraction::new(time_signature.numerator, time_signature.denominator)
}

#[derive(Debug, PartialEq, Eq)]
pub enum TimingEvent {
    TimeSignature(TimeSignature),
    Tempo(Tempo),
}

/// Original event tagged with its absolute note position.
pub struct TimedEvent {
    pub event: Event,
    pub note_stamp: Notes,
}

/// Stateful helper that tracks timeline position while iterating over events.
pub struct TimingHelper {
    time_signature_changes: Vec<(Notes, TimeSignature)>,
    time_signature_index: usize,
    _tempo_changes: Vec<(Notes, Tempo)>,
    number_of_bars: Fraction,
    position_in_bar: Notes,
    note_stamp: Notes,
    bar_length: Notes,
    time_signature: TimeSignature,
    note_length: Notes,
}

/// Timing metadata for one converted note.
pub struct NoteTimingInfo {
    pub note: ast::Note,
    pub time_signature: TimeSignature,
    pub bar_number: u32,
    pub position_in_bar: Notes,
    pub note_stamp: Notes,
    pub length: Notes,
}

impl TimingHelper {
    /// Applies any time-signature changes scheduled for the current note stamp.
    fn apply_pending_time_signature_changes(&mut self) {
        while self.time_signature_changes.len() > self.time_signature_index {
            let (note_stamp, time_signature) = self.time_signature_changes[self.time_signature_index];
            if note_stamp <= self.note_stamp {
                self.next_time_signature(time_signature);
                self.time_signature_index += 1;
                continue;
            }
            break;
        }
    }

    /// Advances timeline state using LilyPond carry-forward duration semantics.
    fn advance_by(&mut self, amount: Notes) {
        self.note_stamp += amount;
        self.number_of_bars += amount / self.bar_length;
        self.position_in_bar = (self.position_in_bar + amount) % self.bar_length;

        self.apply_pending_time_signature_changes();
    }

    /// Advances timeline state using LilyPond carry-forward duration semantics.
    fn advance(&mut self, note_duration: Option<NoteDuration>) {
        if let Some(note_duration) = note_duration {
            self.note_length = note_duration_to_notes(note_duration);
        }

        self.advance_by(self.note_length);
    }

    /// Resets helper state so it can be reused for another part traversal.
    pub fn reset(&mut self) {
        self.time_signature_index = 0;
        self.number_of_bars = Fraction::zero();
        self.position_in_bar = Notes::zero();
        self.note_stamp = Notes::zero();
        self.time_signature = TimeSignature::default();
        self.bar_length = time_signature_to_bar_length(TimeSignature::default());
        self.note_length = note_duration_to_notes(NoteDuration::default());
        self.apply_pending_time_signature_changes();
    }

    /// Advances state by one rest.
    pub fn next_rest(&mut self, rest: &Rest) {
        if let Some(multiplier) = rest.multiplier {
            let rest_duration = rest
                .duration
                .expect("Rest duration needs to be present when there is a multiplier");

            self.note_length = note_duration_to_notes(rest_duration);

            let multiplier = Fraction::new(multiplier.num, multiplier.den);
            self.advance_by(self.note_length * multiplier);
        } else {
            self.advance(rest.duration);
        }
    }

    /// Returns timing information for `note` and advances internal state.
    pub fn next_note(&mut self, note: &ast::Note) -> NoteTimingInfo {
        let bar_number = *self
            .number_of_bars
            .trunc()
            .numer()
            .expect("number_of_bars has a numerator");
        let mut ret = NoteTimingInfo {
            note: note.clone(),
            time_signature: self.time_signature,
            bar_number,
            position_in_bar: self.position_in_bar,
            note_stamp: self.note_stamp,
            length: Notes::default(),
        };

        self.advance(note.duration);
        ret.length = self.note_length;
        ret
    }

    /// Applies a time-signature change and updates derived bar state.
    fn next_time_signature(&mut self, time_signature: TimeSignature) {
        if self.position_in_bar != Notes::zero() {
            panic!("Unaligned time signature change");
        }

        self.bar_length = time_signature_to_bar_length(time_signature);
        self.time_signature = time_signature;
    }

    /// Tags each event in a part with the note position at which it occurs.
    pub fn get_timed_events(part: &LilyPart) -> Vec<TimedEvent> {
        let mut time = Notes::default();
        let mut current_duration = NoteDuration::default();
        let mut ret = Vec::new();

        for event in &part.events {
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

    /// Collects score-global timing changes from all parts.
    fn extract_time_signature_and_tempo_changes(
        score: &LilyScore,
    ) -> (Vec<(Notes, TimeSignature)>, Vec<(Notes, Tempo)>) {
        let mut tempo_changes = Vec::new();
        let mut time_signature_changes = Vec::new();

        for part in &score.parts {
            let timed_events = Self::get_timed_events(part);
            for TimedEvent { event, note_stamp } in timed_events.iter() {
                match event {
                    Event::TimeSignature(time_signature) => time_signature_changes.push((*note_stamp, *time_signature)),
                    Event::Tempo(tempo) => tempo_changes.push((*note_stamp, *tempo)),
                    _ => {}
                }
            }
        }

        time_signature_changes.sort_unstable_by_key(|t| t.0);
        time_signature_changes.dedup_by(|a, b| {
            if a.0 == b.0 && a.1 != b.1 {
                panic!("Conflicting time signatures at the same note stamp");
            }
            a == b
        });

        tempo_changes.sort_unstable_by_key(|t| t.0);
        tempo_changes.dedup_by(|a, b| {
            if a.0 == b.0 && a.1 != b.1 {
                panic!("Conflicting tempos at the same note stamp");
            }
            a == b
        });
        (time_signature_changes, tempo_changes)
    }

    /// Creates a helper from prepared timing-change sequences.
    fn new(time_signature_changes: Vec<(Notes, TimeSignature)>, tempo_changes: Vec<(Notes, Tempo)>) -> Self {
        Self {
            time_signature_changes,
            time_signature_index: 0,
            _tempo_changes: tempo_changes,
            number_of_bars: Fraction::zero(),
            position_in_bar: Notes::zero(),
            note_stamp: Notes::zero(),
            time_signature: TimeSignature::default(),
            bar_length: time_signature_to_bar_length(TimeSignature::default()),
            note_length: note_duration_to_notes(NoteDuration::default()),
        }
    }

    /// Builds a reusable timing helper from a parsed score.
    pub fn from_score(score: &LilyScore) -> Self {
        let (time_signature_changes, tempo_changes) = Self::extract_time_signature_and_tempo_changes(score);
        Self::new(time_signature_changes, tempo_changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyparse::syntax::ast::{Accidental, Dynamic, Note, PitchClass, Tuplet};

    fn note(duration: Option<NoteDuration>) -> Note {
        Note {
            class: PitchClass::C,
            accidental: Accidental::None,
            octave: 0,
            duration,
            dynamic: Some(Dynamic::MF),
            articulation: None,
            crescendo: None,
            tie: false,
        }
    }

    fn part(events: Vec<Event>) -> LilyPart {
        LilyPart {
            name: "string".to_owned(),
            events,
        }
    }

    fn empty_part(name: &str) -> LilyPart {
        LilyPart {
            name: name.to_owned(),
            events: Vec::new(),
        }
    }

    #[test]
    fn converts_dotted_and_tuplet_durations() {
        assert_eq!(
            note_duration_to_notes(NoteDuration {
                ratio: 4,
                augmentation: 0,
                tuplet: None,
            }),
            Fraction::new(1u32, 4u32)
        );
        assert_eq!(
            note_duration_to_notes(NoteDuration {
                ratio: 4,
                augmentation: 1,
                tuplet: None,
            }),
            Fraction::new(3u32, 8u32)
        );
        assert_eq!(
            note_duration_to_notes(NoteDuration {
                ratio: 8,
                augmentation: 0,
                tuplet: Some(Tuplet { num: 3, den: 2 }),
            }),
            Fraction::new(1u32, 12u32)
        );
    }

    #[test]
    fn timed_events_use_carried_note_durations() {
        let timed_events = TimingHelper::get_timed_events(&part(vec![
            Event::Note(note(Some(NoteDuration {
                ratio: 4,
                augmentation: 0,
                tuplet: None,
            }))),
            Event::Note(note(None)),
            Event::Rest(Rest {
                duration: None,
                dynamic: None,
                articulation: None,
                crescendo: None,
                multiplier: None,
            }),
        ]));

        assert_eq!(timed_events.len(), 3);
        assert_eq!(timed_events[0].note_stamp, Fraction::new(0u32, 1u32));
        assert_eq!(timed_events[1].note_stamp, Fraction::new(1u32, 4u32));
        assert_eq!(timed_events[2].note_stamp, Fraction::new(1u32, 2u32));
    }

    #[test]
    fn applies_initial_time_signature_before_first_note() {
        let score = LilyScore {
            header: None,
            global: ast::Global::default(),
            parts: [
                part(vec![
                    Event::TimeSignature(TimeSignature {
                        numerator: 3,
                        denominator: 4,
                    }),
                    Event::Note(note(Some(NoteDuration {
                        ratio: 4,
                        augmentation: 0,
                        tuplet: None,
                    }))),
                ]),
                empty_part("2"),
                empty_part("3"),
                empty_part("4"),
                empty_part("5"),
                empty_part("6"),
            ],
        };

        let mut helper = TimingHelper::from_score(&score);
        helper.reset();

        let info = helper.next_note(&note(Some(NoteDuration {
            ratio: 4,
            augmentation: 0,
            tuplet: None,
        })));

        assert_eq!(
            info.time_signature,
            TimeSignature {
                numerator: 3,
                denominator: 4,
            }
        );
    }
}
