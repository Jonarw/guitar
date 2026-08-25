// use midir::{Ignore, MidiInput, os::unix::VirtualInput};

// fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let mut midi_in = MidiInput::new("Automated Guitar")?;
//     midi_in.ignore(Ignore::None);

//     let _connection = midi_in.create_virtual(
//         "Automated Guitar",
//         |timestamp, message, _| {
//             println!("{timestamp}: {message:02x?}");

//             // Eventually:
//             // guitar_engine.handle_midi(timestamp, message);
//         },
//         (),
//     )?;

//     // Keep the connection alive.
//     loop {
//         std::thread::park();
//     }
// }
//
use midir::{Ignore, MidiInput};
use midly::{MidiMessage, live::LiveEvent};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut midi_in = MidiInput::new("Automated Guitar")?;
    midi_in.ignore(Ignore::None);

    let ports = midi_in.ports();

    let port = ports
        .iter()
        .find(|p| {
            midi_in
                .port_name(p)
                .map(|name| name.contains("Midi Through Port-0"))
                .unwrap_or(false)
        })
        .ok_or("Midi Through Port-0 not found")?;

    let _connection = midi_in.connect(
        port,
        "automated-guitar-input",
        move |timestamp, bytes, _| match LiveEvent::parse(bytes) {
            Ok(LiveEvent::Midi { channel, message }) => {
                print!("{timestamp:>10}  ch={}  ", channel.as_int() + 1);

                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        println!("Note On   {:>3}  velocity {:>3}", key.as_int(), vel.as_int());
                    }

                    MidiMessage::NoteOff { key, vel } => {
                        println!("Note Off  {:>3}  velocity {:>3}", key.as_int(), vel.as_int());
                    }

                    MidiMessage::Controller { controller, value } => {
                        println!("CC        {:>3}  value    {:>3}", controller.as_int(), value.as_int());
                    }

                    MidiMessage::PitchBend { bend } => {
                        println!("Pitch Bend {}", bend.as_int());
                    }

                    MidiMessage::ProgramChange { program } => {
                        println!("Program   {}", program.as_int());
                    }

                    MidiMessage::ChannelAftertouch { vel } => {
                        println!("Aftertouch {}", vel.as_int());
                    }

                    MidiMessage::Aftertouch { key, vel } => {
                        println!("Poly AT   key {:>3}  value {:>3}", key.as_int(), vel.as_int());
                    }
                }
            }

            Ok(other) => {
                println!("{timestamp:>10}  {:?}", other);
            }

            Err(err) => {
                eprintln!("{timestamp:>10}  invalid MIDI: {err}");
            }
        },
        (),
    )?;

    println!("Listening...");
    loop {
        std::thread::park();
    }
}
