use fraction::Zero;
use lilyparse::syntax::ast::{Event, LilyPart, NoteOrRest, TimeSignature};

use crate::machine_score::event_timer::{EventTimer, Fraction, Notes, TimeSignatureChanges, TimedEvent};

pub struct NoteTimer<'a> {
    lily_part: &'a LilyPart,
    time_signature_changes: &'a Vec<(Notes, TimeSignature)>,
    time_signature_index: usize,
    number_of_bars: Fraction,
    position_in_bar: Notes,
    bar_length: Notes,
    time_signature: TimeSignature,
    timed_notes: Vec<NoteTimingInfo>,
}

/// Computes the length of one bar (in whole notes) for a time signature.
fn time_signature_to_bar_length(time_signature: TimeSignature) -> Notes {
    Notes::new(time_signature.numerator, time_signature.denominator)
}

/// Computes the absolute position (in whole notes from score start) of the beginning of
/// the bar with the given 0-based index, honouring bar-aligned time signature changes.
pub fn bar_start_position(bar_index: u32, time_signature_changes: &TimeSignatureChanges) -> Notes {
    let mut position = Notes::zero();
    let mut bar_length = time_signature_to_bar_length(TimeSignature::default());
    let mut next_change = 0;
    for _ in 0..bar_index {
        while next_change < time_signature_changes.len() && time_signature_changes[next_change].0 <= position {
            bar_length = time_signature_to_bar_length(time_signature_changes[next_change].1);
            next_change += 1;
        }
        position += bar_length;
    }
    position
}

/// Timing metadata for one converted note.
pub struct NoteTimingInfo {
    pub note_or_rest: NoteOrRest,
    pub time_signature: TimeSignature,
    /// 0-based index of the bar in which the note starts.
    pub bar_number: u32,
    pub position_in_bar: Notes,
    pub note_stamp: Notes,
    pub length: Notes,
    pub x_note: bool,
}

impl<'a> NoteTimer<'a> {
    fn new(part: &'a LilyPart, time_signature_changes: &'a Vec<(Notes, TimeSignature)>) -> Self {
        Self {
            lily_part: part,
            time_signature_changes,
            time_signature_index: 0,
            number_of_bars: Notes::zero(),
            position_in_bar: Notes::zero(),
            bar_length: time_signature_to_bar_length(TimeSignature::default()),
            time_signature: TimeSignature::default(),
            timed_notes: Vec::new(),
        }
    }

    fn apply_pending_time_signature_changes(&mut self, time: Notes) {
        while self.time_signature_changes.len() > self.time_signature_index {
            let (note_stamp, time_signature) = self.time_signature_changes[self.time_signature_index];
            if note_stamp <= time {
                if self.position_in_bar != Notes::zero() {
                    panic!("Unaligned time signature change");
                }

                self.bar_length = time_signature_to_bar_length(time_signature);
                self.time_signature = time_signature;
                self.time_signature_index += 1;
                continue;
            }

            break;
        }
    }

    fn to_note_or_rest(event: Event) -> Option<NoteOrRest> {
        match event {
            Event::Note(note) => Some(NoteOrRest::Note(note)),
            Event::Rest(rest) => Some(NoteOrRest::Rest(rest)),
            _ => None,
        }
    }

    fn process(&mut self) {
        let timed_events = EventTimer::get_timed_events(self.lily_part);
        for TimedEvent {
            event,
            note_stamp,
            duration,
            x_note,
        } in timed_events
        {
            self.apply_pending_time_signature_changes(note_stamp);
            if let Some(note_or_rest) = Self::to_note_or_rest(event) {
                self.timed_notes.push(NoteTimingInfo {
                    note_or_rest,
                    time_signature: self.time_signature,
                    bar_number: *self.number_of_bars.trunc().numer().unwrap(),
                    position_in_bar: self.position_in_bar,
                    note_stamp,
                    length: duration,
                    x_note,
                });
            }

            self.number_of_bars += duration / self.bar_length;
            self.position_in_bar = (self.position_in_bar + duration) % self.bar_length;
        }
    }

    pub fn get_notes(
        part: &'a LilyPart,
        time_signature_changes: &'a Vec<(Notes, TimeSignature)>,
    ) -> (Vec<NoteTimingInfo>, u32) {
        let mut instance = Self::new(part, time_signature_changes);
        instance.process();
        let bar_count = *instance.number_of_bars.ceil().numer().unwrap_or(&0);
        (instance.timed_notes, bar_count)
    }
}
