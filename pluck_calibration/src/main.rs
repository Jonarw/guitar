use std::io::Write;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;
use std::{env, io};

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result};
use protocol::{Fret, GuitarString, Message};

// ---------------------------------------------------------------------------
// Calibration parameters — adjust as needed
// ---------------------------------------------------------------------------

/// Coarse volume search step (PluckVolume units, 0–255).
const COARSE_STEP: u8 = 10;
/// Fine volume search step used once a sound-producing region is found.
const FINE_STEP: u8 = 1;
/// Starting PluckVolume for the search.
const INITIAL_VOLUME: u8 = 20;
/// Upper bound; give up if no sound is produced below this volume.
const MAX_VOLUME: u8 = 200;
/// RMS evaluation window length in milliseconds.
const RMS_WINDOW_MS: u64 = 10;
/// Minimum RMS level required to consider a pluck "successful".
const DETECTION_THRESHOLD: f32 = 0.01;
/// Total audio capture window after each pluck (milliseconds).
const TIME_PER_STEP_MS: u64 = 250;
/// Requested audio capture sample rate; ALSA will use the nearest supported rate.
const SAMPLE_RATE: u32 = 48_000;
/// Time to hold the fret servo before plucking (milliseconds).
const FRET_PREP_MS: u64 = 2000;
const POST_COARSE_MS: u64 = 2000;
const VOLUME_PREP_MS: u64 = 250;
/// Pause between consecutive tests on the same string (milliseconds).
const BETWEEN_STEPS_MS: u64 = 200;

// ---------------------------------------------------------------------------
// String / fret configuration
// ---------------------------------------------------------------------------

const STRING_CONFIGS: [(GuitarString, &str, u8); 6] = [
    (GuitarString::E, "E", 12),
    (GuitarString::A, "A", 12),
    (GuitarString::D, "D", 12),
    (GuitarString::G, "G", 12),
    (GuitarString::B, "B", 12),
    (GuitarString::e, "e", 12),
];

const ALL_FRETS: [Fret; 19] = [
    Fret::NoFret,
    Fret::Fret1,
    Fret::Fret2,
    Fret::Fret3,
    Fret::Fret4,
    Fret::Fret5,
    Fret::Fret6,
    Fret::Fret7,
    Fret::Fret8,
    Fret::Fret9,
    Fret::Fret10,
    Fret::Fret11,
    Fret::Fret12,
    Fret::Fret13,
    Fret::Fret14,
    Fret::Fret15,
    Fret::Fret16,
    Fret::Fret17,
    Fret::Fret18,
];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("--list-devices") {
        return list_alsa_devices();
    }

    if args.len() != 4 {
        eprintln!("Usage: {} <tty_device> <alsa_capture_device> <csv_output>", args[0]);
        eprintln!("       {} --list-devices", args[0]);
        eprintln!();
        eprintln!("Example: {} /dev/ttyUSB0 sysdefault:CARD=USB calibration.csv", args[0]);
        std::process::exit(1);
    }

    configure_maya22().context("Failed to configure MAYA22")?;

    let tty_path = &args[1];
    let alsa_device = &args[2];
    let csv_path = &args[3];

    // --- Open serial port ---------------------------------------------------
    let mut port = serialport::new(tty_path, 115200)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("Failed to open serial port '{tty_path}'"))?;

    // --- Open ALSA capture device ------------------------------------------
    let capture = AudioCapture::open(alsa_device).with_context(|| {
        format!(
            "Failed to open ALSA capture device '{alsa_device}'.\n\
             Run with --list-devices to see available devices."
        )
    })?;

    println!("ALSA device:  {alsa_device}  ({} Hz, mono)", capture.sample_rate);

    // --- Initial reset -------------------------------------------------------
    send_message(&mut *port, &Message::Reset)?;
    thread::sleep(Duration::from_millis(500));

    // --- Calibration sweep --------------------------------------------------
    println!();
    println!("String | Fret    | Min Volume");
    println!("-------|---------|------------");

    let mut results: Vec<(/*string_name*/ &str, Fret, Option<u8>)> = Vec::new();

    for (guitar_string, string_name, max_frets) in STRING_CONFIGS {
        send_message(&mut *port, &Message::PluckEnable(guitar_string))?;
        thread::sleep(Duration::from_millis(50));

        let frets = &ALL_FRETS[..=(max_frets as usize)];
        let mut result = None;

        for &fret in frets {
            result = find_min_volume(
                result.unwrap_or(INITIAL_VOLUME),
                &mut *port,
                &capture,
                guitar_string,
                fret,
            )?;

            let fret_label = format!("{fret:?}");
            match result {
                Some(vol) => println!("{string_name:6} | {fret_label:7} | {vol}"),
                None => println!("{string_name:6} | {fret_label:7} | not detected (> {MAX_VOLUME})"),
            }

            results.push((string_name, fret, result));
            thread::sleep(Duration::from_millis(BETWEEN_STEPS_MS));
        }

        send_message(&mut *port, &Message::PluckVolume(guitar_string, 50.into()))?;
        send_message(&mut *port, &Message::PluckDisable(guitar_string))?;
    }

    // --- Final reset --------------------------------------------------------
    send_message(&mut *port, &Message::Reset)?;
    println!("\nCalibration complete.");

    // --- Export CSV ---------------------------------------------------------
    write_csv(csv_path, &results).with_context(|| format!("Failed to write CSV to '{csv_path}'"))?;
    println!("Results written to {csv_path}.");

    Ok(())
}

