//! The playback engine: converts normalized MIDI events into scheduled
//! protocol commands, tracking per-string state (current expression, sounding
//! note, held/dampened frets, pending unfrets).

use std::time::{Duration, Instant};

use enum_iterator::{all, cardinality};
use midly::num::u7;
use protocol::{Fret, GuitarString, Message, PluckTechnique};
use string_volume::{MidiVolume, StringVolumeTable};

use crate::midi_input::EngineEvent;
use crate::scheduler::CommandSink;

/// MIDI pitch value in range `0..=127`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiPitch {
    pitch: u7,
}

impl MidiPitch {
    /// Creates a validated MIDI pitch value.
    pub fn new(pitch: u8) -> Self {
        let value = u7::try_from(pitch).expect("Invalid pitch value");
        Self { pitch: value }
    }

    pub fn as_u8(&self) -> u8 {
        self.pitch.as_int()
    }
}

pub fn get_base_pitch(string: GuitarString) -> MidiPitch {
    match string {
        GuitarString::E => MidiPitch::new(40),
        GuitarString::A => MidiPitch::new(45),
        GuitarString::D => MidiPitch::new(50),
        GuitarString::G => MidiPitch::new(55),
        GuitarString::B => MidiPitch::new(59),
        GuitarString::e => MidiPitch::new(64),
    }
}

/// 0-based MIDI channel of the expression control staff.
pub const EXPRESSION_CHANNEL: u8 = 6;

/// Highest controllable fret (calibration tables only cover frets 0..=12).
pub const MAX_FRET: u8 = 12;

// ---------------------------------------------------------------------------
// Timing constants (milliseconds, relative to the pluck/slam time)
// ---------------------------------------------------------------------------

/// Lead time for `FretQuiet` before a pluck.
const FRET_QUIET_PREP_MS: i64 = 60;
/// Lead time for `FretFast` before a finger slam.
const FRET_FAST_PREP_MS: i64 = 20;
/// Lead time for `PluckVolume` (servo settling). If the previous pluck was
/// closer than this, the command is placed at the midpoint instead.
const PLUCK_VOLUME_PREP_MS: i64 = 200;
/// Lead time for `PluckTechnique` switches.
const PLUCK_SWITCH_MS: i64 = 30;
/// Lead time for emergency `UnfretFast` (releasing a higher fret).
const EMERGENCY_UNFRET_MS: i64 = 10;
/// Settle time between `Dampen` and the quiet `Unfret` of a released note.
const DAMPEN_SETTLE_MS: i64 = 500;

/// Fret used to dampen an open string at note-off (lily_conductor convention).
const OPEN_STRING_DAMPEN_FRET: Fret = Fret::Fret11;
/// Fret used to dampen for hard+dampen plucks of the open string.
const HARD_DAMPEN_OPEN_FRET: Fret = Fret::Fret12;

/// Per-string playing expression, controlled via the expression staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Expression {
    /// Pluck with the soft side of the pick.
    #[default]
    Soft,
    /// Pluck with the hard side of the pick.
    Hard,
    /// Don't pluck; slam the finger with force (`FretFast`).
    FingerSlam,
    /// Pluck hard while only dampening (not fretting) the note: percussive.
    HardDampen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FretState {
    Idle,
    Holding,
    Damping,
    Releasing(Instant),
}

#[derive(Debug, Copy, Clone)]
struct FretStates {
    fret_states: [FretState; MAX_FRET as usize + 1],
}

impl FretStates {
    pub fn new() -> Self {
        Self {
            fret_states: [FretState::Idle; _],
        }
    }

    pub fn frets(&self) -> impl Iterator<Item = (Fret, FretState)> {
        self.fret_states
            .iter()
            .enumerate()
            .map(|(i, state)| (fret_from_number(i as u8), *state))
    }

    pub fn index_to_fret(&self, index: u8) -> Option<Fret> {
        if index as usize >= self.fret_states.len() {
            None
        } else {
            Some(fret_from_number(index))
        }
    }

    pub fn get_state_mut(&mut self, fret: Fret) -> &mut FretState {
        self.fret_states.get_mut(fret as usize).expect("Fret should exist")
    }

