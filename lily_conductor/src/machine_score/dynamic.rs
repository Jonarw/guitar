use core::panic;
use std::todo;

use lilyparse::syntax::ast::{self, Crescendo, Dynamic, Event, LilyPart};

use crate::machine_score::{
    MidiVolume, dynamic,
    timing::{Notes, TimedEvent, TimingHelper},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrescendoKind {
    None,
    Crescendo,
    Decrescendo,
}

#[derive(Clone)]
struct CrescendoBlock {
    start: CrescendoPoint,
    end: CrescendoPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CrescendoPoint {
    time: Notes,
    dynamic: Dynamic,
}

pub struct DynamicHelper {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_index: usize,
    dynamic: Dynamic,
}

pub struct DynamicBuilder<'a> {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_start: CrescendoPoint,
    crescendo_kind: CrescendoKind,
    part: &'a LilyPart,
    dynamic: Dynamic,
    note_stamp: Notes,
}

impl<'a> DynamicBuilder<'a> {
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

    fn check_for_crescendo_end(&mut self, event: &Event) {
        if self.crescendo_kind == CrescendoKind::None {
            return;
        };

        if let Event::Note(note) = &event {
            if let Some(dynamic) = note.dynamic {
                self.end_block(dynamic);
            } else if note.crescendo.is_some() {
                let dynamic = Self::infer_crescendo_end_dynamic(self);
                self.end_block(dynamic);
            }
        }
    }

    fn check_for_crescendo_start(&mut self, event: &Event) {
        if self.crescendo_kind != CrescendoKind::None {
            return;
        };

        if let Event::Note(note) = &event {
            if let Some(crescendo) = note.crescendo
                && crescendo != Crescendo::End
            {
                self.start_block(crescendo);
            }
        }
    }

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

    fn update_state(&mut self, timed_event: &TimedEvent) {
        let TimedEvent { event, note_stamp } = timed_event;

        if let Event::Note(note) = event {
            self.note_stamp = *note_stamp;
            if let Some(dynamic) = note.dynamic {
                self.dynamic = dynamic;
            }
        }
    }

    fn init(&mut self) {
        let timed_events = TimingHelper::get_timed_events(self.part);

        for timed_event in timed_events {
            self.update_state(&timed_event);
            self.check_for_crescendo_end(&timed_event.event);
            self.check_for_crescendo_start(&timed_event.event);
        }
    }

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

    pub fn next_note(&mut self, note: ast::Note, note_stamp: Notes) -> MidiVolume {
        if let Some(dynamic) = note.dynamic {
            self.dynamic = dynamic;
            return MidiVolume::from_f64(Self::dynamic_to_volume(dynamic));
        }

        if self.crescendo_index < self.crescendo_blocks.len() {
            let crescendo_block = self.crescendo_blocks[self.crescendo_index];
            if crescendo_block.start.time < note_stamp {
                // we are inside the crescendo block!
            }
        }
    }
}