fn configure_maya22() -> anyhow::Result<()> {
    Command::new("maya22-control").args(["-c", "hiz"]).output()?;
    Command::new("maya22-control").args(["-l", "127"]).output()?;
    Command::new("maya22-control").args(["-r", "127"]).output()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ALSA audio capture
// ---------------------------------------------------------------------------

struct AudioCapture {
    device_name: String,
    sample_rate: u32,
}

impl AudioCapture {
    fn open(device_name: &str) -> Result<Self> {
        // Try the given name first; if it fails, suggest the plughw: equivalent.
        let pcm = PCM::new(device_name, Direction::Capture, false).with_context(|| {
            // Build a plughw: suggestion when the user passed hw:N,M.
            let suggestion = if let Some(rest) = device_name.strip_prefix("hw:") {
                format!(" (try 'plughw:{rest}' to enable format/rate conversion)")
            } else {
                String::new()
            };
            format!("Failed to open ALSA device '{device_name}'{suggestion}")
        })?;
        let hwp = HwParams::any(&pcm)?;
        hwp.set_channels(1)?;
        hwp.set_rate(SAMPLE_RATE, ValueOr::Nearest)?;
        hwp.set_format(Format::s16())?;
        hwp.set_access(Access::RWInterleaved)?;
        pcm.hw_params(&hwp)?;
        let actual_rate = hwp.get_rate()?;

        if actual_rate != SAMPLE_RATE {
            eprintln!(
                "Note: ALSA device sample rate is {actual_rate} Hz \
                 (requested {SAMPLE_RATE} Hz); RMS window adjusted."
            );
        }

        Ok(Self {
            device_name: device_name.to_owned(),
            sample_rate: actual_rate,
        })
    }

    /// Opens a fresh PCM, starts it immediately (so the ring buffer is filling
    /// before the pluck command is sent), calls `while_capturing` to issue the
    /// pluck, then reads `duration_ms` of audio.  This ensures no samples are
    /// lost even if the hardware responds very quickly.
    fn capture_around<F>(&self, duration_ms: u64, while_capturing: F) -> Result<Vec<f32>>
    where
        F: FnOnce() -> Result<()>,
    {
        let num_frames = (self.sample_rate as u64 * duration_ms / 1000) as usize;

        let pcm = PCM::new(&self.device_name, Direction::Capture, false)?;
        {
            let hwp = HwParams::any(&pcm)?;
            hwp.set_channels(1)?;
            hwp.set_rate(self.sample_rate, ValueOr::Nearest)?;
            hwp.set_format(Format::s16())?;
            hwp.set_access(Access::RWInterleaved)?;
            pcm.hw_params(&hwp)?;
        }
        pcm.prepare()?;
        pcm.start()?;

        while_capturing()?;

        let io = pcm.io_i16()?;
        let mut raw = vec![0i16; num_frames];
        let mut offset = 0;
        while offset < num_frames {
            let n = io.readi(&mut raw[offset..])?;
            offset += n;
        }

        Ok(raw.iter().map(|&s| s as f32 / 32_767.0).collect())
    }
}

// ---------------------------------------------------------------------------
// Calibration logic
// ---------------------------------------------------------------------------

fn find_min_volume(
    start_vol: u8,
    port: &mut dyn Write,
    capture: &AudioCapture,
    guitar_string: GuitarString,
    fret: Fret,
) -> Result<Option<u8>> {
    let mut vol = start_vol;

    send_message(port, &Message::Dampen(guitar_string, Fret::Fret12))?;
    if fret != Fret::NoFret {
        send_message(port, &Message::FretQuiet(guitar_string, fret))?;
    }

    thread::sleep(Duration::from_millis(FRET_PREP_MS / 2));
    send_message(port, &Message::Unfret(guitar_string, Fret::Fret12))?;
    thread::sleep(Duration::from_millis(FRET_PREP_MS / 2));

    if start_vol == INITIAL_VOLUME {
        // --- Coarse search: advance by COARSE_STEP until first sound detected ---
        loop {
            if vol >= MAX_VOLUME {
                break;
            }

            if test_pluck(port, capture, guitar_string, vol)? {
                break;
            }

            vol += COARSE_STEP;
        }

        vol -= COARSE_STEP * 2;
        send_message(port, &Message::Dampen(guitar_string, Fret::Fret12))?;
        thread::sleep(Duration::from_millis(1000));
        send_message(port, &Message::Unfret(guitar_string, Fret::Fret12))?;
        thread::sleep(Duration::from_millis(1000));
    }

    // --- Fine search: advance by FINE_STEP ----------
    let mut ret = None;
    loop {
        if vol >= MAX_VOLUME {
            break;
        }

        if test_pluck(port, capture, guitar_string, vol)? {
            ret = Some(vol);
            break;
        }

        vol += FINE_STEP;
    }

    if fret != Fret::NoFret {
        send_message(port, &Message::Unfret(guitar_string, fret))?;
    }

    Ok(ret)
}

fn test_pluck(port: &mut dyn Write, capture: &AudioCapture, guitar_string: GuitarString, volume: u8) -> Result<bool> {
    send_message(port, &Message::PluckVolume(guitar_string, volume.into()))?;
    thread::sleep(Duration::from_millis(VOLUME_PREP_MS));

    let samples = capture.capture_around(TIME_PER_STEP_MS, || {
        send_message(port, &Message::Pluck(guitar_string))?;
        Ok(())
    })?;

    let rms = max_window_rms(&samples, capture.sample_rate);
    println!("String: {guitar_string:?}, Vol: {volume}, RMS: {rms}");

    Ok(rms > DETECTION_THRESHOLD)
}

// ---------------------------------------------------------------------------
// Audio analysis
// ---------------------------------------------------------------------------

fn max_window_rms(samples: &[f32], sample_rate: u32) -> f32 {
    let window_len = (u64::from(sample_rate) * RMS_WINDOW_MS / 1000) as usize;
    if window_len == 0 || samples.len() < window_len {
        return 0.0;
    }
    samples.windows(window_len).map(rms).fold(0.0f32, f32::max)
}

fn rms(samples: &[f32]) -> f32 {
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

// ---------------------------------------------------------------------------
// Serial helpers
// ---------------------------------------------------------------------------

fn send_message(port: &mut dyn Write, msg: &Message) -> Result<()> {
    let mut buf = [0u8; protocol::MAX_FRAME_SIZE];
    let encoded = msg.encode(&mut buf).context("Failed to encode protocol message")?;
    port.write_all(encoded).context("Failed to write to serial port")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CSV export
// ---------------------------------------------------------------------------

/// Writes calibration results as a CSV file.
///
/// Format:
/// ```
/// string,fret,min_volume
/// E,NoFret,149
/// E,Fret1,152
/// E,Fret2,not_detected
/// ...
/// ```
///
/// `min_volume` is the lowest PluckVolume (0–255) that produced a detectable
/// sound, or the string `not_detected` if none was found up to MAX_VOLUME.
/// `lily_conductor` will read this file to populate a `StringVolumeTable`,
/// mapping `min_volume` to the minimum of a per-(string, fret) range and
/// using a configured constant for the maximum.
fn write_csv(path: &str, results: &[(&str, Fret, Option<u8>)]) -> Result<()> {
    use std::fs::File;
    use std::io::BufWriter;

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);

    writeln!(w, "string,fret,min_volume")?;
    for (string_name, fret, min_vol) in results {
        let vol_str = match min_vol {
            Some(v) => v.to_string(),
            None => "not_detected".to_owned(),
        };
        writeln!(w, "{},{:?},{}", string_name, fret, vol_str)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Device listing
// ---------------------------------------------------------------------------

fn list_alsa_devices() -> Result<()> {
    println!("Available ALSA capture devices:\n");
    for hint in alsa::device_name::HintIter::new_str(None, "pcm").context("Failed to enumerate ALSA PCM hints")? {
        // Skip devices that are explicitly output-only.
        if hint.direction == Some(alsa::Direction::Playback) {
            continue;
        }
        let name = hint.name.unwrap_or_default();
        let desc = hint.desc.unwrap_or_default();
        // Condense multi-line descriptions to a single line.
        let desc_oneline = desc.lines().collect::<Vec<_>>().join(" / ");
        println!("  {name:<30}  {desc_oneline}");
    }
    Ok(())
}
