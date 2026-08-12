use fraction::{GenericFraction, One, Zero};
use lilyparse::syntax::ast::{Event, LilyPart, LilyScore, NoteDuration, Rest, Tempo, TimeSignature};

/// Rational number type used for musical time calculations.
pub type Fraction = GenericFraction<u32>;
/// Time unit measured in whole-note fractions.
pub type Notes = Fraction;

pub type TimeSignatureChanges = Vec<(Notes, TimeSignature)>;
pub type TempoChanges = Vec<(Notes, Tempo)>;

/// Original event tagged with its absolute note position.
pub struct TimedEvent {
    pub event: Event,
    pub note_stamp: Notes,
    pub duration: Notes,
    pub x_note: bool,
}

pub struct EventTimer {
    time: Notes,
    current_duration: Notes,
    tuplet_modifier: Notes,
    events: Vec<TimedEvent>,
    x_note: bool,
}

impl EventTimer {
    /// Converts a parsed LilyPond duration to a fractional note length.
    fn note_duration_to_notes(note_duration: NoteDuration) -> Notes {
        let mut dotted_numerator = 1u32;
        let mut dotted_denominator = 1u32;
        // Dot series: 1, 3/2, 7/4, ... = (2^(dots+1)-1)/2^dots
        for _ in 0..note_duration.augmentation {
            dotted_numerator = (dotted_numerator * 2) + 1;
            dotted_denominator *= 2;
        }

        let fraction = Notes::new(dotted_numerator, u32::from(note_duration.ratio) * dotted_denominator);
        fraction
    }

    /// Converts a parsed LilyPond duration to a fractional note length.
    fn effective_rest_duration_to_notes(rest: &Rest) -> Option<Notes> {
        rest.duration.map(|d| {
            let base_duration = Self::note_duration_to_notes(d);
            let mul = rest.multipliers.iter().product::<u32>();
            let div = rest.dividers.iter().product::<u32>();
            let modifier = Notes::new(mul, div);
            base_duration * modifier
        })
    }

    fn new() -> Self {
        Self {
            time: Notes::zero(),
            current_duration: Self::note_duration_to_notes(NoteDuration::default()),
            tuplet_modifier: Notes::one(),
            events: Vec::new(),
            x_note: false,
        }
    }

    fn process_event(&mut self, event: &Event) {
        let duration = match event {
            Event::Note(note) => {
                if let Some(duration) = note.duration {
                    self.current_duration = Self::note_duration_to_notes(duration);
                }

                self.current_duration
            }
            Event::Rest(rest) => {
                // current_duration (affecting following notes) does not depend on modifiers
                if let Some(duration) = rest.duration {
                    self.current_duration = Self::note_duration_to_notes(duration);
                }

                if let Some(effective_duration) = Self::effective_rest_duration_to_notes(rest) {
                    effective_duration
                } else {
                    self.current_duration
                }
            }
            _ => Notes::zero(),
        };

        let duration = duration / self.tuplet_modifier;

        self.events.push(TimedEvent {
            event: event.clone(),
            note_stamp: self.time,
            duration,
            x_note: self.x_note,
        });

        self.time += duration;

        match event {
            Event::Tuplet(tuplet) => {
                let tuplet_ratio = Notes::new(tuplet.num, tuplet.den);

                self.tuplet_modifier *= tuplet_ratio;
                for event in &tuplet.events {
                    self.process_event(event);
                }

                self.tuplet_modifier /= tuplet_ratio;
            }
            Event::Xnotes(xnotes) => {
                let x_note_previous = self.x_note;
                self.x_note = true;
                for event in &xnotes.events {
                    self.process_event(event);
                }

                self.x_note = x_note_previous;
            }
            _ => {}
        }
    }

    fn process(&mut self, part: &LilyPart) {
        for event in &part.events {
            self.process_event(event);
        }
    }

    pub fn extract_time_signature_and_tempo_changes(score: &LilyScore) -> (TimeSignatureChanges, TempoChanges) {
        let mut tempo_changes = Vec::new();
        let mut time_signature_changes = Vec::new();

        for part in &score.parts {
            let timed_events = Self::get_timed_events(part);
            for TimedEvent {
                event,
                note_stamp,
                duration: _,
                x_note: _,
            } in timed_events.iter()
            {
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

    pub fn get_timed_events(part: &LilyPart) -> Vec<TimedEvent> {
        let mut instance = Self::new();
        instance.process(part);
        instance.events
    }
}
