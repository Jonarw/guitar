#![no_std]
#![no_main]

use crate::hw::{Rs485, strings::*};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::pwm::SetDutyCycle;
use embedded_io_async::Read;
use embedded_io_async::Write;
use protocol::{ConfigValue, Fret, GuitarString, Message, Parser, Percentage};

use {defmt_rtt as _, panic_probe as _};

pub mod hw;

pub static MY_FRET: Fret = Fret::Fret12;
type FretSignal = Signal<ThreadModeRawMutex, Message>;

static FRET_SIGNALS: [FretSignal; 6] = [
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
    Signal::new(),
];

static CONFIG: Mutex<ThreadModeRawMutex, Config> = Mutex::new(Config::default());

struct Config {
    pwm_max_force: Percentage,
    pwm_hold_force: Percentage,
    pwm_dampen_force: Percentage,
    pwm_marginal_force: Percentage,
    release_break_off_time: Duration,
    dampen_to_fret_ramp_duration: Duration,
    dampen_phase1_duration: Duration,
    fret_fast_phase1_duration: Duration,
    fret_fast_phase2_duration: Duration,
    fret_quiet_phase1_duration: Duration,
    fret_quiet_phase2_duration: Duration,
    unfret_phase1_duration: Duration,
    unfret_phase2_duration: Duration,
    fret_adaptive_phase1_duration: Duration,
    fret_adaptive_phase2_duration: Duration,
    fret_adaptive_phase3_duration: Duration,
    fret_adaptive_phase1_force: Percentage,
    fret_adaptive_phase2_force: Percentage,
    fret_adaptive_phase3_force: Percentage,
    fret_calibration_offset1: u8,
    fret_calibration_offset2: u8,
    fret_calibration_duration: Duration,
    idle_to_down_phase1_duration: Duration,
    idle_to_down_phase2_duration: Duration,
    idle_to_down_phase1_force: Percentage,
    idle_to_down_phase2_force: Percentage,
    down_to_idle_phase1_duration: Duration,
    down_to_idle_phase2_duration: Duration,
    down_to_idle_phase1_force: Percentage,
    down_to_idle_phase2_force: Percentage,
}

impl Config {
    pub const fn default() -> Self {
        Self {
            pwm_max_force: Percentage::new(100),
            pwm_hold_force: Percentage::new(45),
            pwm_dampen_force: Percentage::new(24),
            pwm_marginal_force: Percentage::new(13),
            release_break_off_time: Duration::from_millis(1),
            dampen_to_fret_ramp_duration: Duration::from_millis(100),
            dampen_phase1_duration: Duration::from_millis(100),
            fret_fast_phase1_duration: Duration::from_millis(5),
            fret_fast_phase2_duration: Duration::from_millis(100),
            fret_quiet_phase1_duration: Duration::from_millis(50),
            fret_quiet_phase2_duration: Duration::from_millis(100),
            unfret_phase1_duration: Duration::from_millis(500),
            unfret_phase2_duration: Duration::from_millis(100),
            fret_adaptive_phase1_duration: Duration::from_millis(10),
            fret_adaptive_phase2_duration: Duration::from_millis(10),
            fret_adaptive_phase3_duration: Duration::from_millis(100),
            fret_adaptive_phase1_force: Percentage::new(100),
            fret_adaptive_phase2_force: Percentage::new(0),
            fret_adaptive_phase3_force: Percentage::new(80),
            fret_calibration_offset1: 10,
            fret_calibration_offset2: 15,
            fret_calibration_duration: Duration::from_millis(1000),
            idle_to_down_phase1_duration: Duration::from_millis(4),
            idle_to_down_phase2_duration: Duration::from_millis(10),
            idle_to_down_phase1_force: Percentage::new(100),
            idle_to_down_phase2_force: Percentage::new(0),
            down_to_idle_phase1_duration: Duration::from_millis(20),
            down_to_idle_phase2_duration: Duration::from_millis(2),
            down_to_idle_phase1_force: Percentage::new(0),
            down_to_idle_phase2_force: Percentage::new(100),
        }
    }
}

async fn pwm_ramp(
    pwm: &mut GuitarStringPwm,
    start: u8,
    end: u8,
    duration: Duration,
    signal: Option<&'static FretSignal>,
) {
    let duration_per_percent = duration / end.abs_diff(start) as u32;

    let mut time = Instant::now();
    if start < end {
        for i in start..end {
            if signal.map_or(false, |s| s.signaled()) {
                break;
            }

            pwm.set_duty_cycle_percent(i);
            time += duration_per_percent;
            Timer::at(time).await;
        }
    } else {
        for i in ((end + 1)..start).rev() {
            if signal.map_or(false, |s| s.signaled()) {
                break;
            }

            pwm.set_duty_cycle_percent(i);
            time += duration_per_percent;
            Timer::at(time).await;
        }
    }

    pwm.set_duty_cycle_percent(end);
}