    pub fn set_state(&mut self, fret: Fret, state: FretState) {
        *self.get_state_mut(fret) = state;
    }

    pub fn frets_mut(&mut self) -> impl Iterator<Item = (Fret, &mut FretState)> {
        self.fret_states
            .iter_mut()
            .enumerate()
            .map(|(i, state)| (fret_from_number(i as u8), state))
    }
}

#[derive(Debug, Clone)]
struct StringState {
    expression: Expression,
    fret_states: FretStates,
    last_technique: PluckTechnique,
    last_volume: Option<u8>,
    last_pluck: Option<Instant>,
    string: GuitarString,
}

impl StringState {
    fn get_fret_by_pitch(&self, pitch: u8) -> Option<Fret> {
        let base = get_base_pitch(self.string).as_u8();
        if pitch < base {
            return None;
        }

        let index = pitch - base;
        self.fret_states.index_to_fret(index)
    }

    pub fn new(string: GuitarString) -> Self {
        Self {
            expression: Expression::default(),
            fret_states: FretStates::new(),
            last_technique: PluckTechnique::Soft,
            last_volume: None,
            last_pluck: None,
            string,
        }
    }
}

struct StringStates {
    states: [StringState; cardinality::<GuitarString>()],
}

impl StringStates {
    pub fn get_state_mut(&mut self, string: GuitarString) -> &mut StringState {
        self.states.get_mut(string as usize).expect("Invalid string")
    }

    pub fn get_state(&self, string: GuitarString) -> &StringState {
        self.states.get(string as usize).expect("Invalid string")
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut StringState> {
        self.states.iter_mut()
    }

    pub fn new() -> Self {
        Self {
            states: [
                StringState::new(GuitarString::E),
                StringState::new(GuitarString::A),
                StringState::new(GuitarString::D),
                StringState::new(GuitarString::G),
                StringState::new(GuitarString::B),
                StringState::new(GuitarString::e),
            ],
        }
    }
}

/// The real-time playback engine.
pub struct GuitarEngine {
    states: StringStates,
    volume_table: StringVolumeTable,
    latency: Duration,
}

impl GuitarEngine {
    pub fn new(volume_table: StringVolumeTable, latency_ms: u64) -> Self {
        Self {
            states: StringStates::new(),
            volume_table,
            latency: Duration::from_millis(latency_ms),
        }
    }

    /// Handles one MIDI event that occurred at `event_time`.
    pub fn handle(&mut self, event_time: Instant, event: EngineEvent, sink: &mut dyn CommandSink) {
        let event_time = event_time + self.latency;
        self.handle_pending_unfret(event_time);

        match event {
            EngineEvent::NoteOn { channel, key, velocity } => self.note_on(event_time, channel, key, velocity, sink),
            EngineEvent::NoteOff { channel, key } => self.note_off(event_time, channel, key, sink),
            EngineEvent::AllNotesOff { channel } => self.all_notes_off(event_time, channel, sink),
        }
    }

