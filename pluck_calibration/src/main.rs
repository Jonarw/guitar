use std::io::Write;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;
use std::{env, io};

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result};
use protocol::{Fret, GuitarString, Message, PluckTechnique};

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
/// Pause between the hard and soft pluck within one volume step (milliseconds).
const BETWEEN_TECHNIQUES_MS: u64 = 50;

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
    println!("String | Fret    | Soft Vol | Hard Vol");
    println!("-------|---------|----------|----------");

    let mut results: Vec<(
        /*string_name*/ &str,
        Fret,
        u8, // soft min volume
        u8, // hard min volume
    )> = Vec::new();

    for (guitar_string, string_name, max_frets) in STRING_CONFIGS {
        send_message(&mut *port, &Message::PluckEnable(guitar_string))?;
        thread::sleep(Duration::from_millis(50));

        let frets = &ALL_FRETS[..=(max_frets as usize)];
        let mut previous: Option<(u8, u8)> = None;

        for &fret in frets {
            // Carry the previous fret's *lower* threshold forward as the next
            // start volume.  Soft isn't necessarily quieter than hard (it
            // depends on mechanical tolerances), so use min(soft, hard).
            let start_vol = previous.map(|(soft, hard)| soft.min(hard)).unwrap_or(INITIAL_VOLUME);

            let (soft_vol, hard_vol) = find_min_volume(start_vol, &mut *port, &capture, guitar_string, fret)
                .with_context(|| format!("calibration failed for string {string_name} at {fret:?}"))?;

            let fret_label = format!("{fret:?}");
            println!("{string_name:6} | {fret_label:7} | {soft_vol:8} | {hard_vol}");

            previous = Some((soft_vol, hard_vol));
            results.push((string_name, fret, soft_vol, hard_vol));
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
) -> Result<(u8, u8)> {
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

            // Coarse search only cares whether *any* technique produced sound.
            let (soft, hard) = test_pluck(port, capture, guitar_string, vol)?;
            if soft || hard {
                break;
            }

            vol += COARSE_STEP;
        }

        vol = vol.saturating_sub(COARSE_STEP * 2);
        send_message(port, &Message::Dampen(guitar_string, Fret::Fret12))?;
        thread::sleep(Duration::from_millis(1000));
        send_message(port, &Message::Unfret(guitar_string, Fret::Fret12))?;
        thread::sleep(Duration::from_millis(1000));
    }

    // --- Fine search: advance by FINE_STEP ----------
    // At each step we fire a hard pluck, wait BETWEEN_TECHNIQUES_MS, then a soft
    // pluck, each in its own capture window.  We keep stepping until *both* have
    // been detected (or we exceed MAX_VOLUME), recording the first volume at
    // which each fired.
    let mut soft_found: Option<u8> = None;
    let mut hard_found: Option<u8> = None;
    loop {
        if vol >= MAX_VOLUME {
            break;
        }

        let (soft, hard) = test_pluck(port, capture, guitar_string, vol)?;
        if soft && soft_found.is_none() {
            soft_found = Some(vol);
        }
        if hard && hard_found.is_none() {
            hard_found = Some(vol);
        }
        if soft_found.is_some() && hard_found.is_some() {
            break;
        }

        vol += FINE_STEP;
    }

    if fret != Fret::NoFret {
        send_message(port, &Message::Unfret(guitar_string, fret))?;
    }

    // Ensure the string is left in soft mode for the next fret's search.
    send_message(port, &Message::PluckTechnique(guitar_string, PluckTechnique::Soft))?;

    // A calibration with missing spots is unusable, so fail hard.
    let soft = soft_found.context("no soft-pluck threshold found below MAX_VOLUME")?;
    let hard = hard_found.context("no hard-pluck threshold found below MAX_VOLUME")?;
    Ok((soft, hard))
}

/// At a given volume, fires a hard pluck and captures its own audio window, then
/// (after BETWEEN_TECHNIQUES_MS) fires a soft pluck and captures a second,
/// separate window.  Returns `(soft_heard, hard_heard)` indicating which
/// techniques exceeded the detection threshold.
fn test_pluck(
    port: &mut dyn Write,
    capture: &AudioCapture,
    guitar_string: GuitarString,
    volume: u8,
) -> Result<(bool, bool)> {
    send_message(port, &Message::PluckVolume(guitar_string, volume.into()))?;
    thread::sleep(Duration::from_millis(VOLUME_PREP_MS));

    // --- Hard pluck, captured in its own window ---
    let hard_samples = capture.capture_around(TIME_PER_STEP_MS, || {
        send_message(port, &Message::PluckTechnique(guitar_string, PluckTechnique::Hard))?;
        send_message(port, &Message::Pluck(guitar_string))?;
        Ok(())
    })?;
    let hard_rms = max_window_rms(&hard_samples, capture.sample_rate);

    if hard_rms > DETECTION_THRESHOLD {
        thread::sleep(Duration::from_millis(500));
    }

    // --- Soft pluck, captured in a separate window ---
    let soft_samples = capture.capture_around(TIME_PER_STEP_MS, || {
        send_message(port, &Message::PluckTechnique(guitar_string, PluckTechnique::Soft))?;
        thread::sleep(Duration::from_millis(BETWEEN_TECHNIQUES_MS));
        send_message(port, &Message::Pluck(guitar_string))?;
        Ok(())
    })?;
    let soft_rms = max_window_rms(&soft_samples, capture.sample_rate);

    println!("String: {guitar_string:?}, Vol: {volume}, hard RMS: {hard_rms}, soft RMS: {soft_rms}");

    Ok((soft_rms > DETECTION_THRESHOLD, hard_rms > DETECTION_THRESHOLD))
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
/// string,fret,min_volume_soft,min_volume_hard
/// E,NoFret,140,156
/// E,Fret1,141,157
/// E,Fret2,143,159
/// ...
/// ```
///
/// `min_volume_soft` / `min_volume_hard` are the lowest PluckVolume (0–255)
/// that produced a detectable sound for the soft / hard technique.  The
/// calibration aborts if no threshold is found below MAX_VOLUME, so every row
/// always has both values.
/// `lily_conductor` will read this file to populate a `StringVolumeTable`,
/// mapping the min volume to the minimum of a per-(string, fret) range and
/// using a configured constant for the maximum.
fn write_csv(path: &str, results: &[(&str, Fret, u8, u8)]) -> Result<()> {
    use std::fs::File;
    use std::io::BufWriter;

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);

    writeln!(w, "string,fret,min_volume_soft,min_volume_hard")?;
    for (string_name, fret, soft_vol, hard_vol) in results {
        writeln!(w, "{},{:?},{},{}", string_name, fret, soft_vol, hard_vol)?;
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
