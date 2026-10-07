//! The playback engine: converts normalized MIDI events into scheduled
//! protocol commands, tracking per-string state (current expression, sounding
//! note, held/dampened frets, pending unfrets).

use std::iter::repeat_n;
use std::time::{Duration, Instant};

use midly::num::u7;
use protocol::{Fret, GuitarString, Message, PluckTechnique};
use string_volume::{MidiVolume, StringVolumeTable, get_number_of_frets};

use crate::NUMBER_OF_CHANNELS;
use crate::config::MidiChannelMode;
use crate::midi_input::{EngineEvent, NoteOffData};
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

pub const META_BASE_NOTE: u8 = 52;
pub const META_NOTE: u8 = META_BASE_NOTE + 1;
pub const META_SLUR_START: u8 = META_NOTE + 1;
pub const META_SLUR_END: u8 = META_SLUR_START + 1;
pub const META_EXPRESSION_SOFT: u8 = META_SLUR_END + 1;
pub const META_EXPRESSION_HARD: u8 = META_EXPRESSION_SOFT + 1;
pub const META_EXPRESSION_SLAM: u8 = META_EXPRESSION_SOFT + 2;
pub const META_EXPRESSION_DAMP: u8 = META_EXPRESSION_SOFT + 3;

// ---------------------------------------------------------------------------
// Timing constants (milliseconds, relative to the pluck/slam time)
// ---------------------------------------------------------------------------