async fn idle_to_down(pwm: &mut GuitarStringPwm) {
    let (
        idle_to_down_phase1_force,
        idle_to_down_phase1_duration,
        idle_to_down_phase2_force,
        idle_to_down_phase2_duration,
        pwm_dampen_force,
    ) = {
        let config = CONFIG.lock().await;
        (
            config.idle_to_down_phase1_force,
            config.idle_to_down_phase1_duration,
            config.idle_to_down_phase2_force,
            config.idle_to_down_phase2_duration,
            config.pwm_dampen_force,
        )
    };

    pwm.set_duty_cycle_percent(idle_to_down_phase1_force.into());
    Timer::after(idle_to_down_phase1_duration).await;
    pwm.set_duty_cycle_percent(idle_to_down_phase2_force.into());
    Timer::after(idle_to_down_phase2_duration).await;
    pwm.set_duty_cycle_percent(pwm_dampen_force.into());
}

async fn down_to_idle(pwm: &mut GuitarStringPwm) {
    let (
        down_to_idle_phase1_force,
        down_to_idle_phase1_duration,
        down_to_idle_phase2_force,
        down_to_idle_phase2_duration,
    ) = {
        let config = CONFIG.lock().await;
        (
            config.down_to_idle_phase1_force,
            config.down_to_idle_phase1_duration,
            config.down_to_idle_phase2_force,
            config.down_to_idle_phase2_duration,
        )
    };

    pwm.set_duty_cycle_percent(down_to_idle_phase1_force.into());
    Timer::after(down_to_idle_phase1_duration).await;
    pwm.set_duty_cycle_percent(down_to_idle_phase2_force.into());
    Timer::after(down_to_idle_phase2_duration).await;
    pwm.set_duty_cycle_fully_off();
}

async fn fret_fast(pwm: &mut GuitarStringPwm, state: FretState) {
    let (pwm_max_force, fret_fast_phase1_duration, pwm_hold_force, fret_fast_phase2_duration) = {
        let config = CONFIG.lock().await;
        (
            config.pwm_max_force,
            config.fret_fast_phase1_duration,
            config.pwm_hold_force,
            config.fret_fast_phase2_duration,
        )
    };

    match state {
        FretState::Idle => {
            pwm.set_duty_cycle_percent(pwm_max_force.into());
            Timer::after(fret_fast_phase1_duration).await;
            pwm.set_duty_cycle_percent(pwm_hold_force.into());
        }
        FretState::Fretting => {}
        FretState::Dampening => {
            pwm.set_duty_cycle_percent(pwm_max_force.into());
            Timer::after(fret_fast_phase2_duration).await;
            pwm.set_duty_cycle_percent(pwm_hold_force.into());
        }
    }
}

async fn fret_calibration(pwm: &mut GuitarStringPwm, state: FretState) {
    fret_quiet(pwm, state).await;
    let (pwm_hold_force, fret_calibration_offset1, fret_calibration_duration, fret_calibration_offset2) = {
        let config = CONFIG.lock().await;
        (
            config.pwm_hold_force,
            config.fret_calibration_offset1,
            config.fret_calibration_duration,
            config.fret_calibration_offset2,
        )
    };

    pwm.set_duty_cycle_percent(pwm_hold_force.get_value() - fret_calibration_offset1);
    Timer::after(fret_calibration_duration).await;
    pwm.set_duty_cycle_percent(pwm_hold_force.get_value() - fret_calibration_offset2);
    Timer::after(fret_calibration_duration).await;
    pwm.set_duty_cycle_fully_off();
}

async fn fret_adaptive(pwm: &mut GuitarStringPwm, state: FretState) {
    let (
        fret_adaptive_phase1_force,
        fret_adaptive_phase1_duration,
        fret_adaptive_phase2_force,
        fret_adaptive_phase2_duration,
        fret_adaptive_phase3_force,
        fret_adaptive_phase3_duration,
    ) = {
        let config = CONFIG.lock().await;
        (
            config.fret_adaptive_phase1_force,
            config.fret_adaptive_phase1_duration,
            config.fret_adaptive_phase2_force,
            config.fret_adaptive_phase2_duration,
            config.fret_adaptive_phase3_force,
            config.fret_adaptive_phase3_duration,
        )
    };

    pwm.set_duty_cycle_percent(fret_adaptive_phase1_force.get_value());
    Timer::after(fret_adaptive_phase1_duration).await;
    pwm.set_duty_cycle_percent(fret_adaptive_phase2_force.get_value());
    Timer::after(fret_adaptive_phase2_duration).await;
    pwm.set_duty_cycle_percent(fret_adaptive_phase3_force.get_value());
    Timer::after(fret_adaptive_phase3_duration).await;
}

async fn fret_quiet(pwm: &mut GuitarStringPwm, state: FretState) {
    let (pwm_dampen_force, pwm_max_force, fret_quiet_phase2_duration, pwm_hold_force) = {
        let config = CONFIG.lock().await;
        (
            config.pwm_dampen_force,
            config.pwm_max_force,
            config.fret_quiet_phase2_duration,
            config.pwm_hold_force,
        )
    };

    match state {
        FretState::Idle => {
            idle_to_down(pwm).await;
            pwm_ramp(
                pwm,
                pwm_dampen_force.into(),
                pwm_max_force.into(),
                fret_quiet_phase2_duration,
                None,
            )
            .await;
        }
        FretState::Fretting => return,
        FretState::Dampening => {
            pwm_ramp(
                pwm,
                pwm_dampen_force.into(),
                pwm_max_force.into(),
                fret_quiet_phase2_duration,
                None,
            )
            .await;
        }
    }

    pwm.set_duty_cycle_percent(pwm_hold_force.into());
}

