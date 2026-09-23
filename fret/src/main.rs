#![no_std]
#![no_main]

use crate::hw::{Rs485, strings::*};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::pwm::SetDutyCycle;
use embedded_io_async::Read;
use embedded_io_async::Write;
use protocol::{Fret, GuitarString, Message, Parser};

use {defmt_rtt as _, panic_probe as _};

pub mod hw;

#[cfg(feature = "fret1")]
pub const MY_FRET: Fret = Fret::Fret1;
#[cfg(feature = "fret2")]
pub const MY_FRET: Fret = Fret::Fret2;
#[cfg(feature = "fret3")]
pub const MY_FRET: Fret = Fret::Fret3;
#[cfg(feature = "fret4")]
pub const MY_FRET: Fret = Fret::Fret4;
#[cfg(feature = "fret5")]
pub const MY_FRET: Fret = Fret::Fret5;
#[cfg(feature = "fret6")]
pub const MY_FRET: Fret = Fret::Fret6;
#[cfg(feature = "fret7")]
pub const MY_FRET: Fret = Fret::Fret7;
#[cfg(feature = "fret8")]
pub const MY_FRET: Fret = Fret::Fret8;
#[cfg(feature = "fret9")]
pub const MY_FRET: Fret = Fret::Fret9;
#[cfg(feature = "fret10")]
pub const MY_FRET: Fret = Fret::Fret10;
#[cfg(feature = "fret11")]
pub const MY_FRET: Fret = Fret::Fret11;
#[cfg(feature = "fret12")]
pub const MY_FRET: Fret = Fret::Fret12;
#[cfg(feature = "fret13")]
pub const MY_FRET: Fret = Fret::Fret13;

type FretSignal = Signal<ThreadModeRawMutex, Message>;

static FRET_SIGNALS: [FretSignal; 6] = [
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
];

const PWM_DAMPEN_FORCE: u8 = 24;
const PWM_HOLD_FORCE: u8 = 45;
const PWM_MAX_FORCE: u8 = 100;
const RELEASE_BREAK_OFF_MS: u64 = 1;

// this is not an actual error, we just use Result<(), Interrupted> as a convenient way to abort the current
// operation when we are interrupted
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
    pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
    wait_millis_interruptible(4, signal).await?;
    pwm.set_duty_cycle_fully_off();
    wait_millis_interruptible(10, signal).await?;
    pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
    Ok(())
}

async fn fret_fast(
    pwm: &mut GuitarStringPwm,
    state: FretState,
    signal: &'static FretSignal,
) -> Result<(), Interrupted> {
    match state {
        FretState::Idle => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
            wait_millis_interruptible(5, signal).await?;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
        }
        FretState::Fretting => {}
        FretState::Dampening => {
            pwm.set_duty_cycle_percent(PWM_MAX_FORCE);
            wait_millis_interruptible(4, signal).await?;
            pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
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
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE - 10);
    wait_millis_interruptible(CALIBRATION_DURATION_MS, signal).await?;
    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE - 15);
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
            pwm_ramp(pwm, PWM_DAMPEN_FORCE, PWM_MAX_FORCE, ramp_duration, signal).await?;
        }
        FretState::Fretting => return Ok(()),
        FretState::Dampening => {
            pwm_ramp(pwm, PWM_DAMPEN_FORCE, PWM_MAX_FORCE, ramp_duration, signal).await?;
        }
    }

    pwm.set_duty_cycle_percent(PWM_HOLD_FORCE);
    Ok(())
}

async fn dampen(pwm: &mut GuitarStringPwm, state: FretState, signal: &'static FretSignal) -> Result<(), Interrupted> {
    match state {
        FretState::Idle => {
            idle_to_down(pwm, signal).await?;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
        }
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            wait_millis_interruptible(RELEASE_BREAK_OFF_MS, signal).await?;
            pwm.set_duty_cycle_percent(PWM_DAMPEN_FORCE);
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
            pwm_ramp(pwm, PWM_DAMPEN_FORCE, 3, ramp_duration, signal).await?;
        }
        FretState::Dampening => pwm_ramp(pwm, PWM_DAMPEN_FORCE, 3, ramp_duration, signal).await?,
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

fn get_fret_signal(fret: Fret, string: GuitarString) -> &'static FretSignal {
    if MY_FRET == Fret::Fret13 {
        match fret {
            Fret::Fret13 => &FRET_SIGNALS[5],
            Fret::Fret14 => &FRET_SIGNALS[4],
            Fret::Fret15 => &FRET_SIGNALS[3],
            Fret::Fret16 => &FRET_SIGNALS[2],
            Fret::Fret17 => &FRET_SIGNALS[1],
            Fret::Fret18 => &FRET_SIGNALS[0],
            _ => defmt::panic!("Invalid Fret"),
        }
    } else {
        match string {
            GuitarString::E => &FRET_SIGNALS[5],
            GuitarString::A => &FRET_SIGNALS[0],
            GuitarString::D => &FRET_SIGNALS[4],
            GuitarString::G => &FRET_SIGNALS[1],
            GuitarString::B => &FRET_SIGNALS[3],
            GuitarString::e => &FRET_SIGNALS[2],
        }
    }
}

fn is_message_relevant(message: &Message) -> bool {
    if *message == Message::Reset {
        return true;
    }

    if MY_FRET == Fret::Fret13 {
        message.get_string() == Some(GuitarString::e)
            && message.get_fret().is_some_and(|fret| {
                matches!(
                    fret,
                    Fret::Fret13 | Fret::Fret14 | Fret::Fret15 | Fret::Fret16 | Fret::Fret17 | Fret::Fret18
                )
            })
    } else {
        message.get_fret().is_some_and(|fret| fret == MY_FRET)
    }
}

async fn process_message(message: &Message, rs485: &mut Rs485) {
    defmt::info!("Incoming Message: {}", message);

    if !is_message_relevant(message) {
        return;
    }

    match message {
        Message::FretPresence(_) => send_confirm_presence(rs485).await,
        Message::FretFast(guitar_string, fret)
        | Message::FretQuiet(guitar_string, fret)
        | Message::Unfret(guitar_string, fret)
        | Message::UnfretFast(guitar_string, fret)
        | Message::Dampen(guitar_string, fret)
        | Message::FretCalibration(guitar_string, fret) => {
            get_fret_signal(*fret, *guitar_string).signal(*message);
        }
        Message::Reset => {
            for signal in FRET_SIGNALS.iter() {
                signal.signal(*message);
            }
        }
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
