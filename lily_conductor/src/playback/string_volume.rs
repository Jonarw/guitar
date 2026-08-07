use std::path::Path;

use protocol::Fret;

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
    pub ranges: [[StringVolumeRange; 13]; 6],
}

impl StringVolumeTable {
    /// Returns the calibrated range for a (part index, fret) pair.
    pub fn range_for(&self, part_index: usize, fret: Fret) -> StringVolumeRange {
        self.ranges
            .get(part_index)
            .expect("part index outside of 0..=5 for StringVolumeTable")[fret as usize]
    }

    /// Constructs a table where every (string, fret) cell has the same range.
    pub fn uniform(range: StringVolumeRange) -> Self {
        Self {
            ranges: [[range; 13]; 6],
        }
    }
}

impl Default for StringVolumeTable {
    fn default() -> Self {
        // Defaults preserve current behaviour until calibration values are provided.
        Self::uniform(StringVolumeRange { min: 0, max: 255 })
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

        let mut table = Self::uniform(StringVolumeRange { min: 0, max: 0 });

        for (line_no, line) in content.lines().enumerate() {
            // Skip header and blank lines.
            if line_no == 0 || line.trim().is_empty() {
                continue;
            }

            let cols: Vec<&str> = line.splitn(3, ',').collect();
            if cols.len() != 3 {
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

            let min_vol = cols[2].trim();
            if min_vol == "not_detected" {
                return Err(format!("Calibration CSV line {}: not detected", line_no + 1,));
            }

            let min: u8 = min_vol
                .parse()
                .map_err(|_| format!("Calibration CSV line {}: invalid min_volume '{}'", line_no + 1, min_vol))?;

            table.ranges[part_index][fret as usize] = StringVolumeRange { min, max: 0 };
        }

        for range in &mut table.ranges {
            if range.iter().find(|r| r.min == 0).is_some() {
                return Err("Found empty range".to_owned());
            }

            let min = range
                .iter()
                .map(|r| r.min)
                .min()
                .ok_or_else(|| "Range should contain items")?;
            let max = range
                .iter()
                .map(|r| r.min)
                .max()
                .ok_or_else(|| "Range should contain items")?;
            let span = (max - min) * 2;
            for item in range {
                item.max = item.min.saturating_add(span);
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

#[cfg(test)]
mod tests {
    use crate::{
        machine_score::MidiVolume,
        playback::string_volume::{StringVolumeRange, StringVolumeTable},
    };

    #[test]
    fn maps_midi_volume_into_string_range() {
        let range = StringVolumeRange { min: 149, max: 220 };
        assert_eq!(range.map_midi_volume(MidiVolume::new(0)), 149);
        assert_eq!(range.map_midi_volume(MidiVolume::new(MidiVolume::MAX_VALUE)), 220);
    }

    #[test]
    fn default_string_volume_table_spans_full_range() {
        let table = StringVolumeTable::default();
        let range = table.range_for(0, protocol::Fret::NoFret);
        assert_eq!(range.min, 0);
        assert_eq!(range.max, 255);
    }

    #[test]
    fn string_volume_table_per_fret_lookup() {
        use protocol::Fret;

        let low = StringVolumeRange { min: 100, max: 180 };
        let high = StringVolumeRange { min: 120, max: 200 };
        let mut table = StringVolumeTable::uniform(low);
        // Override Fret5 on string 0 with a different range.
        table.ranges[0][Fret::Fret5 as usize] = high;

        assert_eq!(table.range_for(0, Fret::NoFret), low);
        assert_eq!(table.range_for(0, Fret::Fret5), high);
        assert_eq!(table.range_for(1, Fret::Fret5), low); // other strings unaffected
    }
}