    fn handle_pending_unfret(&mut self, event_time: Instant) {
        for state in self.states.iter_mut() {
            for (_, state) in state.fret_states.frets_mut() {
                if let FretState::Releasing(time) = state
                    && *time < event_time
                {
                    *state = FretState::Idle;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expression staff
    // -----------------------------------------------------------------------

    /// Expression control notes: pitch = open-string base + offset, where
    /// 0 = soft, 1 = hard, 2 = finger slam, 3 = hard+dampen. Durations are
    /// ignored; the expression persists until the next control note.
    fn expression_change(&mut self, key: u8) {
        for string in all::<GuitarString>() {
            let base = get_base_pitch(string).as_u8();
            if key >= base && key - base <= 3 {
                let expression = match key - base {
                    0 => Expression::Soft,
                    1 => Expression::Hard,
                    2 => Expression::FingerSlam,
                    _ => Expression::HardDampen,
                };

                self.states.get_state_mut(string).expression = expression;
                eprintln!("Expression {string:?} -> {expression:?}");
                return;
            }
        }
        eprintln!("Ignoring out-of-range expression note (key {key}).");
    }

    // -----------------------------------------------------------------------
    // Note on
    // -----------------------------------------------------------------------

    fn note_on(&mut self, event_time: Instant, channel: u8, key: u8, velocity: u8, sink: &mut dyn CommandSink) {
        if channel == EXPRESSION_CHANNEL {
            self.expression_change(key);
            return;
        }

        let Some(string) = channel_to_string(channel) else {
            eprintln!("Ignoring note on unknown channel {}", channel + 1);
            return;
        };

        let string_state = self.states.get_state_mut(string);
        let Some(fret) = string_state.get_fret_by_pitch(key) else {
            eprintln!("Ignoring note: key {key} out of range for string {string:?}");
            return;
        };

        // --- Release held frets above the new fret ----------------------------
        // A pressed (or dampened) higher fret would mask the lower note.
        for (fret, state) in string_state.fret_states.frets_mut().skip(fret as usize + 1) {
            if *state != FretState::Idle {
                sink.schedule(
                    shifted(event_time, -EMERGENCY_UNFRET_MS),
                    Message::UnfretFast(string, fret),
                );
                *state = FretState::Idle;
            }
        }

        // --- Expression-specific handling -------------------------------------
        // Any still-sounding note on this string is implicitly ended by the
        // release loop above (same/higher old frets may stay pressed).
        match string_state.expression {
            Expression::Soft => self.plucked_note(string, fret, velocity, PluckTechnique::Soft, event_time, sink),
            Expression::Hard => self.plucked_note(string, fret, velocity, PluckTechnique::Hard, event_time, sink),
            Expression::FingerSlam => self.finger_slam(string, fret, event_time, sink),
            Expression::HardDampen => self.hard_dampen(string, fret, velocity, event_time, sink),
        }
    }

    /// Soft/hard pluck of a fretted note.
    fn plucked_note(
        &mut self,
        string: GuitarString,
        fret: Fret,
        velocity: u8,
        technique: PluckTechnique,
        t: Instant,
        sink: &mut dyn CommandSink,
    ) {
        let volume = self
            .volume_table
            .map_midi_volume_string(string, fret, technique, MidiVolume::new(velocity));

        self.schedule_technique(string, technique, t, sink);
        self.schedule_volume(string, volume, t, sink);

        if fret != Fret::NoFret {
            self.schedule_fret(string, fret, t, sink);
            self.schedule_nut_dampen(string, fret, t, sink);
        }

        self.schedule_pluck(string, t, sink);
    }

    fn schedule_nut_dampen(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        if fret != Fret::Fret1 {
            self.schedule_pre_dampen(string, Fret::Fret1, t, sink);
        }
    }

    fn schedule_pre_dampen(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        self.schedule_dampen(string, fret, shifted(t, -FRET_QUIET_PREP_MS), sink);
    }

    fn schedule_dampen(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let fret = if fret == Fret::NoFret { Fret::Fret11 } else { fret };

        let st = &mut self.states.get_state_mut(string);
        let fret_state = st.fret_states.get_state_mut(fret);
        if *fret_state != FretState::Damping {
            sink.schedule(t, Message::Dampen(string, fret));
            *fret_state = FretState::Damping;
        }
    }

    fn schedule_fret(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let st = &mut self.states.get_state_mut(string);
        let fret_state = st.fret_states.get_state_mut(fret);
        if *fret_state != FretState::Holding {
            sink.schedule(shifted(t, -FRET_QUIET_PREP_MS), Message::FretQuiet(string, fret));
            *fret_state = FretState::Holding;
        }
    }

    fn schedule_pluck(&mut self, string: GuitarString, t: Instant, sink: &mut dyn CommandSink) {
        let st = &mut self.states.get_state_mut(string);
        sink.schedule(t, Message::Pluck(string));
        st.last_pluck = Some(t);
    }

    /// Finger slam: no pluck, just a hard fretting action.
    fn finger_slam(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        if fret == Fret::NoFret {
            eprintln!("Ignoring finger slam on open string {string:?}.");
            return;
        }

        self.schedule_nut_dampen(string, fret, t, sink);
        let st = &mut self.states.get_state_mut(string);

        let fret_state = st.fret_states.get_state_mut(fret);
        if *fret_state != FretState::Idle {
            sink.schedule(shifted(t, -2 * FRET_FAST_PREP_MS), Message::UnfretFast(string, fret));
        }

        sink.schedule(shifted(t, -FRET_FAST_PREP_MS), Message::FretFast(string, fret));
        *fret_state = FretState::Holding;
    }

    /// Hard pluck while only dampening the note position: percussive sound.
    fn hard_dampen(&mut self, string: GuitarString, fret: Fret, velocity: u8, t: Instant, sink: &mut dyn CommandSink) {
        let damp_fret = if fret == Fret::NoFret {
            HARD_DAMPEN_OPEN_FRET
        } else {
            fret
        };

        let volume =
            self.volume_table
                .map_midi_volume_string(string, fret, PluckTechnique::Hard, MidiVolume::new(velocity));

        self.schedule_technique(string, PluckTechnique::Hard, t, sink);
        self.schedule_volume(string, volume, t, sink);
        self.schedule_pre_dampen(string, damp_fret, t, sink);
        self.schedule_pluck(string, t, sink);
    }

    fn schedule_technique(
        &mut self,
        string: GuitarString,
        technique: PluckTechnique,
        t: Instant,
        sink: &mut dyn CommandSink,
    ) {
        let st = self.states.get_state_mut(string);
        if st.last_technique != technique {
            sink.schedule(shifted(t, -PLUCK_SWITCH_MS), Message::PluckTechnique(string, technique));
            st.last_technique = technique;
        }
    }

    fn schedule_volume(&mut self, string: GuitarString, volume: u8, t: Instant, sink: &mut dyn CommandSink) {
        let st = self.states.get_state_mut(string);
        if st.last_volume == Some(volume) {
            return;
        }
        // Ideally PLUCK_VOLUME_PREP_MS before the pluck; if the previous pluck
        // was too recent for that, place the command midway between the two.
        let ideal = shifted(t, -PLUCK_VOLUME_PREP_MS);
        let at = match st.last_pluck {
            Some(prev) if ideal < prev + Duration::from_millis(PLUCK_VOLUME_PREP_MS as u64 / 2) => {
                prev + (t - prev) / 2
            }
            _ => ideal,
        };
        sink.schedule(at, Message::PluckVolume(string, volume.into()));
        st.last_volume = Some(volume);
    }

    // -----------------------------------------------------------------------
    // Note off
    // -----------------------------------------------------------------------

    fn note_off(&mut self, event_time: Instant, channel: u8, key: u8, sink: &mut dyn CommandSink) {
        if channel == EXPRESSION_CHANNEL {
            return; // Expression durations are ignored.
        }

        let Some(string) = channel_to_string(channel) else {
            eprintln!("Ignoring note-off on unknown channel {}", channel + 1);
            return;
        };

        let string_state = self.states.get_state_mut(string);
        let Some(_) = string_state.get_fret_by_pitch(key) else {
            eprintln!("Ignoring note: key {key} out of range for string {string:?}");
            return;
        };

        // As each string is monophonic, we can treat note_off in the same way as all_notes_off
        self.stop_sounding_all(string, event_time, sink);
    }

    /// CC 123/120 "all notes off": stops whatever is sounding on the string,
    /// regardless of key (MuseScore sends this per channel on pause/stop).
    fn all_notes_off(&mut self, event_time: Instant, channel: u8, sink: &mut dyn CommandSink) {
        if channel == EXPRESSION_CHANNEL {
            return; // Expression durations are ignored.
        }

        let Some(string) = channel_to_string(channel) else {
            eprintln!("Ignoring note_off on unknown channel {}", channel + 1);
            return;
        };

        self.stop_sounding_all(string, event_time, sink);
    }

    fn stop_sounding_all(&mut self, string: GuitarString, event_time: Instant, sink: &mut dyn CommandSink) {
        let st = self.states.get_state(string);
        let mut no_active_frets = 0;
        for (fret, fret_state) in st.fret_states.clone().frets() {
            match fret_state {
                FretState::Idle | FretState::Releasing(_) => {} // nothing to do,
                FretState::Holding => {
                    self.schedule_mute_sequence(string, fret, event_time, sink);
                    no_active_frets += 1;
                }
                FretState::Damping => {
                    self.schedule_unfret(string, fret, event_time, sink);
                    no_active_frets += 1;
                }
            }
        }

        // if no frets were active, we had an open string -> mute it
        if no_active_frets == 0 {
            self.schedule_mute_sequence(string, Fret::NoFret, event_time, sink);
        }
    }

    fn schedule_mute_sequence(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let damp_fret = if fret == Fret::NoFret {
            OPEN_STRING_DAMPEN_FRET
        } else {
            fret
        };

        self.schedule_dampen(string, damp_fret, t, sink);
        self.schedule_unfret(string, damp_fret, shifted(t, DAMPEN_SETTLE_MS), sink);
    }

    /// Schedules the quiet `Unfret` of a dampened fret after the settle time.
    fn schedule_unfret(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let st = &mut self.states.get_state_mut(string);
        sink.schedule(t, Message::Unfret(string, fret));
        st.fret_states.set_state(fret, FretState::Releasing(t));
    }
}

/// Maps a 0-based MIDI channel to a string index, or `None` for non-string
/// channels.
fn channel_to_string(channel: u8) -> Option<GuitarString> {
    match channel {
        0 => Some(GuitarString::e),
        1 => Some(GuitarString::B),
        2 => Some(GuitarString::G),
        3 => Some(GuitarString::D),
        4 => Some(GuitarString::A),
        5 => Some(GuitarString::E),
        _ => None,
    }
}

fn fret_from_number(n: u8) -> Fret {
    match n {
        0 => Fret::NoFret,
        1 => Fret::Fret1,
        2 => Fret::Fret2,
        3 => Fret::Fret3,
        4 => Fret::Fret4,
        5 => Fret::Fret5,
        6 => Fret::Fret6,
        7 => Fret::Fret7,
        8 => Fret::Fret8,
        9 => Fret::Fret9,
        10 => Fret::Fret10,
        11 => Fret::Fret11,
        12 => Fret::Fret12,
        13 => Fret::Fret13,
        14 => Fret::Fret14,
        15 => Fret::Fret15,
        16 => Fret::Fret16,
        17 => Fret::Fret17,
        18 => Fret::Fret18,
        _ => panic!("fret number {n} out of range"),
    }
}

/// `t` shifted by a signed millisecond offset, saturating at `t` for offsets
/// that would underflow.
fn shifted(t: Instant, offset_ms: i64) -> Instant {
    if offset_ms >= 0 {
        t + Duration::from_millis(offset_ms as u64)
    } else {
        t.checked_sub(Duration::from_millis(offset_ms.unsigned_abs()))
            .unwrap_or(t)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use string_volume::StringVolumeRange;

    const LATENCY_MS: u64 = 250;

    /// Records scheduled commands; `cancel` drops matching pending entries.
    #[derive(Default)]
    struct RecordingSink {
        entries: Vec<(u64, Message)>,
    }

    impl RecordingSink {
        fn record(&mut self, base: Instant, at: Instant, message: Message) {
            let ms = at.duration_since(base).as_millis() as u64;
            self.entries.push((ms, message));
        }

        /// Sorted (by time, stable) list of surviving messages.
        fn messages(&self) -> Vec<(u64, Message)> {
            let mut entries: Vec<_> = self.entries.iter().collect();
            entries.sort_by_key(|(ms, _)| *ms);
            entries.iter().map(|(ms, m)| (*ms, *m)).collect()
        }

        fn count(&self, message: Message) -> usize {
            self.entries.iter().filter(|(_, m)| *m == message).count()
        }
    }

    impl CommandSink for RecordingSink {
        fn schedule(&mut self, at: Instant, message: Message) {
            self.record(BASE.with(|b| *b.borrow()), at, message);
        }
    }

    thread_local! {
        static BASE: std::cell::RefCell<Instant> = std::cell::RefCell::new(Instant::now());
    }

    struct Fixture {
        engine: GuitarEngine,
        sink: RecordingSink,
        base: Instant,
    }

    impl Fixture {
        fn new() -> Self {
            let base = Instant::now();
            BASE.with(|b| *b.borrow_mut() = base);
            Self {
                engine: GuitarEngine::new(
                    StringVolumeTable::uniform(StringVolumeRange { min: 0, max: 255 }),
                    LATENCY_MS,
                ),
                sink: RecordingSink::default(),
                base,
            }
        }

        fn at(&self, ms: u64) -> Instant {
            self.base + Duration::from_millis(ms)
        }

        fn on(&mut self, ms: u64, channel: u8, key: u8, velocity: u8) {
            self.engine.handle(
                self.at(ms),
                EngineEvent::NoteOn { channel, key, velocity },
                &mut self.sink,
            );
        }

        fn off(&mut self, ms: u64, channel: u8, key: u8) {
            self.engine
                .handle(self.at(ms), EngineEvent::NoteOff { channel, key }, &mut self.sink);
        }

        fn messages(&self) -> Vec<(u64, Message)> {
            self.sink.messages()
        }
    }

    use GuitarString::e;

    #[test]
    fn simple_soft_note_on_and_off() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 5, 100); // high e, fret 5
        f.off(500, 0, 64 + 5);

        let msgs = f.messages();
        let t = LATENCY_MS;
        // Volume first (uniform 0..=255, velocity 100 -> 200), then fret +
        // fret1 dampen, then the pluck.
        assert_eq!(
            msgs,
            vec![
                (t - 200, Message::PluckVolume(e, 200.into())),
                (t - 60, Message::FretQuiet(e, Fret::Fret5)),
                (t - 60, Message::Dampen(e, Fret::Fret1)),
                (t, Message::Pluck(e)),
                (500 + t, Message::Dampen(e, Fret::Fret5)),
                (500 + t + 500, Message::Unfret(e, Fret::Fret5)),
            ]
        );
    }

    #[test]
    fn lower_note_after_higher_note_unfrets_fast() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 8, 100); // fret 8
        f.off(400, 0, 64 + 8); // dampen fret 8, quiet unfret pending at 400+250+500
        f.on(500, 0, 64 + 3, 100); // fret 3: must release fret 8 first

        let msgs = f.messages();
        let t2 = 500 + LATENCY_MS;
        assert!(msgs.contains(&(t2 - 10, Message::UnfretFast(e, Fret::Fret8))));
        assert!(!msgs.iter().any(|(_, m)| *m == Message::Unfret(e, Fret::Fret8)));
        // The fret command still comes before the pluck.
        assert!(msgs.contains(&(t2 - 60, Message::FretQuiet(e, Fret::Fret3))));
        assert!(msgs.contains(&(t2, Message::Pluck(e))));
    }

    #[test]
    fn rearticulated_dampened_fret_cancels_pending_unfret() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 5, 100);
        f.off(400, 0, 64 + 5); // dampen + pending unfret
        f.on(500, 0, 64 + 5, 100); // same fret again: cancel the unfret

