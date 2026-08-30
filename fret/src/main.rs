#![no_std]
#![no_main]

use crate::hw::{Rs485, strings::*};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::pwm::SetDutyCycle;
use embedded_io_async::Read;
use embedded_io_async::Write;
use protocol::{Fret, GuitarString, Message, Parser, Percentage};

use {defmt_rtt as _, panic_probe as _};

pub mod hw;

pub static MY_FRET: Fret = Fret::Fret1;
type FretSignal = Signal<ThreadModeRawMutex, Message>;

static FRET_SIGNALS: [FretSignal; 6] = [
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
];

const PWM_DAMPEN_FORCE: Percentage = Percentage::new(24);
const PWM_HOLD_FORCE: Percentage = Percentage::new(45);
const PWM_MAX_FORCE: Percentage = Percentage::new(100);
const RELEASE_BREAK_OFF_MS: u64 = 1;

struct Interrupted {}

async fn wait_interruptible(duration: Duration, signal: &'static FretSignal) -> Result<(), Interrupted> {
    let increment = Duration::from_millis(1);

    let end = Instant::now() + duration;
    let mut now = Instant::now();
    while now < end {
        if signal.signaled() {
            return Err(Interrupted {});
        }

        now = (now + increment).min(end);
        Timer::at(end).await;
    }

    Ok(())
}

async fn wait_millis_interruptible(millis: u64, signal: &'static FretSignal) -> Result<(), Interrupted> {
    wait_interruptible(Duration::from_millis(millis), signal).await
}

async fn pwm_ramp(
    pwm: &mut GuitarStringPwm,
    start: u8,
    end: u8,
    duration: Duration,
    signal: &'static FretSignal,
) -> Result<(), Interrupted> {
    let duration_per_percent = duration / end.abs_diff(start) as u32;

    let mut time = Instant::now();
    if start < end {
        for i in start..end {
            if signal.signaled() {
                return Err(Interrupted {});
            }

            pwm.set_duty_cycle_percent(i);
            time += duration_per_percent;
            Timer::at(time).await;
        }
    } else {
        for i in ((end + 1)..start).rev() {
            if signal.signaled() {
                return Err(Interrupted {});
            }

            pwm.set_duty_cycle_percent(i);
            time += duration_per_percent;
            Timer::at(time).await;
        }
    }

    pwm.set_duty_cycle_percent(end);
    Ok(())
}

async fn idle_to_down(pwm: &mut GuitarStringPwm, signal: &'static FretSignal) -> Result<(), Interrupted> {
    pwm.set_duty_cycle_percent(PWM_MAX_FORCE.into());
    wait_millis_interruptible(4, signal).await?;
    pwm.set_duty_cycle_fully_off();
    wait_millis_interruptible(10, signal).await?;
    pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE.into());
    Ok(())
}

async fn fret_fast(
    pwm: &mut GuitarStringPwm,
    state: FretState,
    signal: &'static FretSignal,
) -> Result<(), Interrupted> {
    match state {
        FretState::Idle => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE.into());
            wait_millis_interruptible(5, signal).await?;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE.into());
        }
        FretState::Fretting => {}
        FretState::Dampening => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE.into());
            wait_millis_interruptible(4, signal).await?;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE.into());
        }
    }

    Ok(())
}

async fn fret_calibration(
    pwm: &mut GuitarStringPwm,
    state: FretState,
    signal: &'static FretSignal,
) -> Result<(), Interrupted> {
    fret_quiet(pwm, state, signal).await?;

    const CALIBRATION_DURATION_MS: u64 = 1000;
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE.get_value() - 10);
    wait_millis_interruptible(CALIBRATION_DURATION_MS, signal).await?;
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE.get_value() - 15);
    wait_millis_interruptible(CALIBRATION_DURATION_MS, signal).await?;
    pwm.set_duty_cycle_fully_off();

    Ok(())
}

