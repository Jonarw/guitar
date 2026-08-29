//! Calibrated pluck-volume tables shared by the conductors.
//!
//! Extracted from `lily_conductor` so that both the score-based
//! (`lily_conductor`) and the real-time MIDI (`midi_conductor`) playback
//! engines map dynamics to hardware pluck volumes the same way.

use std::path::Path;

use anyhow::{bail, Context};
use protocol::{Fret, GuitarString, PluckTechnique};

/// MIDI note velocity in range `0..=127`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiVolume {
    pub volume: u8,
}

impl MidiVolume {
    pub const MAX_VALUE: u8 = 127;

    /// Creates a validated MIDI volume value.
    pub fn new(volume: u8) -> Self {
        if volume > Self::MAX_VALUE {
            panic!("MIDI volume outside of allowed range");
        }

        Self { volume }
    }
}

/// Calibrated pluck-volume range for one string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringVolumeRange {
    pub min: i32,
    pub max: i32,
}

const NUMBER_OF_ENTRIES: usize = 13;

impl StringVolumeRange {
    /// Maps MIDI volume (`0..=127`) into this string's calibrated pluck-volume range.
    pub fn map_midi_volume(&self, volume: MidiVolume) -> i32 {
        let span = self.max - self.min;
        self.min + i32::from(volume.volume) * span / i32::from(MidiVolume::MAX_VALUE)
    }

    pub fn neutral() -> Self {
        Self { min: 0, max: 255 }
    }

    pub fn empty() -> Self {
        Self { min: 0, max: 0 }
    }
}

#[derive(Debug, Clone, Copy)]
struct StringTable {
    ranges: [StringVolumeRange; NUMBER_OF_ENTRIES],
    hard_offset: i32,
    hysteresis: i32,
    last_requested_volume: i32,
    last_returned_value: i32,
}

impl StringTable {
    pub fn uniform(range: StringVolumeRange) -> Self {
        Self {
            ranges: [range; _],
            hard_offset: 0,
            hysteresis: 0,
            last_requested_volume: i32::MIN,
            last_returned_value: 0,
        }
    }

    pub fn map_midi_volume(&mut self, fret: Fret, technique: PluckTechnique, volume: MidiVolume) -> u8 {
        let range = self.ranges[fret as usize];
        let mut ret = range.map_midi_volume(volume);
        if technique == PluckTechnique::Hard {
            ret += self.hard_offset;
        }

        let requested_value = ret;
        if ret < self.last_requested_volume {
            ret -= self.hysteresis;
        } else if ret == self.last_requested_volume {
            ret = self.last_returned_value;
        }

        if ret <= 0 || ret > 255 {
            panic!("Invalid volume {} after compensation", ret);
        }

        self.last_requested_volume = requested_value;
        self.last_returned_value = ret;

        ret as u8
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
#[derive(Debug, Clone)]
pub struct StringVolumeTable {
    tables: [StringTable; 6],
}

impl StringVolumeTable {
    pub fn map_midi_volume(
        &mut self,
        part_index: usize,
        fret: Fret,
        technique: PluckTechnique,
        volume: MidiVolume,
    ) -> u8 {
        self.tables[part_index].map_midi_volume(fret, technique, volume)
    }

    pub fn map_midi_volume_string(
        &mut self,
        string: GuitarString,
        fret: Fret,
        technique: PluckTechnique,
        volume: MidiVolume,
    ) -> u8 {
        self.tables[5 - string as usize].map_midi_volume(fret, technique, volume)
    }

    /// Constructs a table where every (string, fret) cell has the same range.
    pub fn uniform(range: StringVolumeRange) -> Self {
        Self {
            tables: [StringTable::uniform(range); 6],
        }
    }

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
    pub fn from_csv(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref()).context("Cannot read calibration file")?;

        let lines: Vec<_> = content.lines().collect();
        let mut ret = Self::uniform(StringVolumeRange::empty());

        let mut i_line = 1;
        for (i_string, table) in ret.tables.iter_mut().enumerate().rev() {
            let mut raw_data = [(0i32, 0i32, 0i32, 0i32); NUMBER_OF_ENTRIES];
            for data in raw_data.iter_mut() {
                let line = lines[i_line];
                let cols: Vec<&str> = line.splitn(6, ',').collect();

                if parse_string_name(cols[0]) != Some(i_string) {
                    bail!("Expected string #{}, found {}", i_string, cols[0]);
                }

                if cols.len() != 6 {
                    bail!(
                        "Calibration CSV line {}: expected 6 columns, got {}",
                        i_line,
                        cols.len()
                    );
                }

                *data = (cols[2].parse()?, cols[3].parse()?, cols[4].parse()?, cols[5].parse()?);

                i_line += 1;
            }

            let up_sum: i32 = raw_data.iter().map(|d| d.0 + d.1).sum();
            let down_sum: i32 = raw_data.iter().map(|d| d.2 + d.3).sum();
            let hysteresis = (up_sum - down_sum) / (NUMBER_OF_ENTRIES as i32 * 2);

            let soft_sum: i32 = raw_data.iter().map(|d| d.0 + d.2).sum();
            let hard_sum: i32 = raw_data.iter().map(|d| d.1 + d.3).sum();

            let hard_offset = (hard_sum - soft_sum) / (NUMBER_OF_ENTRIES as i32 * 2);

            table.hard_offset = hard_offset;
            table.hysteresis = hysteresis;

            let span_top = raw_data[NUMBER_OF_ENTRIES - 1].0
                + raw_data[NUMBER_OF_ENTRIES - 1].1
                + raw_data[NUMBER_OF_ENTRIES - 1].2
                + raw_data[NUMBER_OF_ENTRIES - 1].3;
            let span_bottom = raw_data[0].0 + raw_data[0].1 + raw_data[0].2 + raw_data[0].3;

            let span = (span_top - span_bottom) * 2 / 4;

            for (i, range) in table.ranges.iter_mut().enumerate() {
                range.min = raw_data[i].0;
                range.max = range.min + span;
            }
        }

        Ok(ret)
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