        let msgs = f.messages();
        assert!(!msgs.iter().any(|(_, m)| *m == Message::Unfret(e, Fret::Fret5)));
        assert_eq!(f.sink.count(Message::FretQuiet(e, Fret::Fret5)), 2);
        // Same fret & velocity: volume is not re-sent.
        assert_eq!(f.sink.count(Message::PluckVolume(e, 200.into())), 1);
    }

    #[test]
    fn expression_staff_switches_technique_and_slam() {
        let mut f = Fixture::new();
        // Control chord: high e -> hard (base 64 + 1), low E -> slam (base 40 + 2).
        f.on(0, EXPRESSION_CHANNEL, 65, 100);
        f.on(0, EXPRESSION_CHANNEL, 42, 100);
        f.on(10, 0, 64 + 5, 100); // high e, fret 5
        f.on(10, 5, 40 + 2, 100); // low E, fret 2

        let msgs = f.messages();
        let t = 10 + LATENCY_MS;
        assert!(msgs.contains(&(t - 30, Message::PluckTechnique(e, PluckTechnique::Hard))));
        assert!(msgs.contains(&(t, Message::Pluck(e))));
        // Slam: FretFast, no pluck on the low E string.
        assert!(msgs.contains(&(t - 20, Message::FretFast(GuitarString::E, Fret::Fret2))));
        assert!(!msgs.iter().any(|(_, m)| *m == Message::Pluck(GuitarString::E)));
    }

