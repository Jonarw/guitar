#![no_std]
#![no_main]

use crate::hw::{Rs485, strings::*};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::pwm::SetDutyCycle;
use protocol::{Fret, GuitarString, MessageAction, MessageFrame, Parser};

use {defmt_rtt as _, panic_probe as _};

pub mod hw;

pub static MY_FRET: Fret = Fret::Fret2;
type FretSignal = Signal<CriticalSectionRawMutex, MessageAction>;

static FRET_SIGNALS: [FretSignal; 6] = [
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
];

static PWM_MAX_FORCE: u8 = 100;
static PWM_HOLD_FORCE: u8 = 33;
static PWM_DAMPEN_FORCE: u8 = 25;

async fn pwm_ramp(pwm: &mut GuitarStringPwm, start: u8, end: u8, duration: Duration) {}

async fn fret_fast(pwm: &mut GuitarStringPwm, state: FretState) {
    match state {
        FretState::Idle => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
            Timer::after(Duration::from_millis(100)).await;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
        }
        FretState::Fretting => {}
        FretState::Dampening => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
            Timer::after(Duration::from_millis(50)).await;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
        }
    }
}

async fn fret_calibration(pwm: &mut GuitarStringPwm, _state: FretState) {
    pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
    Timer::after(Duration::from_millis(100)).await;
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE - 3);
    Timer::after(Duration::from_millis(1000)).await;
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE - 5);
    Timer::after(Duration::from_millis(1000)).await;
    pwm.set_duty_cycle_fully_off();
}

async fn fret_quiet(pwm: &mut GuitarStringPwm, state: FretState) {
    match state {
        FretState::Idle => {
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
            Timer::after(Duration::from_millis(100)).await;
        }
        FretState::Fretting => return,
        FretState::Dampening => {}
    }

    let mut time = Instant::now();
    for i in PWM_DAMPEN_FORCE..PWM_MAX_FORCE {
        time += Duration::from_millis(3);
        Timer::at(time).await;

        pwm.set_duty_cycle_percent(i);
    }

    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
}

async fn dampen(pwm: &mut GuitarStringPwm, state: FretState) {
    match state {
        FretState::Idle => {
            let mut time = Instant::now();
            for i in 0..PWM_DAMPEN_FORCE {
                time += Duration::from_millis(5);
                Timer::at(time).await;

                pwm.set_duty_cycle_percent(i);
            }
        }
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            Timer::after(Duration::from_millis(22)).await;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
        }
        FretState::Dampening => {}
    }
}

async fn unfret(pwm: &mut GuitarStringPwm, state: FretState) {
    match state {
        FretState::Idle => {}
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            Timer::after(Duration::from_millis(22)).await;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
            Timer::after(Duration::from_millis(50)).await;
            pwm.set_duty_cycle_fully_off();
        }
        FretState::Dampening => {
            pwm.set_duty_cycle_fully_off();
        }
    }
}

#[derive(PartialEq, Eq)]
enum FretState {
    Idle,
    Fretting,
    Dampening,
}

#[embassy_executor::task(pool_size = 6)]
async fn string_task(mut pwm: GuitarStringPwm, signal: &'static FretSignal) {
    pwm.set_duty_cycle_fully_off();

    let mut state = FretState::Idle;

    loop {
        let action = signal.wait().await;

        defmt::info!("Processing action {}", action);

        match action {
            MessageAction::FretFast => {
                fret_fast(&mut pwm, state).await;
                state = FretState::Fretting;
            }
            MessageAction::FretQuiet => {
                fret_quiet(&mut pwm, state).await;
                state = FretState::Fretting;
            }
            MessageAction::Unfret => {
                unfret(&mut pwm, state).await;
                state = FretState::Idle;
            }
            MessageAction::Dampen => {
                dampen(&mut pwm, state).await;
                state = FretState::Dampening;
            }
            MessageAction::FretCalibration => {
                fret_calibration(&mut pwm, state).await;
                state = FretState::Idle;
            }
            _ => {}
        }
    }
}

async fn send_confirm_presence(request: &MessageFrame, rs485: &mut Rs485) {
    let message = MessageFrame::new(MessageAction::ConfirmPresence, request.string, request.fret, 0.into());
    let bytes = message.cobs_encode();
    if let Err(e) = rs485.write(&bytes).await {
        defmt::error!("TX error: {}", e);
    }
}

fn get_fret_signal(string: GuitarString) -> &'static FretSignal {
    match string {
        GuitarString::E => &FRET_SIGNALS[5],
        GuitarString::A => &FRET_SIGNALS[0],
        GuitarString::D => &FRET_SIGNALS[4],
        GuitarString::G => &FRET_SIGNALS[1],
        GuitarString::B => &FRET_SIGNALS[3],
        GuitarString::e => &FRET_SIGNALS[2],
    }
}

async fn process_message(message: &MessageFrame, rs485: &mut Rs485) {
    defmt::info!("Incoming Message: {}", message);

    if message.fret != MY_FRET {
        return;
    }

    match message.action {
        MessageAction::Presence => send_confirm_presence(message, rs485).await,
        MessageAction::FretFast
        | MessageAction::FretQuiet
        | MessageAction::Dampen
        | MessageAction::Unfret
        | MessageAction::FretCalibration => get_fret_signal(message.string).signal(message.action),
        _ => {}
    }
}

#[embassy_executor::task]
async fn rs485_task(mut rs485: Rs485) -> ! {
    let mut parser = Parser::new();

    loop {
        let mut buffer = [0u8; 1];
        match rs485.read(&mut buffer).await {
            Ok(_) => match parser.consume(buffer[0]) {
                Ok(Some(message)) => process_message(&message, &mut rs485).await,
                Ok(None) => {}
                Err(e) => defmt::error!("Parse Error: {}", e),
            },
            Err(e) => defmt::error!("Rx error: {}", e),
        }
    }
}
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let hw = hw::init();

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChA(hw.ch_a),
        &FRET_SIGNALS[0]
    )));

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChB(hw.ch_b),
        &FRET_SIGNALS[1]
    )));

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChC(hw.ch_c),
        &FRET_SIGNALS[2]
    )));

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChD(hw.ch_d),
        &FRET_SIGNALS[3]
    )));

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChE(hw.ch_e),
        &FRET_SIGNALS[4]
    )));

    spawner.spawn(defmt::unwrap!(string_task(
        GuitarStringPwm::ChF(hw.ch_f),
        &FRET_SIGNALS[5]
    )));

    spawner.spawn(defmt::unwrap!(rs485_task(hw.rs485)));
}
