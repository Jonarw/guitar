use std::{collections::HashSet, mem};

use fraction::Zero;
use lilyparse::syntax::ast::{
    self, Articulation, Crescendo, Dynamic, Event, LilyPart, NoteOrRest, Rest, TimeSignature,
};

use crate::machine_score::{
    MidiVolume,
    event_timer::{EventTimer, Fraction, Notes, TimeSignatureChanges, TimedEvent},
    note_timer::{NoteTimer, NoteTimingInfo},
};

// ---------------------------------------------------------------------------
// Beat stress classification
// ---------------------------------------------------------------------------

/// Metric stress level for a note's position within its bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatStress {
    Primary,
    Secondary,
    Regular,
}

/// Returns the stress level for a position within a bar, according to the
/// conventions of the most common time signatures.
///
/// Position is measured in whole notes from the start of the bar.
pub fn beat_stress(position_in_bar: Notes, time_signature: TimeSignature) -> BeatStress {
    // Quantise the position to a beat number (0-based) using the denominator.
    let beat_length = Fraction::new(1u32, u32::from(time_signature.denominator));
    let beat = position_in_bar / beat_length;

    match time_signature.numerator {
        2 | 3 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else {
                BeatStress::Regular
            }
        }
        4 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(3) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        5 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(4) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        6 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(4) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        9 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(4) || beat == Fraction::from(7) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        12 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(4) || beat == Fraction::from(7) || beat == Fraction::from(10) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        7 => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else if beat == Fraction::from(2) || beat == Fraction::from(5) {
                BeatStress::Secondary
            } else {
                BeatStress::Regular
            }
        }
        _ => {
            if beat == Fraction::from(0) {
                BeatStress::Primary
            } else {
                BeatStress::Regular
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic configuration
// ---------------------------------------------------------------------------

/// Additive MIDI-volume boosts applied on top of the base dynamic.
/// All values are in the same 0–127 MIDI-volume scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicConfig {
    /// Boost applied to notes carrying an `Accent` articulation.
    pub accent_boost: u8,
    /// Boost applied to notes on the primary stressed beat of the bar.
    pub primary_beat_boost: u8,
    /// Boost applied to notes on the secondary stressed beat of the bar.
    pub secondary_beat_boost: u8,
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            accent_boost: 15,
            primary_beat_boost: 10,
            secondary_beat_boost: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// Crescendo state
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// DynamicHelper
// ---------------------------------------------------------------------------

/// Runtime helper that resolves note volumes, including hairpin interpolation.
pub struct DynamicHelper {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_index: usize,
    dynamic: Dynamic,
    config: DynamicConfig,
    bars_with_accents: HashSet<u32>,
}

/// Builder that scans one part and extracts crescendo/decrescendo blocks.
pub struct DynamicBuilder {
    crescendo_blocks: Vec<CrescendoBlock>,
    crescendo_start: CrescendoPoint,
    crescendo_kind: CrescendoKind,
    bars_with_accents: HashSet<u32>,
    timed_notes: Vec<NoteTimingInfo>,
    dynamic: Dynamic,
    note_stamp: Notes,
}

impl DynamicBuilder {
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
    fn check_for_crescendo_end(&mut self, event: &NoteOrRest) {
        if self.crescendo_kind == CrescendoKind::None {
            return;
        }

        let (dynamic, crescendo) = match event {
            NoteOrRest::Note(note) => (note.dynamic, note.crescendo),
            NoteOrRest::Rest(rest) => (rest.dynamic, rest.crescendo),
        };

        if let Some(dynamic) = dynamic {
            self.end_block(dynamic);
        } else if crescendo.is_some() {
            let dynamic = Self::infer_crescendo_end_dynamic(self);
            self.end_block(dynamic);
        }
    }

    /// Starts a block when no block is currently active.
    fn check_for_crescendo_start(&mut self, event: &NoteOrRest) {
        if self.crescendo_kind != CrescendoKind::None {
            return;
        }

        let crescendo = match event {
            NoteOrRest::Note(note) => note.crescendo,
            NoteOrRest::Rest(rest) => rest.crescendo,
        };

        if let Some(crescendo) = crescendo
            && crescendo != Crescendo::End
        {
            self.start_block(crescendo);
        }
    }

    /// Creates a builder for one part scan.
    fn new(timed_notes: Vec<NoteTimingInfo>) -> Self {
        Self {
            crescendo_blocks: Vec::new(),
            crescendo_start: CrescendoPoint::default(),
            dynamic: Dynamic::default(),
            note_stamp: Notes::default(),
            timed_notes,
            crescendo_kind: CrescendoKind::None,
            bars_with_accents: HashSet::new(),
        }
    }

    /// Updates running state from the current timeline event.
    fn update_state(&mut self, nti: &NoteTimingInfo) {
        match &nti.note_or_rest {
            NoteOrRest::Note(note) => {
                self.note_stamp = nti.note_stamp;
                if let Some(dynamic) = note.dynamic {
                    self.dynamic = dynamic;
                }

                if note.articulation.contains(Articulation::Accent) {
                    self.bars_with_accents.insert(nti.bar_number);
                }
            }
            NoteOrRest::Rest(rest) => {
                self.note_stamp = nti.note_stamp;
                if let Some(dynamic) = rest.dynamic {
                    self.dynamic = dynamic;
                }
            }
        }
    }

    /// Scans the whole part and captures crescendo blocks.
    fn init(&mut self) {
        let timed_notes = mem::take(&mut self.timed_notes);
        for nti in &timed_notes {
            self.update_state(nti);
            self.check_for_crescendo_end(&nti.note_or_rest);
            self.check_for_crescendo_start(&nti.note_or_rest);
        }

        if self.crescendo_kind != CrescendoKind::None {
            let inferred_end = self.infer_crescendo_end_dynamic();
            self.end_block(inferred_end);
        }
    }

    /// Builds a dynamic helper from one LilyPond part.
    pub fn build(part: &LilyPart, time_signature_changes: &TimeSignatureChanges) -> DynamicHelper {
        Self::build_with_config(part, time_signature_changes, DynamicConfig::default())
    }

    /// Builds a dynamic helper from one LilyPond part with a custom config.
    pub fn build_with_config(
        part: &LilyPart,
        time_signature_changes: &TimeSignatureChanges,
        config: DynamicConfig,
    ) -> DynamicHelper {
        let (timed_notes, _) = NoteTimer::get_notes(part, time_signature_changes);
        let mut builder = Self::new(timed_notes);
        builder.init();

        DynamicHelper {
            crescendo_blocks: builder.crescendo_blocks,
            crescendo_index: 0,
            dynamic: Dynamic::default(),
            config,
            bars_with_accents: builder.bars_with_accents,
        }
    }
}

impl DynamicHelper {
    /// Maps textual dynamics to a linear MIDI-volume scale.
    fn dynamic_to_fraction(dynamic: Dynamic) -> Fraction {
        match dynamic {
            Dynamic::PPP => 0,
            Dynamic::PP => 16,
            Dynamic::P => 32,
            Dynamic::MP => 48,
            Dynamic::MF => 64,
            Dynamic::F => 80,
            Dynamic::FF => 96,
            Dynamic::FFF => 127,
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

    /// Updates the current dynamic state from a rest event.
    pub fn next_rest(&mut self, rest: &Rest) {
        if let Some(dynamic) = rest.dynamic {
            self.dynamic = dynamic;
        }
    }

    /// Returns the volume for the next note at `timing.note_stamp`.
    pub fn next_note(&mut self, note: &ast::Note, timing: &NoteTimingInfo) -> MidiVolume {
        let base_volume = self.base_volume(note, timing);

        // Additive boosts — clamped to MAX_VALUE.
        let mut boost: u16 = 0;
        if note.articulation.contains(Articulation::Accent) {
            boost += u16::from(self.config.accent_boost);
        }

        if !self.bars_with_accents.contains(&timing.bar_number) {
            boost += match beat_stress(timing.position_in_bar, timing.time_signature) {
                BeatStress::Primary => u16::from(self.config.primary_beat_boost),
                BeatStress::Secondary => u16::from(self.config.secondary_beat_boost),
                BeatStress::Regular => 0,
            };
        }

        let boosted = (u16::from(base_volume.volume) + boost).min(u16::from(MidiVolume::MAX_VALUE));
        MidiVolume::new(boosted as u8)
    }

    /// Resolves the base volume from dynamic markings and crescendo interpolation,
    /// without applying articulation or beat-stress boosts.
    fn base_volume(&mut self, note: &ast::Note, timing: &NoteTimingInfo) -> MidiVolume {
        if let Some(dynamic) = note.dynamic {
            self.dynamic = dynamic;
            return Self::dynamic_to_volume(dynamic);
        }

        while self.crescendo_index < self.crescendo_blocks.len() {
            let block = &self.crescendo_blocks[self.crescendo_index];
            if timing.note_stamp >= block.end.time {
                if note.dynamic.is_none() {
                    self.dynamic = block.end.dynamic;
                }

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
