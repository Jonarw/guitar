//! Real-time scheduler that writes protocol messages to the serial port.
//!
//! A dedicated writer thread owns the serial port and drains a vec of
//! time-stamped commands.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use protocol::{Message, MAX_FRAME_SIZE};

/// Destination for commands produced by the playback engine.
///
/// Abstracted into a trait so the engine can be unit-tested against a
/// recording sink.
pub trait CommandSink {
    /// Schedules a message to be sent at `at`.
    fn schedule(&mut self, at: Instant, message: Message);
}

enum SchedulerCommand {
    Schedule { at: Instant, message: Message },
    SendNow(Message),
}

/// Handle to the scheduler. Cheap to clone.
#[derive(Clone)]
pub struct Scheduler {
    tx: Sender<SchedulerCommand>,
}

impl CommandSink for Scheduler {
    fn schedule(&mut self, at: Instant, message: Message) {
        self.tx
            .send(SchedulerCommand::Schedule { at, message })
            .expect("Failed to send message");
    }
}

impl Scheduler {
    /// Sends a message immediately, bypassing the scheduled queue.
    pub fn send_now(&self, message: Message) {
        self.tx
            .send(SchedulerCommand::SendNow(message))
            .expect("Failed to send message");
    }
}

#[derive(Debug)]
struct ScheduledItem {
    at: Instant,
    message: Message,
}

/// Spawns the writer thread and returns its handle.
pub fn spawn(mut port: Box<dyn serialport::SerialPort>) -> Scheduler {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("guitar-scheduler".to_owned())
        .spawn(move || writer_loop(&mut *port, rx))
        .expect("failed to spawn scheduler thread");
    Scheduler { tx }
}

fn send_message(port: &mut dyn serialport::SerialPort, message: &Message) {
    let mut buffer = [0u8; MAX_FRAME_SIZE];
    match message.encode(&mut buffer) {
        Ok(frame) => {
            if let Err(e) = port.write_all(frame) {
                eprintln!("Serial write error for {message:?}: {e}");
            }
        }
        Err(e) => eprintln!("Failed to encode {message:?}: {e}"),
    }
}

fn writer_loop(port: &mut dyn serialport::SerialPort, rx: Receiver<SchedulerCommand>) {
    let mut scheduled_items: Vec<ScheduledItem> = Vec::new();

    loop {
        // Send everything that is due.
        let now = Instant::now();
        for due_item in scheduled_items.extract_if(.., |item| item.at < now) {
            send_message(port, &due_item.message);
        }

        // Wait for the next command or the next deadline, whichever comes first.
        // A timeout just means a scheduled command became due: loop around and
        // send it. Only a disconnected channel ends the thread.
        use std::sync::mpsc::RecvTimeoutError;
        let next_due = scheduled_items.iter().min_by(|x, y| x.at.cmp(&y.at));
        let received = match next_due {
            Some(next_due) => {
                let wait = next_due.at.saturating_duration_since(Instant::now());
                match rx.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break, // All senders dropped: shut down.
                }
            }
            None => match rx.recv() {
                Ok(command) => Some(command),
                Err(_) => break, // All senders dropped: shut down.
            },
        };

        match received {
            Some(SchedulerCommand::Schedule { at, message }) => {
                match message.get_string_and_fret() {
                    Some(new_sf) => {
                        // extract and remove all items that refer to the same (string, fret) combination
                        // AND are scheduled later than the new item
                        scheduled_items
                            .extract_if(.., |item| match item.message.get_string_and_fret() {
                                Some(existing_sf) => existing_sf == new_sf && item.at > at,
                                None => false,
                            })
                            .for_each(drop);
                    }
                    None => {}
                }

                scheduled_items.push(ScheduledItem { at, message });
            }
            Some(SchedulerCommand::SendNow(message)) => {
                send_message(port, &message);
            }
            None => {}
        }
    }
}
