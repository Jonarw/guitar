#![no_std]
#![no_main]

pub mod hw;

use crate::hw::{PWM_CLOCK, PluckStepper, Rs485, VolumePwm};
use embassy_executor::Spawner;
use embassy_rp::pwm::SetDutyCycle;
use embassy_rp::{gpio, uart};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Read;
use embedded_io_async::Write;
use protocol::{GuitarString, Message, Parser, PluckVolume};
use stepgen::Stepgen;
use {defmt_rtt as _, panic_probe as _};

type PluckSignal = Signal<CriticalSectionRawMutex, Message>;
type VolumeSignal = Signal<CriticalSectionRawMutex, PluckVolume>;

struct ChannelSignals {
    pluck_signal: PluckSignal,
    volume_signal: VolumeSignal,
}

impl ChannelSignals {
    pub const fn new() -> Self {
        Self {
            pluck_signal: Signal::new(),
            volume_signal: Signal::new(),
        }
    }
}

static SIGNALS: [ChannelSignals; 3] = [ChannelSignals::new(), ChannelSignals::new(), ChannelSignals::new()];
static VARIANT: Variant = Variant::Right;

#[derive(Clone, Copy)]
pub enum Variant {
    Left,
    Right,
}

fn is_string_relevant(string: GuitarString) -> bool {
    matches!(
        (VARIANT, string),
        (Variant::Left, GuitarString::E | GuitarString::A | GuitarString::D)
            | (Variant::Right, GuitarString::G | GuitarString::B | GuitarString::e)
    )
}

async fn send_bytes(bytes: &[u8], rs485: &mut Rs485) -> Result<(), uart::Error> {
    rs485.de.set_high();
    rs485.uart.write(bytes).await?;
    rs485.uart.blocking_flush()?;
    while rs485.uart.busy() {
        core::hint::spin_loop();
    }
    rs485.de.set_low();

    Ok(())
}

async fn send_confirm_presence(rs485: &mut Rs485) {
    let message = Message::ConfirmPresence;
    let mut buffer = [0; protocol::MAX_FRAME_SIZE];
    let frame = message.encode(&mut buffer).unwrap();
    if let Err(e) = send_bytes(frame, rs485).await {
        defmt::error!("TX error: {}", e);
    }
}

fn get_signals(string: GuitarString) -> &'static ChannelSignals {
    match string {
        GuitarString::E => &SIGNALS[0],
        GuitarString::A => &SIGNALS[1],
        GuitarString::D => &SIGNALS[2],
        GuitarString::G => &SIGNALS[2],
        GuitarString::B => &SIGNALS[1],
        GuitarString::e => &SIGNALS[0],
    }
}

async fn process_message(message: &Message, rs485: &mut Rs485) {
    defmt::info!("Incoming Message: {}", message);

    if !matches!(message, Message::Reset) {
        let Some(string) = message.get_string() else {
            return;
        };

        if !is_string_relevant(string) {
            return;
        }
    }

    match message {
        Message::PluckPresence(_) => send_confirm_presence(rs485).await,
        Message::Pluck(string) | Message::PluckDisable(string) | Message::PluckEnable(string) => {
            get_signals(*string).pluck_signal.signal(*message)
        }
        Message::Reset => {
            for signal in SIGNALS.iter() {
                signal.pluck_signal.signal(*message);
                signal.volume_signal.signal(50.into());
            }
        }
        Message::PluckVolume(string, volume) => get_signals(*string).volume_signal.signal(*volume),
        _ => {}
    }
}

#[embassy_executor::task]
async fn rs485_task(mut rs485: Rs485) -> ! {
    let mut parser = Parser::new();

    loop {
        let mut buffer = [0u8; 1];
        match rs485.uart.read(&mut buffer).await {
            Ok(_) => match parser.consume(buffer[0]) {
                Ok(Some(message)) => process_message(&message, &mut rs485).await,
                Ok(None) => {}
                Err(e) => defmt::error!("Parse Error: {}", e),
            },
            Err(e) => defmt::error!("Rx error: {}", e),
        }
    }
}

#[derive(Clone, Copy, defmt::Format)]
enum PluckStepperState {
    Disabled,
    EnabledLeft,
    EnabledRight,
}

struct StepperStuff {
    hw: PluckStepper,
    stepgen: Stepgen,
    state: PluckStepperState,
}

static PLUCK_STEPS: u32 = 200 * 8 / 3;