async fn fret_quiet(
    pwm: &mut GuitarStringPwm,
    state: FretState,
    signal: &'static FretSignal,
) -> Result<(), Interrupted> {
    let ramp_duration = Duration::from_millis(100);
    match state {
        FretState::Idle => {
            idle_to_down(pwm, signal).await?;
            pwm_ramp(
                pwm,
                PWM_DAMPEN_FORCE.into(),
                PWM_MAX_FORCE.into(),
                ramp_duration,
                signal,
            )
            .await?;
        }
        FretState::Fretting => return Ok(()),
        FretState::Dampening => {
            pwm_ramp(
                pwm,
                PWM_DAMPEN_FORCE.into(),
                PWM_MAX_FORCE.into(),
                ramp_duration,
                signal,
            )
            .await?;
        }
    }

    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE.into());
    Ok(())
}

async fn dampen(pwm: &mut GuitarStringPwm, state: FretState, signal: &'static FretSignal) -> Result<(), Interrupted> {
    match state {
        FretState::Idle => {
            idle_to_down(pwm, signal).await?;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE.into());
        }
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            wait_millis_interruptible(RELEASE_BREAK_OFF_MS, signal).await?;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE.into());
        }
        FretState::Dampening => {}
    }

    Ok(())
}

async fn unfret(pwm: &mut GuitarStringPwm, state: FretState, signal: &'static FretSignal) -> Result<(), Interrupted> {
    let ramp_duration = Duration::from_millis(500);
    match state {
        FretState::Idle => {}
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            wait_millis_interruptible(RELEASE_BREAK_OFF_MS, signal).await?;
            pwm_ramp(pwm, PWM_DAMPEN_FORCE.into(), 3, ramp_duration, signal).await?;
        }
        FretState::Dampening => pwm_ramp(pwm, PWM_DAMPEN_FORCE.into(), 3, ramp_duration, signal).await?,
    }

    pwm.set_duty_cycle_fully_off();
    Ok(())
}

async fn unfret_fast(pwm: &mut GuitarStringPwm) {
    pwm.set_duty_cycle_fully_off();
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
            Message::FretFast(_, _) => {
                let _ = fret_fast(&mut pwm, state, signal).await;
                state = FretState::Fretting;
            }
            Message::FretQuiet(_, _) => {
                let _ = fret_quiet(&mut pwm, state, signal).await;
                state = FretState::Fretting;
            }
            Message::Unfret(_, _) => {
                let _ = unfret(&mut pwm, state, signal).await;
                state = FretState::Idle;
            }
            Message::UnfretFast(_, _) | Message::Reset => {
                unfret_fast(&mut pwm).await;
                state = FretState::Idle;
            }
            Message::Dampen(_, _) => {
                let _ = dampen(&mut pwm, state, signal).await;
                state = FretState::Dampening;
            }
            Message::FretCalibration(_, _) => {
                let _ = fret_calibration(&mut pwm, state, signal).await;
                state = FretState::Idle;
            }
            Message::FretAdaptive(_, _) => {
                let _ = fret_quiet(&mut pwm, state, signal).await;
                state = FretState::Fretting;
            }
            _ => {}
        }
    }
}

async fn send_confirm_presence(rs485: &mut Rs485) {
    let message = Message::ConfirmPresence;
    let mut bytes = [0; protocol::MAX_FRAME_SIZE];
    let frame = message.encode(&mut bytes).unwrap();
    if let Err(e) = rs485.write(frame).await {
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

async fn process_message(message: &Message, rs485: &mut Rs485) {
    defmt::info!("Incoming Message: {}", message);

    if !matches!(message, Message::Reset) && message.get_fret() != Some(MY_FRET) {
        return;
    }

    match message {
        Message::FretPresence(_) => send_confirm_presence(rs485).await,
        Message::FretFast(guitar_string, _)
        | Message::FretQuiet(guitar_string, _)
        | Message::Unfret(guitar_string, _)
        | Message::UnfretFast(guitar_string, _)
        | Message::Dampen(guitar_string, _)
        | Message::FretAdaptive(guitar_string, _)
        | Message::FretCalibration(guitar_string, _) => {
            get_fret_signal(*guitar_string).signal(*message);
        }
        Message::Reset => {
            for signal in FRET_SIGNALS.iter() {
                signal.signal(*message);
            }
        }
        Message::Config(_, _) => {}
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