async fn dampen(pwm: &mut GuitarStringPwm, state: FretState) {
    let (pwm_dampen_force, release_break_off_time) = {
        let config = CONFIG.lock().await;
        (config.pwm_dampen_force, config.release_break_off_time)
    };

    match state {
        FretState::Idle => {
            idle_to_down(pwm).await;
            pwm.set_duty_cycle_percent(pwm_dampen_force.into());
        }
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            Timer::after(release_break_off_time).await;
            pwm.set_duty_cycle_percent(pwm_dampen_force.into());
        }
        FretState::Dampening => {}
    }
}

async fn unfret(pwm: &mut GuitarStringPwm, state: FretState, signal: &'static FretSignal) {
    let (release_break_off_time, unfret_phase1_duration, pwm_dampen_force) = {
        let config = CONFIG.lock().await;
        (
            config.release_break_off_time,
            config.unfret_phase1_duration,
            config.pwm_dampen_force,
        )
    };

    match state {
        FretState::Idle => {}
        FretState::Fretting => {
            pwm.set_duty_cycle_fully_off();
            Timer::after(release_break_off_time).await;
            pwm_ramp(pwm, pwm_dampen_force.into(), 3, unfret_phase1_duration, Some(signal)).await
        }
        FretState::Dampening => pwm_ramp(pwm, pwm_dampen_force.into(), 3, unfret_phase1_duration, Some(signal)).await,
    }

    pwm.set_duty_cycle_fully_off();
}

async fn unfret_fast(pwm: &mut GuitarStringPwm, state: FretState) {
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
                fret_fast(&mut pwm, state).await;
                state = FretState::Fretting;
            }
            Message::FretQuiet(_, _) => {
                fret_quiet(&mut pwm, state).await;
                state = FretState::Fretting;
            }
            Message::Unfret(_, _) => {
                unfret(&mut pwm, state, signal).await;
                state = FretState::Idle;
            }
            Message::UnfretFast(_, _) | Message::Reset => {
                unfret_fast(&mut pwm, state).await;
                state = FretState::Idle;
            }
            Message::Dampen(_, _) => {
                dampen(&mut pwm, state).await;
                state = FretState::Dampening;
            }
            Message::FretCalibration(_, _) => {
                fret_calibration(&mut pwm, state).await;
                state = FretState::Idle;
            }
            Message::FretAdaptive(_, _) => {
                fret_adaptive(&mut pwm, state).await;
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

fn convert_duration(duration: protocol::Duration) -> Duration {
    Duration::from_millis(duration.get_value_ms() as u64)
}

async fn apply_config(value: &ConfigValue) {
    let mut config = CONFIG.lock().await;
    match value {
        ConfigValue::MaxForce(percentage) => config.pwm_max_force = *percentage,
        ConfigValue::HoldForce(percentage) => config.pwm_hold_force = *percentage,
        ConfigValue::DampenForce(percentage) => config.pwm_dampen_force = *percentage,
        ConfigValue::MarginalForce(percentage) => config.pwm_marginal_force = *percentage,
        ConfigValue::ReleaseDuration(duration) => config.release_break_off_time = convert_duration(*duration),
        ConfigValue::DampenToFretRampDuration(duration) => {
            config.dampen_to_fret_ramp_duration = convert_duration(*duration)
        }
        ConfigValue::FretFastMaxForceDuration(duration) => {
            config.fret_fast_phase1_duration = convert_duration(*duration)
        }
        ConfigValue::FretQuietPhase1Duration(duration) => {
            config.fret_quiet_phase1_duration = convert_duration(*duration)
        }
        ConfigValue::FretQuietPhase2Duration(duration) => {
            config.fret_quiet_phase2_duration = convert_duration(*duration)
        }
        ConfigValue::FretAdaptivePhase1Duration(duration) => {
            config.fret_adaptive_phase1_duration = convert_duration(*duration)
        }
        ConfigValue::FretAdaptivePhase2Duration(duration) => {
            config.fret_adaptive_phase2_duration = convert_duration(*duration)
        }
        ConfigValue::FretAdaptivePhase3Durtaion(duration) => {
            config.fret_adaptive_phase3_duration = convert_duration(*duration)
        }
        ConfigValue::FretAdaptivePhase1Force(percentage) => config.fret_adaptive_phase1_force = *percentage,
        ConfigValue::FretAdaptivePhase2Force(percentage) => config.fret_adaptive_phase2_force = *percentage,
        ConfigValue::FretAdaptivePhase3Force(percentage) => config.fret_adaptive_phase3_force = *percentage,
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
        Message::Config(_, config_value) => apply_config(config_value).await,
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
