#![no_std]
#![no_main]

pub mod hw;

use crate::hw::{PWM_CLOCK, PluckStepper, Rs485, VolumePwm};
use embassy_executor::Spawner;
use embassy_rp::pwm::SetDutyCycle;
use embassy_rp::uart;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Read;
use embedded_io_async::Write;
use protocol::{GuitarString, Message, Parser, PluckTechnique, PluckVolume};
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
        Message::Pluck(string)
        | Message::PluckDisable(string)
        | Message::PluckEnable(string)
        | Message::PluckSpeed(string, _)
        | Message::PluckTechnique(string, _) => get_signals(*string).pluck_signal.signal(*message),
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
    EnabledLeftSoft,
    EnabledRightSoft,
    EnabledLeftHard,
    EnabledRightHard,
}

impl PluckStepperState {
    fn get_abs_position(&self) -> i32 {
        match self {
            PluckStepperState::Disabled => 0,
            PluckStepperState::EnabledLeftSoft => -PLUCK_STEPS / 2,
            PluckStepperState::EnabledRightSoft => PLUCK_STEPS / 2,
            PluckStepperState::EnabledLeftHard => -FULL_CIRCLE_STEPS / 2 + PLUCK_STEPS / 2,
            PluckStepperState::EnabledRightHard => FULL_CIRCLE_STEPS / 2 - PLUCK_STEPS / 2,
        }
    }

    pub fn get_delta(&self, other: PluckStepperState) -> i32 {
        let mut ret = other.get_abs_position() - self.get_abs_position();
        if ret > FULL_CIRCLE_STEPS / 2 {
            ret -= FULL_CIRCLE_STEPS
        }

        if ret < -FULL_CIRCLE_STEPS / 2 {
            ret += FULL_CIRCLE_STEPS;
        }

        ret
    }
}

struct StepperStuff {
    hw: PluckStepper,
    stepgen: Stepgen,
    state: PluckStepperState,
    speed: u16,
}

static FULL_CIRCLE_STEPS: i32 = 200 * 8;
static PLUCK_STEPS: i32 = FULL_CIRCLE_STEPS / 3;

impl StepperStuff {
    const MAX_SPEED: u16 = 20000;
    pub fn new(hw: PluckStepper) -> Self {
        const ACC: u32 = 1_000_000;
        let mut stepgen = Stepgen::new(1_000_000);
        stepgen.set_acceleration(ACC << 8).unwrap();
        let mut ret = Self {
            hw,
            stepgen,
            state: PluckStepperState::Disabled,
            speed: 0,
        };

        ret.set_speed(Self::MAX_SPEED);
        ret
    }

    async fn rotate(&mut self, steps: u32) {
        self.stepgen
            .set_target_step(self.stepgen.current_step() + steps)
            .unwrap();

        let mut time = Instant::now();
        for delay in self.stepgen.by_ref() {
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
        self.move_to_state(PluckStepperState::EnabledLeftSoft).await;
    }

    pub async fn disable(&mut self) {
        self.move_to_state(PluckStepperState::Disabled).await;
        self.hw.en.set_high();
    }

    pub async fn pluck(&mut self) {
        let new_state = match self.state {
            PluckStepperState::Disabled => {
                defmt::error!("Cannot pluck when disabled");
                return;
            }
            PluckStepperState::EnabledLeftHard => PluckStepperState::EnabledRightHard,
            PluckStepperState::EnabledRightHard => PluckStepperState::EnabledLeftHard,
            PluckStepperState::EnabledLeftSoft => PluckStepperState::EnabledRightSoft,
            PluckStepperState::EnabledRightSoft => PluckStepperState::EnabledLeftSoft,
        };

        self.move_to_state(new_state).await;
    }

    async fn move_to_state(&mut self, new_state: PluckStepperState) {
        let delta = self.state.get_delta(new_state);
        if (delta > 0) != self.hw.reversed {
            self.hw.dir.set_high();
        } else {
            self.hw.dir.set_low();
        }

        self.rotate(delta.unsigned_abs()).await;
        self.state = new_state;
    }

    pub async fn set_technique(&mut self, new_technique: PluckTechnique) {
        let new_state = match (new_technique, self.state) {
            (_, PluckStepperState::Disabled) => {
                defmt::error!("Cannot set technique when disabled");
                return;
            }
            (PluckTechnique::Soft, PluckStepperState::EnabledLeftHard | PluckStepperState::EnabledLeftSoft) => {
                PluckStepperState::EnabledLeftSoft
            }
            (PluckTechnique::Soft, PluckStepperState::EnabledRightHard | PluckStepperState::EnabledRightSoft) => {
                PluckStepperState::EnabledRightSoft
            }
            (PluckTechnique::Hard, PluckStepperState::EnabledLeftHard | PluckStepperState::EnabledLeftSoft) => {
                PluckStepperState::EnabledLeftHard
            }
            (PluckTechnique::Hard, PluckStepperState::EnabledRightHard | PluckStepperState::EnabledRightSoft) => {
                PluckStepperState::EnabledRightHard
            }
        };

        let speed = self.speed;
        self.set_speed(Self::MAX_SPEED);
        self.move_to_state(new_state).await;
        self.set_speed(speed);
    }

    pub fn set_speed(&mut self, speed: u16) {
        self.stepgen.set_target_speed((speed as u32) << 8).unwrap();
        self.speed = speed;
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn stepper_task(stepper: PluckStepper, signal: &'static PluckSignal) -> ! {
    let mut stepper = StepperStuff::new(stepper);

    loop {
        let action = signal.wait().await;

        match action {
            Message::PluckEnable(_) => stepper.enable().await,
            Message::PluckDisable(_) => stepper.disable().await,
            Message::Pluck(_) => stepper.pluck().await,
            Message::PluckTechnique(_, t) => stepper.set_technique(t).await,
            Message::PluckSpeed(_, s) => stepper.set_speed(s),

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