impl StepperStuff {
    pub fn new(hw: PluckStepper) -> Self {
        const ACC: u32 = 1_000_000;
        const SPEED: u32 = 20000;
        let mut stepgen = Stepgen::new(1_000_000);
        stepgen.set_acceleration(ACC << 8).unwrap();
        stepgen.set_target_speed(SPEED << 8).unwrap();

        Self {
            hw,
            stepgen,
            state: PluckStepperState::Disabled,
        }
    }

    fn set_next_direction(&mut self) {
        let level = match (self.state, self.hw.reversed) {
            (PluckStepperState::Disabled | PluckStepperState::EnabledLeft, true) => gpio::Level::High,
            (PluckStepperState::Disabled | PluckStepperState::EnabledLeft, false) => gpio::Level::Low,
            (PluckStepperState::EnabledRight, true) => gpio::Level::Low,
            (PluckStepperState::EnabledRight, false) => gpio::Level::High,
        };

        self.hw.dir.set_level(level);

        self.state = match self.state {
            PluckStepperState::Disabled | PluckStepperState::EnabledLeft => PluckStepperState::EnabledRight,
            PluckStepperState::EnabledRight => PluckStepperState::EnabledLeft,
        }
    }

    async fn rotate(&mut self, steps: u32) {
        self.stepgen
            .set_target_step(self.stepgen.current_step() + steps)
            .unwrap();

        let mut time = Instant::now();
        loop {
            let Some(delay) = self.stepgen.next() else {
                break;
            };

            self.hw.step.set_high();

            let delay_ns = (delay * 1000 + 128) >> 8;
            let half_delay_ns = delay_ns / 2;
            time += Duration::from_nanos(half_delay_ns as u64);
            Timer::at(time).await;

            self.hw.step.set_low();

            time += Duration::from_nanos((delay_ns - half_delay_ns) as u64);
            Timer::at(time).await;
        }
    }

    pub async fn enable(&mut self) {
        self.hw.en.set_low();
        self.set_next_direction();
        self.rotate(PLUCK_STEPS / 2).await;
    }

    pub async fn disable(&mut self) {
        self.set_next_direction();
        self.rotate(PLUCK_STEPS / 2).await;
        self.hw.en.set_high();
        self.state = PluckStepperState::Disabled;
    }

    pub async fn pluck(&mut self) {
        self.set_next_direction();
        self.rotate(PLUCK_STEPS).await;
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn stepper_task(stepper: PluckStepper, signal: &'static PluckSignal) -> ! {
    let mut stepper = StepperStuff::new(stepper);

    loop {
        let action = signal.wait().await;

        match (action, stepper.state) {
            (Message::PluckEnable(_), PluckStepperState::Disabled) => stepper.enable().await,

            (
                Message::PluckDisable(_) | Message::Reset,
                PluckStepperState::EnabledLeft | PluckStepperState::EnabledRight,
            ) => stepper.disable().await,

            (Message::Pluck(_), PluckStepperState::EnabledLeft | PluckStepperState::EnabledRight) => {
                stepper.pluck().await
            }

            _ => defmt::error!("Received invalid {} command while in state {}", action, stepper.state),
        }
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn volume_task(mut pwm: VolumePwm, signal: &'static VolumeSignal) -> ! {
    const PWM_MIDDLE_US: u32 = 1500;
    const PWM_RANGE_US: u32 = 1000;

    let pwm_max_volume_us = match pwm.reversed {
        true => PWM_MIDDLE_US - PWM_RANGE_US,
        false => PWM_MIDDLE_US + PWM_RANGE_US,
    };

    let min_volume_count = PWM_CLOCK * (PWM_MIDDLE_US / 100) / (1_000_000 / 100);
    let max_volume_count = PWM_CLOCK * (pwm_max_volume_us / 100) / (1_000_000 / 100);

    let get_pwm_count_from_volume = |vol| {
        ((min_volume_count * (PluckVolume::max_volume() as u32 + 1 - vol as u32) + max_volume_count * vol as u32)
            / (PluckVolume::max_volume() as u32 + 1)) as u16
    };

    pwm.servo_pwm.set_duty_cycle(get_pwm_count_from_volume(50)).unwrap();

    loop {
        let volume = signal.wait().await;
        let volume_count = get_pwm_count_from_volume(volume.volume());
        pwm.servo_pwm.set_duty_cycle(volume_count).unwrap();
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let hw = hw::init();

    spawner.spawn(defmt::unwrap!(rs485_task(hw.rs485)));

    for (i, ch) in hw.channels.into_iter().enumerate() {
        let signals = &SIGNALS[i];
        spawner.spawn(defmt::unwrap!(stepper_task(ch.stepper, &signals.pluck_signal)));
        spawner.spawn(defmt::unwrap!(volume_task(ch.volume_pwm, &signals.volume_signal)));
    }
}