    #[test]
    fn hard_dampen_plucks_dampened_fret_and_releases_without_dampen() {
        let mut f = Fixture::new();
        f.on(0, EXPRESSION_CHANNEL, 64 + 3, 100); // high e -> hard+dampen
        f.on(10, 0, 64 + 5, 100);
        f.off(500, 0, 64 + 5);

        let msgs = f.messages();
        let t = 10 + LATENCY_MS;
        assert!(msgs.contains(&(t - 60, Message::Dampen(e, Fret::Fret5))));
        assert!(msgs.contains(&(t - 30, Message::PluckTechnique(e, PluckTechnique::Hard))));
        assert!(msgs.contains(&(t, Message::Pluck(e))));
        // Exactly one dampen (at note-on), release at note-off + settle.
        assert_eq!(f.sink.count(Message::Dampen(e, Fret::Fret5)), 1);
        assert!(msgs.contains(&(500 + LATENCY_MS + 500, Message::Unfret(e, Fret::Fret5))));
    }

    #[test]
    fn fret1_dampened_while_fretting_and_released_for_open_string() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 5, 100); // fret 5 -> dampens fret 1
        f.off(400, 0, 64 + 5);
        f.on(500, 0, 64 + 7, 100); // fret 7 -> fret 1 stays damped, no new dampen
        f.off(900, 0, 64 + 7);
        f.on(1000, 0, 64, 100); // open string -> release fret 1

        let msgs = f.messages();
        assert_eq!(f.sink.count(Message::Dampen(e, Fret::Fret1)), 1);
        let t_open = 1000 + LATENCY_MS;
        assert!(msgs.contains(&(t_open - 10, Message::UnfretFast(e, Fret::Fret1))));
        assert!(msgs.contains(&(t_open, Message::Pluck(e))));
    }

    #[test]
    fn open_string_note_off_dampens_fret11() {
        let mut f = Fixture::new();
        f.on(0, 0, 64, 100); // open high e
        f.off(500, 0, 64);

        let msgs = f.messages();
        let t_off = 500 + LATENCY_MS;
        assert!(msgs.contains(&(t_off, Message::Dampen(e, Fret::Fret11))));
        assert!(msgs.contains(&(t_off + 500, Message::Unfret(e, Fret::Fret11))));
    }

    #[test]
    fn expired_pending_unfret_is_not_released_twice() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 8, 100); // fret 8
        f.off(100, 0, 64 + 8); // quiet unfret fires at 100+250+500 = 850
        f.on(1000, 0, 64 + 3, 100); // fret 3 at T=1250: unfret long done

        let msgs = f.messages();
        // The quiet unfret survives (it completes before the new pluck) and no
        // emergency unfret is issued for fret 8.
        assert!(msgs.contains(&(850, Message::Unfret(e, Fret::Fret8))));
        assert!(!msgs.iter().any(|(_, m)| *m == Message::UnfretFast(e, Fret::Fret8)));
    }

    #[test]
    fn out_of_range_note_is_ignored() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 13, 100); // fret 13 > MAX_FRET
        assert!(f.messages().is_empty());
    }

    #[test]
    fn all_notes_off_stops_sounding_note() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 5, 100); // high e, fret 5
        f.engine
            .handle(f.at(500), EngineEvent::AllNotesOff { channel: 0 }, &mut f.sink);
        // A second AllNotesOff (nothing sounding anymore) is a no-op.
        f.engine
            .handle(f.at(600), EngineEvent::AllNotesOff { channel: 0 }, &mut f.sink);

        let msgs = f.messages();
        let t_off = 500 + LATENCY_MS;
        assert!(msgs.contains(&(t_off, Message::Dampen(e, Fret::Fret5))));
        assert!(msgs.contains(&(t_off + 500, Message::Unfret(e, Fret::Fret5))));
        assert_eq!(f.sink.count(Message::Dampen(e, Fret::Fret5)), 1);
        assert_eq!(f.sink.count(Message::Unfret(e, Fret::Fret5)), 1);
    }
}
