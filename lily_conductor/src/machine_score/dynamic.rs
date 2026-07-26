use fraction::Zero;
use lilyparse::syntax::ast::{self, Crescendo, Dynamic, Event, LilyPart};

use crate::machine_score::{
    MidiVolume,
    timing::{Fraction, NoteTimingInfo, Notes, TimedEvent, TimingHelper},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrescendoKind {
    None,
    Crescendo,
    Decrescendo,
}

/// Dynamic interpolation range between two note positions.
#[derive(Clone)]
struct CrescendoBlock {
    start: CrescendoPoint,
    end: CrescendoPoint,
}

/// One sampled point on the dynamic timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CrescendoPoint {
    time: Notes,
    dynamic: Dynamic,
}

/// Runtime helper that resolves note volumes, including hairpin interpolation.
pub struct DynamicHelper {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_index: usize,
    dynamic: Dynamic,
}

/// Builder that scans one part and extracts crescendo/decrescendo blocks.
pub struct DynamicBuilder<'a> {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_start: CrescendoPoint,
    crescendo_kind: CrescendoKind,
    part: &'a LilyPart,
    dynamic: Dynamic,
    note_stamp: Notes,
}

impl<'a> DynamicBuilder<'a> {
    /// Infers a one-step target dynamic when no explicit end dynamic is given.
    fn infer_crescendo_end_dynamic(&mut self) -> Dynamic {
        match (self.crescendo_kind, self.crescendo_start.dynamic) {
            (CrescendoKind::Crescendo, Dynamic::PPP) => Dynamic::PP,
            (CrescendoKind::Crescendo, Dynamic::PP) => Dynamic::P,
            (CrescendoKind::Crescendo, Dynamic::P) => Dynamic::MP,
            (CrescendoKind::Crescendo, Dynamic::MP) => Dynamic::MF,
            (CrescendoKind::Crescendo, Dynamic::MF) => Dynamic::F,
            (CrescendoKind::Crescendo, Dynamic::F) => Dynamic::FF,
            (CrescendoKind::Crescendo, Dynamic::FF) => Dynamic::FFF,
            (CrescendoKind::Crescendo, Dynamic::FFF) => Dynamic::FFF,
            (CrescendoKind::Decrescendo, Dynamic::PPP) => Dynamic::PPP,
            (CrescendoKind::Decrescendo, Dynamic::PP) => Dynamic::PPP,
            (CrescendoKind::Decrescendo, Dynamic::P) => Dynamic::PP,
            (CrescendoKind::Decrescendo, Dynamic::MP) => Dynamic::P,
            (CrescendoKind::Decrescendo, Dynamic::MF) => Dynamic::MP,
            (CrescendoKind::Decrescendo, Dynamic::F) => Dynamic::MF,
            (CrescendoKind::Decrescendo, Dynamic::FF) => Dynamic::F,
            (CrescendoKind::Decrescendo, Dynamic::FFF) => Dynamic::FF,
            (CrescendoKind::None, _) => panic!("Cannot infer dynamic with CrescendoKind::None"),
        }
    }

    /// Closes the current block at the current note stamp.
    fn end_block(&mut self, dynamic: Dynamic) {
        let end = CrescendoPoint {
            time: self.note_stamp,
            dynamic,
        };

        let crescendo_block = CrescendoBlock {
            start: self.crescendo_start,
            end,
        };

        self.crescendo_blocks.push(crescendo_block);
        self.crescendo_kind = CrescendoKind::None;
    }

    /// Starts a new crescendo/decrescendo block.
    fn start_block(&mut self, crescendo: Crescendo) {
        self.crescendo_start = CrescendoPoint {
            time: self.note_stamp,
            dynamic: self.dynamic,
        };

        self.crescendo_kind = match crescendo {
            Crescendo::CrescendoStart => CrescendoKind::Crescendo,
            Crescendo::DecrescendoStart => CrescendoKind::Decrescendo,
            Crescendo::End => panic!("Can't start crescendo with Crescendo::End"),
        }
    }

    /// Ends the active block when an explicit end cue is encountered.
    fn check_for_crescendo_end(&mut self, event: &Event) {
        if self.crescendo_kind == CrescendoKind::None {
            return;
        }

        if let Event::Note(note) = event {
            if let Some(dynamic) = note.dynamic {
                self.end_block(dynamic);
            } else if note.crescendo.is_some() {
                let dynamic = Self::infer_crescendo_end_dynamic(self);
                self.end_block(dynamic);
            }
        }
    }

