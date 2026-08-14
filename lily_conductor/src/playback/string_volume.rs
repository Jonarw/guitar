use std::path::Path;

use protocol::{Fret, PluckTechnique};

use crate::machine_score::MidiVolume;

/// Calibrated pluck-volume range for one string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringVolumeRange {
    pub min: u8,
    pub max: u8,
}

impl StringVolumeRange {
    /// Maps MIDI volume (`0..=127`) into this string's calibrated pluck-volume range.
    pub fn map_midi_volume(&self, volume: MidiVolume) -> u8 {
        assert!(self.min <= self.max, "StringVolumeRange min must be <= max");
        let span = u16::from(self.max - self.min);
        let mapped = u16::from(self.min) + (u16::from(volume.volume) * span) / u16::from(MidiVolume::MAX_VALUE);
        mapped as u8
    }
}

/// Runtime table of calibrated pluck-volume ranges, one per (string, fret) combination.
///
/// Indexed as `ranges[part_index][fret as u8]` where `fret as u8 == 0` is the open string
/// (`Fret::NoFret`) and `1..=18` correspond to `Fret1..=Fret18`.
///
/// When a finger presses the string at higher frets the string's angle at the bridge
/// increases, lowering the effective volume for a given pluck force.  The per-fret ranges
/// compensate for this: they are expected to scale upward toward higher frets so that the
/// perceived volume remains consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringVolumeTable {
    pub ranges: [[(StringVolumeRange, StringVolumeRange); 13]; 6],
}

impl StringVolumeTable {
    /// Returns the calibrated range for a (part index, fret) pair.
    pub fn range_for(&self, part_index: usize, fret: Fret, technique: PluckTechnique) -> StringVolumeRange {
        let ranges = self
            .ranges
            .get(part_index)
            .expect("part index outside of 0..=5 for StringVolumeTable")[fret as usize];

        match technique {
            PluckTechnique::Soft => ranges.0,
            PluckTechnique::Hard => ranges.1,
        }
    }

    /// Constructs a table where every (string, fret) cell has the same range.
    pub fn uniform(range: StringVolumeRange) -> Self {
        Self {
            ranges: [[(range, range); 13]; 6],
        }
    }
}

impl StringVolumeRange {
    pub fn empty() -> Self {
        Self { min: 0, max: 0 }
    }
}

impl StringVolumeTable {
    /// Loads a `StringVolumeTable` from a CSV file produced by `pluck_calibration`.
    ///
    /// Expected format (header required):
    /// ```text
    /// string,fret,min_volume
    /// E,NoFret,149
    /// E,Fret1,152
    /// E,Fret2,not_detected
    /// ```
    ///
    /// The `min` of each cell is taken from the CSV.  The `max` is computed as
    /// `min + volume_span`, clamped to 255.  Cells with `not_detected` (or any
    /// missing row) keep the provided `fallback` range.
    pub fn from_csv(path: impl AsRef<Path>) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path.as_ref()).map_err(|e| format!("Cannot read calibration file: {e}"))?;

        let mut table = Self::uniform(StringVolumeRange::empty());

        for (line_no, line) in content.lines().enumerate() {
            // Skip header and blank lines.
            if line_no == 0 || line.trim().is_empty() {
                continue;
            }

            let cols: Vec<&str> = line.splitn(3, ',').collect();
            if cols.len() != 4 {
                return Err(format!(
                    "Calibration CSV line {}: expected 3 columns, got {}",
                    line_no + 1,
                    cols.len()
                ));
            }

            let part_index = parse_string_name(cols[0])
                .ok_or_else(|| format!("Calibration CSV line {}: unknown string '{}'", line_no + 1, cols[0]))?;
            let fret = parse_fret(cols[1])
                .ok_or_else(|| format!("Calibration CSV line {}: unknown fret '{}'", line_no + 1, cols[1]))?;

            let min_vol_soft = cols[2].trim();
            let min_soft: u8 = min_vol_soft.parse().map_err(|_| {
                format!(
                    "Calibration CSV line {}: invalid min_volume_soft '{}'",
                    line_no + 1,
                    min_vol_soft
                )
            })?;

            let min_vol_hard = cols[3].trim();
            let min_hard: u8 = min_vol_hard.parse().map_err(|_| {
                format!(
                    "Calibration CSV line {}: invalid min_volume_hard '{}'",
                    line_no + 1,
                    min_vol_hard
                )
            })?;

            table.ranges[part_index][fret as usize] = (
                StringVolumeRange { min: min_soft, max: 0 },
                StringVolumeRange { min: min_hard, max: 0 },
            );
        }

        for range in &mut table.ranges {
            if range.iter().find(|r| r.0.min == 0 || r.1.min == 0).is_some() {
                return Err("Found empty range".to_owned());
            }

            let min = range
                .iter()
                .map(|r| r.0.min)
                .min()
                .ok_or_else(|| "Range should contain items")?;
            let max = range
                .iter()
                .map(|r| r.0.min)
                .max()
                .ok_or_else(|| "Range should contain items")?;
            let span = (max - min) * 2;
            for item in range {
                item.0.max = item.0.min.saturating_add(span);
                item.1.max = item.1.min.saturating_add(span);
            }
        }

        Ok(table)
    }
}

fn parse_string_name(s: &str) -> Option<usize> {
    match s.trim() {
        "E" => Some(5),
        "A" => Some(4),
        "D" => Some(3),
        "G" => Some(2),
        "B" => Some(1),
        "e" => Some(0),
        _ => None,
    }
}

fn parse_fret(s: &str) -> Option<Fret> {
    match s.trim() {
        "NoFret" => Some(Fret::NoFret),
        "Fret1" => Some(Fret::Fret1),
        "Fret2" => Some(Fret::Fret2),
        "Fret3" => Some(Fret::Fret3),
        "Fret4" => Some(Fret::Fret4),
        "Fret5" => Some(Fret::Fret5),
        "Fret6" => Some(Fret::Fret6),
        "Fret7" => Some(Fret::Fret7),
        "Fret8" => Some(Fret::Fret8),
        "Fret9" => Some(Fret::Fret9),
        "Fret10" => Some(Fret::Fret10),
        "Fret11" => Some(Fret::Fret11),
        "Fret12" => Some(Fret::Fret12),
        "Fret13" => Some(Fret::Fret13),
        "Fret14" => Some(Fret::Fret14),
        "Fret15" => Some(Fret::Fret15),
        "Fret16" => Some(Fret::Fret16),
        "Fret17" => Some(Fret::Fret17),
        "Fret18" => Some(Fret::Fret18),
        _ => None,
    }
}