/// Lead time for `FretQuiet` before a pluck.
const FRET_QUIET_PREP_MS: i64 = 50;
const DAMPEN_PREP_MS: i64 = 20;
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
const UNFRET_QUIET_DURATION_MS: i64 = 550;

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
enum FretPosition {
    Idle,
    Holding,
    Damping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FretState {
    position: FretPosition,
    release_time: Option<Instant>,
}

#[derive(Debug, Clone)]
struct FretStates {
    fret_states: Vec<FretState>,
}

impl FretStates {
    pub fn new(string: GuitarString) -> Self {
        Self {
            fret_states: repeat_n(
                FretState {
                    position: FretPosition::Idle,
                    release_time: None,
                },
                get_number_of_frets(string) + 1,
            )
            .collect(),
        }
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
    slur_active: bool,
    string: GuitarString,
    playing_note: Option<u8>,
    note_priority: u8,
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

    fn can_play_note(&self, pitch: u8) -> bool {
        self.get_fret_by_pitch(pitch).is_some()
    }

    pub fn new(string: GuitarString) -> Self {
        Self {
            expression: Expression::default(),
            fret_states: FretStates::new(string),
            last_technique: PluckTechnique::Soft,
            last_volume: None,
            last_pluck: None,
            slur_active: false,
            string,
            playing_note: None,
            note_priority: 0,
        }
    }
}

struct StringStates {
    states: [StringState; NUMBER_OF_CHANNELS],
}

impl StringStates {
    pub fn get_state_mut(&mut self, string: GuitarString) -> &mut StringState {
        self.states.get_mut(string as usize).expect("Invalid string")
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut StringState> {
        self.states.iter_mut()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &StringState> {
        self.states.iter()
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
    midi_channel_mode: MidiChannelMode,
}

impl GuitarEngine {
    pub fn new(volume_table: StringVolumeTable, latency_ms: u64, midi_channel_mode: MidiChannelMode) -> Self {
        Self {
            states: StringStates::new(),
            volume_table,
            latency: Duration::from_millis(latency_ms),
            midi_channel_mode,
        }
    }

    fn handle_fixed(&mut self, event_time: Instant, mut events: Vec<EngineEvent>, sink: &mut dyn CommandSink) {
        let meta_events: Vec<_> = events
            .extract_if(.., |e| e.channel() as usize >= NUMBER_OF_CHANNELS)
            .filter_map(|e| match e {
                EngineEvent::NoteOn(note_on_data) => Some(note_on_data),
                _ => None,
            })
            .collect();

        for meta_event in meta_events.iter().filter(|e| e.key != META_SLUR_START) {
            self.meta_note_on(meta_event.channel, meta_event.key);
        }

        for off_event in events
            .iter()
            .filter(|e| matches!(e, EngineEvent::NoteOff { .. } | EngineEvent::AllNotesOff { .. }))
        {
            match off_event {
                EngineEvent::NoteOff(NoteOffData { channel, key }) => self.note_off(event_time, *channel, *key, sink),
                EngineEvent::AllNotesOff { .. } => self.all_notes_off(event_time, sink),
                _ => panic!("Only processing NoteOff events here"),
            }
        }

        for event in events.iter().filter_map(|e| match e {
            EngineEvent::NoteOn(note_on_data) => Some(note_on_data),
            _ => None,
        }) {
            self.note_on_string(
                event_time,
                channel_to_string(event.channel).expect("Meta events already handles"),
                event.key,
                event.velocity,
                sink,
            );
        }

        for meta_event in meta_events.iter().filter(|e| e.key == META_SLUR_START) {
            self.meta_note_on(meta_event.channel, meta_event.key);
        }
    }

    fn handle_auto(&mut self, event_time: Instant, mut events: Vec<EngineEvent>, sink: &mut dyn CommandSink) {
        // first note off events
        for off_event in events.extract_if(.., |e| {
            matches!(e, EngineEvent::NoteOff { .. } | EngineEvent::AllNotesOff { .. })
        }) {
            match off_event {
                EngineEvent::NoteOff(NoteOffData { channel, key }) => self.note_off(event_time, channel, key, sink),
                EngineEvent::AllNotesOff { .. } => self.all_notes_off(event_time, sink),
                _ => panic!("Only processing NoteOff events here"),
            }
        }

        // At this point, events only contains note-on events.
        let mut events: Vec<_> = events
            .iter()
            .map(|e| match e {
                EngineEvent::NoteOn(note_on_data) => *note_on_data,
                _ => panic!("Only NoteOn events should be left"),
            })
            .collect();

        // Sort by channel, then by key (higher notes first)
        events.sort_by_key(|e| (e.channel, 128 - e.key));

        let mut possible_mappings: Vec<_> = events
            .iter()
            .map(|event| {
                // strings that can play this note and are free
                let primary: Vec<_> = self
                    .states
                    .iter()
                    .rev()
                    .filter(|st| st.playing_note.is_none() && st.can_play_note(event.key))
                    .collect();

                // strings that can play this note and are busy, but would be eligible for interruption
                let secondary: Vec<_> = self
                    .states
                    .iter()
                    .rev()
                    .filter(|st| {
                        st.playing_note.is_some() && st.note_priority > event.channel && st.can_play_note(event.key)
                    })
                    .collect();

                (event, primary, secondary)
            })
            .collect();

        let mut mappings = Vec::new();

        while !possible_mappings.is_empty() {
            let result = if let Some(index) = possible_mappings
                .iter()
                .position(|(_, primary, secondary)| primary.is_empty() && secondary.len() == 1)
            {
                // can only play this note on one string where we need to interrupt another note
                let (event, _, secondary) = &possible_mappings[index];
                Some((index, **event, secondary.first().expect("We filtered").string))
            } else if let Some(index) = possible_mappings.iter().position(|(_, primary, _)| primary.len() == 1) {
                // can only play this note on one string
                let (event, primary, _) = &possible_mappings[index];
                Some((index, **event, primary.first().expect("We filtered").string))
            } else if let Some(index) = possible_mappings
                .iter()
                .position(|(_, primary, secondary)| primary.is_empty() && secondary.len() > 1)
            {
                // can play this note on multiple strings, need to interrupt in any case -> choose no open string
                let (event, _, secondary) = &possible_mappings[index];
                Some((
                    index,
                    **event,
                    secondary
                        .iter()
                        .find(|st| st.get_fret_by_pitch(event.key).expect("Already checked") != Fret::NoFret)
                        .expect("We filtered")
                        .string,
                ))
            } else if let Some(index) = possible_mappings.iter().position(|(_, primary, _)| primary.len() > 1) {
                // can play this note on multiple free strings -> choose no open string
                let (event, primary, _) = &possible_mappings[index];
                Some((
                    index,
                    **event,
                    primary
                        .iter()
                        .find(|st| st.get_fret_by_pitch(event.key).expect("Already checked") != Fret::NoFret)
                        .expect("We filtered")
                        .string,
                ))
            } else {
                None
            };

            if let Some((index, event, string)) = result {
                mappings.push((event, string));
                possible_mappings.remove(index);
            }

            // filter out notes that are not possible to map
            possible_mappings.retain_mut(|(event, primary, secondary)| {
                if primary.is_empty() && secondary.is_empty() {
                    eprintln!("Dropping event {event:?} (no string available)");
                    return false;
                }

                if let Some((_, _, string)) = result {
                    primary.retain(|st| st.string != string);
                    secondary.retain(|st| st.string != string);
                }

                true
            });
        }

        for (event, string) in mappings {
            let st = self.states.get_state_mut(string);

            let note_priority = st.note_priority;
            st.note_priority = event.channel;

            // interrupt previous note if necessary
            if let Some(playing_note) = st.playing_note {
                eprintln!(
                    "Interrupting note {playing_note} on string {string:?} (prio: {note_priority}) with {event:?}"
                );
                self.note_off(event_time, note_priority, playing_note, sink);
            }

            self.note_on_string(event_time, string, event.key, event.velocity, sink);
        }
    }

    pub fn handle(
        &mut self,
        event_time: Instant,
        events: impl Iterator<Item = EngineEvent>,
        sink: &mut dyn CommandSink,
    ) {
        let event_time = event_time + self.latency;
        self.handle_pending_unfret(event_time);
        let events: Vec<_> = events.collect();

        match self.midi_channel_mode {
            MidiChannelMode::MuseScorePlugin => self.handle_fixed(event_time, events, sink),
            MidiChannelMode::AutoNoMeta => self.handle_auto(event_time, events, sink),
        }
    }

    fn handle_pending_unfret(&mut self, event_time: Instant) {
        for state in self.states.iter_mut() {
            for (_, state) in state.fret_states.frets_mut() {
                if let Some(release_time) = state.release_time
                    && release_time < event_time
                {
                    state.release_time = None;
                    state.position = FretPosition::Idle;
                }
            }
        }
    }

    fn meta_note_on(&mut self, channel: u8, key: u8) {
        let Some(string) = channel_to_string(channel - NUMBER_OF_CHANNELS as u8) else {
            return;
        };

        let state = self.states.get_state_mut(string);
        match key {
            META_EXPRESSION_SOFT => state.expression = Expression::Soft,
            META_EXPRESSION_HARD => state.expression = Expression::Hard,
            META_EXPRESSION_DAMP => state.expression = Expression::HardDampen,
            META_EXPRESSION_SLAM => state.expression = Expression::FingerSlam,
            META_SLUR_START => state.slur_active = true,
            META_SLUR_END => state.slur_active = false,
            _ => {}
        }
    }

    fn note_on_string(
        &mut self,
        event_time: Instant,
        string: GuitarString,
        key: u8,
        velocity: u8,
        sink: &mut dyn CommandSink,
    ) {
        let string_state = self.states.get_state_mut(string);
        let Some(fret) = string_state.get_fret_by_pitch(key) else {
            eprintln!("Ignoring note: key {key} out of range for string {string:?}");
            return;
        };

        string_state.playing_note = Some(key);

        // --- Release held frets above the new fret ----------------------------
        // A pressed (or dampened) higher fret would mask the lower note.
        for (fret, state) in string_state.fret_states.frets_mut().skip(fret as usize + 1) {
            if state.position != FretPosition::Idle {
                sink.schedule(
                    shifted(event_time, -EMERGENCY_UNFRET_MS),
                    Message::UnfretFast(string, fret),
                );
                state.position = FretPosition::Idle;
                state.release_time = None;
            }
        }

        if string_state.slur_active {
            self.slur_note(string, fret, event_time, sink);
        } else {
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
    }

    fn slur_note(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        if fret != Fret::NoFret {
            self.schedule_fret(string, fret, t, sink);
            self.schedule_nut_dampen(string, fret, t, sink);
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
            self.schedule_unfret(string, Fret::Fret1, shifted(t, 2000), sink);
        }
    }

    fn schedule_pre_dampen(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        self.schedule_dampen(string, fret, shifted(t, -DAMPEN_PREP_MS), sink);
    }

    fn schedule_dampen(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let fret = if fret == Fret::NoFret { Fret::Fret11 } else { fret };

        let st = &mut self.states.get_state_mut(string);
        let fret_state = st.fret_states.get_state_mut(fret);
        if fret_state.position != FretPosition::Damping {
            sink.schedule(t, Message::Dampen(string, fret));
            fret_state.position = FretPosition::Damping;
            fret_state.release_time = None;
        }
    }

    fn schedule_fret(&mut self, string: GuitarString, fret: Fret, t: Instant, sink: &mut dyn CommandSink) {
        let st = &mut self.states.get_state_mut(string);
        let fret_state = st.fret_states.get_state_mut(fret);
        if fret_state.position != FretPosition::Holding {
            sink.schedule(shifted(t, -FRET_QUIET_PREP_MS), Message::FretQuiet(string, fret));
            fret_state.position = FretPosition::Holding;
            fret_state.release_time = None;
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
        if fret_state.position != FretPosition::Idle {
            sink.schedule(shifted(t, -2 * FRET_FAST_PREP_MS), Message::UnfretFast(string, fret));
        }

        sink.schedule(shifted(t, -FRET_FAST_PREP_MS), Message::FretFast(string, fret));
        fret_state.position = FretPosition::Holding;
        fret_state.release_time = None;
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

        let mut at = shifted(t, -PLUCK_VOLUME_PREP_MS);
        if let Some(last_pluck) = st.last_pluck {
            at = at.max(last_pluck);
        }

        sink.schedule(at, Message::PluckVolume(string, volume.into()));
        st.last_volume = Some(volume);
    }

    // -----------------------------------------------------------------------
    // Note off
    // -----------------------------------------------------------------------

    fn note_off(&mut self, event_time: Instant, channel: u8, key: u8, sink: &mut dyn CommandSink) {
        let Some(string) = (match self.midi_channel_mode {
            MidiChannelMode::MuseScorePlugin => {
                if channel as usize >= NUMBER_OF_CHANNELS {
                    None
                } else {
                    channel_to_string(channel)
                }
            }
            MidiChannelMode::AutoNoMeta => self
                .states
                .iter()
                .find(|s| s.playing_note == Some(key))
                .map(|s| s.string),
        }) else {
            return;
        };

        let string_state = self.states.get_state_mut(string);
        let Some(_) = string_state.get_fret_by_pitch(key) else {
            eprintln!("Ignoring note-off: key {key} out of range for string {string:?}");
            return;
        };

        // As each string is monophonic, we can treat note_off in the same way as all_notes_off
        self.stop_sounding_all(string, event_time, sink);
    }

    /// CC 123/120 "all notes off": stops whatever is sounding
    /// regardless of key (MuseScore sends this per channel on pause/stop).
    fn all_notes_off(&mut self, event_time: Instant, sink: &mut dyn CommandSink) {
        let playing_strings: Vec<_> = self
            .states
            .iter()
            .filter(|s| s.playing_note.is_some())
            .map(|s| s.string)
            .collect();

        for string in playing_strings {
            self.stop_sounding_all(string, event_time, sink);
        }
    }

    fn stop_sounding_all(&mut self, string: GuitarString, event_time: Instant, sink: &mut dyn CommandSink) {
        let st = self.states.get_state_mut(string);
        let Some(playing_note) = st.playing_note else {
            return;
        };

        let fret = st.get_fret_by_pitch(playing_note).expect("String is playing this note");
        self.schedule_mute_sequence(string, fret, event_time, sink);
        // self.mute_idle_strings(event_time, sink);

        let st = self.states.get_state_mut(string);
        st.playing_note = None;
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
        let fs = st.fret_states.get_state_mut(fret);

        let release_time = shifted(t, UNFRET_QUIET_DURATION_MS);

        if fs.position == FretPosition::Idle || fs.release_time.is_some_and(|rt| rt <= release_time) {
            return;
        }

        sink.schedule(t, Message::Unfret(string, fret));
        fs.release_time = Some(release_time);
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

    /// Records scheduled commands, modelling the scheduler's cancellation
    /// rule: a new message cancels all *later* pending messages for the same
    /// (string, fret) combination.
    #[derive(Default)]
    struct RecordingSink {
        entries: Vec<(u64, Message)>,
    }

    impl RecordingSink {
        fn record(&mut self, base: Instant, at: Instant, message: Message) {
            let ms = at.duration_since(base).as_millis() as u64;
            if let Some(new_sf) = message.get_string_and_fret() {
                self.entries.retain(|&(t, m)| match m.get_string_and_fret() {
                    Some(existing_sf) => !(existing_sf == new_sf && t > ms),
                    None => true,
                });
            }
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
                    MidiChannelMode::MuseScorePlugin,
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
                EngineEvent::NoteOn(NoteOnData { channel, key, velocity }),
                &mut self.sink,
            );
        }

        fn off(&mut self, ms: u64, channel: u8, key: u8) {
            self.engine.handle(
                self.at(ms),
                EngineEvent::NoteOff(NoteOffData { channel, key }),
                &mut self.sink,
            );
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
        // fret1 dampen, then the pluck. At note-off the sounding fret gets the
        // dampen+settle sequence; the already-damped Fret 1 is released
        // immediately (idle strings cool down).
        assert_eq!(
            msgs,
            vec![
                (t - 200, Message::PluckVolume(e, 200.into())),
                (t - 60, Message::FretQuiet(e, Fret::Fret5)),
                (t - 60, Message::Dampen(e, Fret::Fret1)),
                (t, Message::Pluck(e)),
                (500 + t, Message::Unfret(e, Fret::Fret1)),
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
        // Exactly one dampen (at note-on). The fret is still Damping at
        // note-off, so it is released immediately, without settle time.
        assert_eq!(f.sink.count(Message::Dampen(e, Fret::Fret5)), 1);
        assert!(msgs.contains(&(500 + LATENCY_MS, Message::Unfret(e, Fret::Fret5))));
    }

    #[test]
    fn fret1_dampened_while_fretting_and_released_for_open_string() {
        let mut f = Fixture::new();
        f.on(0, 0, 64 + 5, 100); // fret 5 -> dampens fret 1
        f.off(400, 0, 64 + 5);
        // The open note comes within Fret 1's 500ms release window, so it must
        // be emergency-unfretted in time for the pluck.
        f.on(600, 0, 64, 100);

        let msgs = f.messages();
        assert_eq!(f.sink.count(Message::Dampen(e, Fret::Fret1)), 1);
        let t_open = 600 + LATENCY_MS;
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
