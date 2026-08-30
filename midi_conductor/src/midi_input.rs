//! MIDI input: subscribes to a system MIDI port (e.g. ALSA "Midi Through",
//! which MuseScore can send to) and forwards normalized events to the engine.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use midir::{MidiInput, MidiInputConnection};
use midly::{MidiMessage, live::LiveEvent};

/// Normalized MIDI event the engine understands. Channels are 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineEvent {
    NoteOn { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8 },
    AllNotesOff { channel: u8 },
}

impl EngineEvent {
    pub fn channel(&self) -> u8 {
        match self {
            EngineEvent::NoteOn { channel, .. }
            | EngineEvent::NoteOff { channel, .. }
            | EngineEvent::AllNotesOff { channel } => *channel,
        }
    }
}

struct CallbackState {
    /// Local clock anchor for converting midir timestamps to `Instant`s.
    base_instant: Instant,
    /// midir timestamp of the first received event (anchor on the ALSA clock).
    base_ts: Option<u64>,
    tx: Sender<(Instant, EngineEvent)>,
}

impl CallbackState {
    fn event_instant(&mut self, ts_micros: u64) -> Instant {
        let base_ts = *self.base_ts.get_or_insert_with(|| {
            self.base_instant = Instant::now();
            ts_micros
        });

        self.base_instant + Duration::from_micros(ts_micros.saturating_sub(base_ts))
    }
}

pub struct MidiConnection {
    _connection: MidiInputConnection<CallbackState>,
}

/// Subscribes to the first MIDI input port whose name contains `substring`
/// and forwards parsed events (with their MIDI timestamps converted to
/// `Instant`s) to `tx`.
pub fn connect(substring: &str, tx: Sender<(Instant, EngineEvent)>) -> Result<MidiConnection, String> {
    let midi_in = MidiInput::new("Automated Guitar").map_err(|e| format!("Failed to create MIDI input: {e}"))?;

    let ports = midi_in.ports();
    let port = ports
        .iter()
        .find(|p| {
            midi_in
                .port_name(p)
                .map(|name| name.contains(substring))
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            let available: Vec<String> = ports.iter().filter_map(|p| midi_in.port_name(p).ok()).collect();
            format!("MIDI port containing '{substring}' not found. Available ports: {available:?}")
        })?;

    let state = CallbackState {
        base_instant: Instant::now(),
        base_ts: None,
        tx,
    };

    eprintln!("Found MIDI port '{}'.", midi_in.port_name(port).unwrap_or_default());

    let connection = midi_in
        .connect(port, "automated-guitar-input", handle_event, state)
        .map_err(|e| format!("Failed to connect MIDI input: {e}"))?;

    eprintln!("Connected to MIDI");

    Ok(MidiConnection {
        _connection: connection,
    })
}

enum MidiControlMessage {
    AllSoundOff = 120,
    AllNotesOff = 123,
}

impl MidiControlMessage {
    pub fn from_u8(value: u8) -> Option<MidiControlMessage> {
        match value {
            120 => Some(MidiControlMessage::AllSoundOff),
            123 => Some(MidiControlMessage::AllNotesOff),
            _ => None,
        }
    }

    pub fn is_all_notes_off(value: u8) -> bool {
        matches!(
            Self::from_u8(value),
            Some(MidiControlMessage::AllSoundOff) | Some(MidiControlMessage::AllNotesOff)
        )
    }
}

fn handle_event(ts_micros: u64, bytes: &[u8], state: &mut CallbackState) {
    let Ok(LiveEvent::Midi { channel, message }) = LiveEvent::parse(bytes) else {
        // System / meta events are irrelevant for playback.
        return;
    };

    let channel = channel.as_int();
    let event = match message {
        // Velocity-0 NoteOn is the conventional NoteOff.
        MidiMessage::NoteOn { key, vel } if vel.as_int() == 0 => EngineEvent::NoteOff {
            channel,
            key: key.as_int(),
        },
        MidiMessage::NoteOn { key, vel } => EngineEvent::NoteOn {
            channel,
            key: key.as_int(),
            velocity: vel.as_int(),
        },
        MidiMessage::NoteOff { key, .. } => EngineEvent::NoteOff {
            channel,
            key: key.as_int(),
        },
        MidiMessage::Controller { controller, .. } if MidiControlMessage::is_all_notes_off(controller.as_int()) => {
            EngineEvent::AllNotesOff { channel }
        }
        _ => return,
    };

    let at = state.event_instant(ts_micros);
    if state.tx.send((at, event)).is_err() {
        eprintln!("Engine channel closed; dropping MIDI event.");
    }
}
