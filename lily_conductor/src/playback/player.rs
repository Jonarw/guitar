use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::timeline::CommandTimeline;

/// Plays a [`CommandTimeline`] in real time by writing encoded protocol frames to `port`.
///
/// The function blocks until the entire timeline has been transmitted (i.e. until the
/// final [`protocol::Message::Reset`] at the end of the score is sent).
pub fn play(timeline: &CommandTimeline, port: &mut dyn Write) -> Result<()> {
    let start = Instant::now();

    for cmd in &timeline.commands {
        let target = Duration::from_millis(cmd.time_ms);
        let elapsed = start.elapsed();
        if elapsed < target {
            thread::sleep(target - elapsed);
        }

        let mut buf = [0u8; protocol::MAX_FRAME_SIZE];
        let frame = cmd
            .message
            .encode(&mut buf)
            .context("failed to encode protocol message")?;

        port.write_all(frame).context("failed to write to serial port")?;
    }

    Ok(())
}
