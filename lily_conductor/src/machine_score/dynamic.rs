use fraction::{ConstOne, ConstZero, Ratio, Zero};
use lilyparse::syntax::ast::{self, Articulation, Crescendo, Dynamic, Event, LilyPart, Rest, TimeSignature};

use crate::machine_score::{
    MidiVolume,
    event_timer::{EventTimer, Fraction, Notes, TimedEvent},
    note_timer::NoteTimingInfo,
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

        let (dynamic, crescendo) = match event {
            Event::Note(note) => (note.dynamic, note.crescendo),
            Event::Rest(rest) => (rest.dynamic, rest.crescendo),
            _ => return,
        };

        if let Some(dynamic) = dynamic {
            self.end_block(dynamic);
        } else if crescendo.is_some() {
            let dynamic = Self::infer_crescendo_end_dynamic(self);
            self.end_block(dynamic);
        }
    }

    /// Starts a block when no block is currently active.
    fn check_for_crescendo_start(&mut self, event: &Event) {
        if self.crescendo_kind != CrescendoKind::None {
            return;
        }

        let crescendo = match event {
            Event::Note(note) => note.crescendo,
            Event::Rest(rest) => rest.crescendo,
            _ => None,
        };

        if let Some(crescendo) = crescendo
            && crescendo != Crescendo::End
        {
            self.start_block(crescendo);
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
        let TimedEvent {
            event,
            note_stamp,
            duration: _,
            x_note: _,
        } = timed_event;

        match event {
            Event::Note(note) => {
                self.note_stamp = *note_stamp;
                if let Some(dynamic) = note.dynamic {
                    self.dynamic = dynamic;
                }
            }
            Event::Rest(rest) => {
                self.note_stamp = *note_stamp;
                if let Some(dynamic) = rest.dynamic {
                    self.dynamic = dynamic;
                }
            }
            _ => {}
        }
    }

    /// Scans the whole part and captures crescendo blocks.
    fn init(&mut self) {
        let timed_events = EventTimer::get_timed_events(self.part);

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
        Self::build_with_config(part, DynamicConfig::default())
    }

    /// Builds a dynamic helper from one LilyPond part with a custom config.
    pub fn build_with_config(part: &'a LilyPart, config: DynamicConfig) -> DynamicHelper {
        let mut builder = Self::new(part);
        builder.init();

        DynamicHelper {
            crescendo_blocks: builder.crescendo_blocks,
            crescendo_index: 0,
            dynamic: Dynamic::default(),
            config,
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
        boost += match beat_stress(timing.position_in_bar, timing.time_signature) {
            BeatStress::Primary => u16::from(self.config.primary_beat_boost),
            BeatStress::Secondary => u16::from(self.config.secondary_beat_boost),
            BeatStress::Regular => 0,
        };

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

#[cfg(test)]
mod tests {
    use super::*;
    use lilyparse::syntax::ast::{Articulation, Note, NoteDuration, NoteOrRest, TimeSignature};

    fn no_boost() -> DynamicConfig {
        DynamicConfig {
            accent_boost: 0,
            primary_beat_boost: 0,
            secondary_beat_boost: 0,
        }
    }

    fn note(duration: Option<NoteDuration>, dynamic: Option<Dynamic>, crescendo: Option<Crescendo>) -> Note {
        let mut ret = Note::default();
        ret.duration = duration;
        ret.dynamic = dynamic;
        ret.crescendo = crescendo;
        ret
    }

    fn note_with_articulation(
        duration: Option<NoteDuration>,
        dynamic: Option<Dynamic>,
        articulation: Articulation,
    ) -> Note {
        let mut ret = Note::default();
        ret.duration = duration;
        ret.dynamic = dynamic;
        ret.articulation = articulation;
        ret
    }

    fn rest(duration: Option<NoteDuration>, dynamic: Option<Dynamic>, crescendo: Option<Crescendo>) -> Rest {
        let mut ret = Rest::default();
        ret.duration = duration;
        ret.dynamic = dynamic;
        ret.crescendo = crescendo;
        ret
    }

    fn part(events: Vec<Event>) -> LilyPart {
        LilyPart {
            name: "string".to_owned(),
            events,
        }
    }

    fn quarter() -> NoteDuration {
        NoteDuration {
            ratio: 4,
            augmentation: 0,
        }
    }

    fn timing_at(position: Fraction, ts: TimeSignature) -> NoteTimingInfo {
        NoteTimingInfo {
            note_or_rest: NoteOrRest::Note(note(None, None, None)),
            time_signature: ts,
            bar_number: 0,
            position_in_bar: position,
            note_stamp: position,
            length: Fraction::new(1u32, 4u32),
            x_note: false,
        }
    }

    // --- Pre-existing tests (use zero boosts to isolate crescendo logic) ----

    #[test]
    fn interpolates_volume_inside_crescendo_block() {
        let part = part(vec![
            Event::Note(note(
                Some(quarter()),
                Some(Dynamic::MF),
                Some(Crescendo::CrescendoStart),
            )),
            Event::Note(note(Some(quarter()), None, None)),
            Event::Note(note(Some(quarter()), None, Some(Crescendo::End))),
        ]);
        let timed_events = EventTimer::get_timed_events(&part);
        let mut helper = DynamicBuilder::build_with_config(&part, no_boost());

        let ts = TimeSignature::default();
        let first = match &timed_events[0].event {
            Event::Note(note) => helper.next_note(note, &timing_at(timed_events[0].note_stamp, ts)),
            _ => unreachable!(),
        };
        let second = match &timed_events[1].event {
            Event::Note(note) => helper.next_note(note, &timing_at(timed_events[1].note_stamp, ts)),
            _ => unreachable!(),
        };
        let third = match &timed_events[2].event {
            Event::Note(note) => helper.next_note(note, &timing_at(timed_events[2].note_stamp, ts)),
            _ => unreachable!(),
        };

        assert_eq!(first.volume, 80);
        assert_eq!(second.volume, 88);
        assert_eq!(third.volume, 96);
    }

    #[test]
    fn rest_dynamic_updates_following_note_volume() {
        let part = part(vec![
            Event::Note(note(Some(quarter()), Some(Dynamic::MF), None)),
            Event::Rest(rest(Some(quarter()), Some(Dynamic::P), None)),
            Event::Note(note(Some(quarter()), None, None)),
        ]);
        let timed_events = EventTimer::get_timed_events(&part);
        let mut helper = DynamicBuilder::build_with_config(&part, no_boost());

        let ts = TimeSignature::default();
        let first_note = match &timed_events[0].event {
            Event::Note(note) => note,
            _ => unreachable!(),
        };
        assert_eq!(
            helper
                .next_note(first_note, &timing_at(timed_events[0].note_stamp, ts))
                .volume,
            80
        );

        let rest_ev = match &timed_events[1].event {
            Event::Rest(rest) => rest,
            _ => unreachable!(),
        };
        helper.next_rest(rest_ev);

        let second_note = match &timed_events[2].event {
            Event::Note(note) => note,
            _ => unreachable!(),
        };
        assert_eq!(
            helper
                .next_note(second_note, &timing_at(timed_events[2].note_stamp, ts))
                .volume,
            48
        );
    }

    // --- Beat stress classification ------------------------------------------

    #[test]
    fn beat_stress_4_4() {
        let ts = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        assert_eq!(beat_stress(Fraction::new(0u32, 1u32), ts), BeatStress::Primary); // beat 1
        assert_eq!(beat_stress(Fraction::new(1u32, 4u32), ts), BeatStress::Regular); // beat 2
        assert_eq!(beat_stress(Fraction::new(2u32, 4u32), ts), BeatStress::Secondary); // beat 3
        assert_eq!(beat_stress(Fraction::new(3u32, 4u32), ts), BeatStress::Regular); // beat 4
    }

    #[test]
    fn beat_stress_3_4() {
        let ts = TimeSignature {
            numerator: 3,
            denominator: 4,
        };
        assert_eq!(beat_stress(Fraction::new(0u32, 1u32), ts), BeatStress::Primary);
        assert_eq!(beat_stress(Fraction::new(1u32, 4u32), ts), BeatStress::Regular);
        assert_eq!(beat_stress(Fraction::new(2u32, 4u32), ts), BeatStress::Regular);
    }

    #[test]
    fn beat_stress_6_8() {
        let ts = TimeSignature {
            numerator: 6,
            denominator: 8,
        };
        assert_eq!(beat_stress(Fraction::new(0u32, 1u32), ts), BeatStress::Primary); // beat 1
        assert_eq!(beat_stress(Fraction::new(1u32, 8u32), ts), BeatStress::Regular); // beat 2
        assert_eq!(beat_stress(Fraction::new(3u32, 8u32), ts), BeatStress::Secondary); // beat 4
    }

    // --- Accent boost --------------------------------------------------------

    #[test]
    fn accent_boost_applied() {
        let part = part(vec![]);
        let config = DynamicConfig {
            accent_boost: 20,
            primary_beat_boost: 0,
            secondary_beat_boost: 0,
        };
        let mut helper = DynamicBuilder::build_with_config(&part, config);

        let plain = note_with_articulation(Some(quarter()), Some(Dynamic::MF), Articulation::Portato);
        let accented = note_with_articulation(Some(quarter()), Some(Dynamic::MF), Articulation::Accent);
        let ts = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        // Use beat 1 (position 1/4) so no beat boost interferes
        let timing = timing_at(Fraction::new(1u32, 4u32), ts);

        let plain_vol = helper.next_note(&plain, &timing).volume;
        // reset helper state
        let mut helper2 = DynamicBuilder::build_with_config(&part, config);
        let accented_vol = helper2.next_note(&accented, &timing).volume;

        assert_eq!(accented_vol, plain_vol + 20);
    }

    #[test]
    fn beat_stress_boost_applied() {
        let part = part(vec![]);
        let config = DynamicConfig {
            accent_boost: 0,
            primary_beat_boost: 12,
            secondary_beat_boost: 6,
        };
        let mut helper = DynamicBuilder::build_with_config(&part, config);

        let n = note_with_articulation(Some(quarter()), Some(Dynamic::MF), Articulation::Portato);
        let ts = TimeSignature {
            numerator: 4,
            denominator: 4,
        };

        let base = helper.next_note(&n, &timing_at(Fraction::new(1u32, 4u32), ts)).volume; // beat 2 (regular)
        let primary = helper.next_note(&n, &timing_at(Fraction::new(0u32, 1u32), ts)).volume; // beat 1
        let secondary = helper.next_note(&n, &timing_at(Fraction::new(2u32, 4u32), ts)).volume; // beat 3

        assert_eq!(primary, base + 12);
        assert_eq!(secondary, base + 6);
    }

    #[test]
    fn boosts_clamped_to_max_volume() {
        let part = part(vec![]);
        let config = DynamicConfig {
            accent_boost: 100,
            primary_beat_boost: 100,
            secondary_beat_boost: 0,
        };
        let mut helper = DynamicBuilder::build_with_config(&part, config);

        let n = note_with_articulation(Some(quarter()), Some(Dynamic::FFF), Articulation::Accent);
        let ts = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        // Primary beat + accent at FFF — total boost would exceed 127.
        let vol = helper.next_note(&n, &timing_at(Fraction::new(0u32, 1u32), ts)).volume;
        assert_eq!(vol, MidiVolume::MAX_VALUE);
    }
}