    /// Starts a block when no block is currently active.
    fn check_for_crescendo_start(&mut self, event: &Event) {
        if self.crescendo_kind != CrescendoKind::None {
            return;
        }

        if let Event::Note(note) = event {
            if let Some(crescendo) = note.crescendo
                && crescendo != Crescendo::End
            {
                self.start_block(crescendo);
            }
        }
    }

    /// Creates a builder for one part scan.
    fn new(part: &'a LilyPart) -> Self {
        Self {
            crescendo_blocks: Vec::new(),
            crescendo_start: CrescendoPoint::default(),
            part,
            dynamic: Dynamic::default(),
            note_stamp: Notes::default(),
            crescendo_kind: CrescendoKind::None,
        }
    }

    /// Updates running state from the current timeline event.
    fn update_state(&mut self, timed_event: &TimedEvent) {
        let TimedEvent { event, note_stamp } = timed_event;

        if let Event::Note(note) = event {
            self.note_stamp = *note_stamp;
            if let Some(dynamic) = note.dynamic {
                self.dynamic = dynamic;
            }
        }
    }

    /// Scans the whole part and captures crescendo blocks.
    fn init(&mut self) {
        let timed_events = TimingHelper::get_timed_events(self.part);

        for timed_event in timed_events {
            self.update_state(&timed_event);
            self.check_for_crescendo_end(&timed_event.event);
            self.check_for_crescendo_start(&timed_event.event);
        }

        if self.crescendo_kind != CrescendoKind::None {
            let inferred_end = self.infer_crescendo_end_dynamic();
            self.end_block(inferred_end);
        }
    }

    /// Builds a dynamic helper from one LilyPond part.
    pub fn build(part: &'a LilyPart) -> DynamicHelper {
        let mut builder = Self::new(part);
        builder.init();

        DynamicHelper {
            crescendo_blocks: builder.crescendo_blocks,
            crescendo_index: 0,
            dynamic: Dynamic::default(),
        }
    }
}

impl DynamicHelper {
    /// Maps textual dynamics to a linear MIDI-volume scale.
    fn dynamic_to_fraction(dynamic: Dynamic) -> Fraction {
        match dynamic {
            Dynamic::PPP => 1 * 128 / 8,
            Dynamic::PP => 2 * 128 / 8,
            Dynamic::P => 3 * 128 / 8,
            Dynamic::MP => 4 * 128 / 8,
            Dynamic::MF => 5 * 128 / 8,
            Dynamic::F => 6 * 128 / 8,
            Dynamic::FF => 7 * 128 / 8,
            Dynamic::FFF => 8 * 128 / 8,
        }
        .into()
    }

    /// Converts fraction volume to a bounded MIDI value.
    fn fraction_to_volume(fraction: Fraction) -> MidiVolume {
        if fraction < Fraction::zero() {
            panic!("fraction cannot be negative");
        }

        let num = *fraction.trunc().numer().expect("fraction has a numerator");
        if num > u32::from(MidiVolume::MAX_VALUE) {
            panic!("value outside of allowed range");
        }

        MidiVolume::new(num as u8)
    }

    /// Resolves a plain dynamic marking directly to MIDI volume.
    fn dynamic_to_volume(dynamic: Dynamic) -> MidiVolume {
        Self::fraction_to_volume(Self::dynamic_to_fraction(dynamic))
    }

    /// Returns the volume for the next note at `timing.note_stamp`.
    pub fn next_note(&mut self, note: &ast::Note, timing: &NoteTimingInfo) -> MidiVolume {
        if let Some(dynamic) = note.dynamic {
            self.dynamic = dynamic;
            return Self::dynamic_to_volume(dynamic);
        }

        while self.crescendo_index < self.crescendo_blocks.len() {
            let block = &self.crescendo_blocks[self.crescendo_index];
            if timing.note_stamp >= block.end.time {
                self.dynamic = block.end.dynamic;
                self.crescendo_index += 1;
                continue;
            }
            break;
        }

        if self.crescendo_index < self.crescendo_blocks.len() {
            let crescendo_block = &self.crescendo_blocks[self.crescendo_index];
            if timing.note_stamp >= crescendo_block.start.time && timing.note_stamp < crescendo_block.end.time {
                let y1 = Self::dynamic_to_fraction(crescendo_block.start.dynamic);
                let y2 = Self::dynamic_to_fraction(crescendo_block.end.dynamic);
                let x1 = crescendo_block.start.time;
                let x2 = crescendo_block.end.time;
                let x = timing.note_stamp;

                // Linear interpolation of dynamic value inside the active block.
                let fraction = (x - x1) / (x2 - x1) * (y2 - y1) + y1;
                return Self::fraction_to_volume(fraction);
            }
        }

        Self::dynamic_to_volume(self.dynamic)
    }
}
