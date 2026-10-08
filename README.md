# Guitarrobot - Automated Acoustic Guitar

Guitarrobot is an automated acoustic guitar. Solenoid-driven fingers fret and dampen the strings, stepper-driven picks pluck them. The host software supports MIDI input, which it converts to control signals that are sent to the hardware over an RS485 bus.

This repo contains sources for the MCU firmware and host software needed to control the guitar. Other documentation can be found here:

- **Showcase Video:** https://youtu.be/p_n5vLgNi6Q
- **Mechanical parts:** https://www.printables.com/model/1846714-guitarobot-self-playing-guitar
- **PCB files:** https://oshwlab.com/adfaskdjfasldkf/project_nzxtrjle

## Repository Layout

### Firmware

| Folder | Target | Description |
|---|---|---|
| `fret/` | STM32G030F6 | Firmware for the fretting controllers. One MCU per fret (13 total), selected via cargo feature (`fret1`..`fret13`). Drives the solenoids that press down / dampen the strings. |
| `pluck/` | RP2040 | Firmware for the plucking controllers. Two MCUs (`left` / `right` cargo feature, 3 strings each). Drives the plucking steppers and volume PWM. |

### Host software

The host software is only tested on Fedora Linux, but in principle it should also run on Windows or MacOS, with the exception of `pluck_calibration`, which uses ALSA for audio capture.

| Folder | Description |
|---|---|
| `protocol/` | Shared message protocol (serde/postcard, COBS framing) spoken between host and firmware over RS485. Used by everything. |
| `midi_conductor/` | Real-time MIDI → guitar playback engine. Listens on a MIDI input and schedules note commands. Tested with MuseScore and REAPER on Fedora Linux, but should in principle work with any MIDI software and also on Windows and MacOS. Config via `config.toml`.  |
| `lily_conductor/` | Parses a limited subset of LilyPond scores (`.ly`) and plays them on the guitar. Experimental / not really used by me. |
| `lilyparse/` | LilyPond parsing library used by `lily_conductor`. |
| `txt_conductor/` | Plays simple text-based scripts (`tempo <bpm>`, then `<beat> <action> [args]`). Useful for sequence-based testing. |
| `manual_conductor/` | CLI (`rs485host`) for sending individual protocol messages manually. Useful for bringup, testing and calibration. |
| `pluck_calibration/` | Calibration tool: plucks strings at various volumes, records the result via ALSA audio, and produces `calibration.csv`. |
| `string_volume/` | Shared calibrated pluck-volume tables, consumed by the conductors. |

### Misc

| Folder / File | Description |
|---|---|
| `musescore_plugin/` | MuseScore plugin (`apply_markers.qml`) that can be used to apply articulation markers. `midi_conductor`, when configured in `MuseScorePlugin` mode via `config.toml`, can use these markers to switch between different articulation modes (soft, hard ...). |
| `calibration-example.csv` | Example of measured pluck-volume calibration data. |
| `calculations.ods` | Design calculations (mechanics, electronics). |

## Building

All subprojects (with the obvious exception of `musescore_plugin`) are written in Rust. There is no root-level workspace, each subproject is just built individually using `cargo build`.

### Host tools

```sh
cd <subproject>
cargo build
```

`pluck_calibration` needs ALSA development headers (`alsa-lib-devel` on Fedora / `libasound2-dev` on Debian).

### Firmware

Requires the stable toolchain with the `thumbv6m-none-eabi` target installed:

```sh
rustup target add thumbv6m-none-eabi
```

Building `fret`:
```sh
cd fret
cargo build --no-default-features --features fret1   # one feature per fret, fret1..fret13
```

Building `pluck`:
```sh
cd pluck
cargo build --no-default-features --features left    # or 'right'
```

### Flashing

Flashing and debugging is done with [probe-rs](https://probe.rs/). `cargo run` flashes the firmware and gives defmt log output.

## Running Host Software

Run using `cargo run`, or directly run the compiled binaries. The tools will inform you about CLI arguments they require.

## AI Disclaimer

Parts of this repository were created with the help of LLMs. The firmware projects (`fret` and `pluck` firmwares) are almost entirely human-written, some of the supporting tools (`txt_conductor` and `manual_conductor`) are almost entirely LLM-written. Everything else is somewhere in-between.

## License

[GPLv3](LICENSE)
