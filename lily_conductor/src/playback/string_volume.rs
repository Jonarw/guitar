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
    pub ranges: [[StringVolumeRange; 19]; 6],
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
            ranges: [[range; 19]; 6],
        }
    }
}

impl Default for StringVolumeTable {
    fn default() -> Self {
        // Defaults preserve current behaviour until calibration values are provided.
        Self::uniform(StringVolumeRange { min: 0, max: 255 })
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
