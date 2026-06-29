#![no_std]
#![no_main]

pub mod hw;

use crate::hw::{PWM_CLOCK, PluckStepper, Rs485, VolumePwm};
use embassy_executor::Spawner;
use embassy_rp::pwm::SetDutyCycle;
use embassy_rp::uart;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use protocol::{Fret, GuitarString, MessageAction, MessageFrame, Parser, PluckVolume};

use {defmt_rtt as _, panic_probe as _};

static PLUCK_SIGNAL_1: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PLUCK_SIGNAL_2: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static PLUCK_SIGNAL_3: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static VOLUME_SIGNAL_1: Signal<CriticalSectionRawMutex, PluckVolume> = Signal::new();
static VOLUME_SIGNAL_2: Signal<CriticalSectionRawMutex, PluckVolume> = Signal::new();
static VOLUME_SIGNAL_3: Signal<CriticalSectionRawMutex, PluckVolume> = Signal::new();
static VARIANT: Variant = Variant::Left;

pub enum Variant {
    Left,
    Right,
}

// async fn rs485_test(mut uart: Uart<'_, Async>, mut de: Output<'_>) {
//     loop {
//         let mut buffer = [0u8; 1];
//         buffer[0] = 'a' as u8;

//         // de.set_high();
//         // Timer::after_micros(100).await;

//         // uart.write(&buffer).await.unwrap();
//         // uart.blocking_flush().unwrap();
//         // while uart.busy() {core::hint::spin_loop();}
//         // de.set_low();
//         //         Timer::after_secs(1).await;

//         //         continue;;

//         match uart.read_to_break(&mut buffer) {
//             Ok(n) => {
//                 defmt::info!("Rx: {}", buffer);
//                 de.set_high();
//                 Timer::after_micros(100).await;
//                 if let Err(e) = uart.write(&buffer).await {
//                     defmt::warn!("Tx error: {}", e);
//                 }

//                 if let Err(e) = uart.blocking_flush() {
//                     defmt::warn!("Flush error: {}", e);
//                 }

//                 while uart.busy() {core::hint::spin_loop();}

//                 Timer::after_micros(100).await;
//                 de.set_low();
//             }
//             Err(e) => {
//                 defmt::warn!("Rx error: {}", e);
//             }
//         }
//     }

// }

// async fn pick_test(hw: &mut Hw) {
//     let acc: u32 = 16_000_000;
//     defmt::info!("Acc: {}", acc);
//     let mut stepper = Stepgen::new(1_000_000);
//     stepper.set_acceleration(acc << 8).unwrap();
//     stepper.set_target_speed(20000 << 8).unwrap();
//     hw.channels[2].stepper.en.set_low();

//     loop {
//         stepper.set_target_step(stepper.target_step() + 800).unwrap();

//         let mut timestamp = Instant::now();

//         // defmt::info!("target: {}, current: {}, speed: {}", stepper.target_step(), stepper.current_step(), stepper.current_speed());

//         loop {
//             let Some(next) = stepper.next() else {
//                 break;
//             };

//             hw.channels[2].stepper.step.set_high();

//             let delay = (next + 128) >> 8;
//             // defmt::info!("delay [µs]: {}, speed: {}, target: {}, current: {}", delay, stepper.current_speed() >> 8, stepper.target_step(), stepper.current_step());
//             let high_phase = delay / 2;
//             let low_phase = delay - high_phase;

//             timestamp += Duration::from_micros(high_phase as u64);
//             Timer::at(timestamp).await;

//             hw.channels[2].stepper.step.set_low();
//             timestamp += Duration::from_micros(low_phase as u64);
//             Timer::at(timestamp).await;
//         }

//         // acc = acc * 6 / 5;
//             // hw.channels[2].stepper.step.toggle();
//             hw.channels[2].stepper.dir.toggle();
//         // Timer::after(Duration::from_millis(100)).await;
//     }

// }

fn is_string_relevant(string: GuitarString) -> bool {
    match VARIANT {
        Variant::Left => matches!(string, GuitarString::E | GuitarString::A | GuitarString::D),
        Variant::Right => matches!(string, GuitarString::G | GuitarString::B | GuitarString::e),
    }
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

async fn send_confirm_presence(request: &MessageFrame, rs485: &mut Rs485) {
    let message = MessageFrame::new(MessageAction::ConfirmPresence, request.string, request.fret, 0.into());
    let bytes = message.cobs_encode();
    if let Err(e) = send_bytes(&bytes, rs485).await {
        defmt::error!("TX error: {}", e);
    }
}

fn schedule_pluck(string: GuitarString) {
    match string {
        GuitarString::E => PLUCK_SIGNAL_1.signal(()),
        GuitarString::A => PLUCK_SIGNAL_2.signal(()),
        GuitarString::D => PLUCK_SIGNAL_3.signal(()),
        GuitarString::G => PLUCK_SIGNAL_1.signal(()),
        GuitarString::B => PLUCK_SIGNAL_2.signal(()),
        GuitarString::e => PLUCK_SIGNAL_3.signal(()),
    }
}

async fn set_volume(string: GuitarString, volume: PluckVolume) {
    match string {
        GuitarString::E => VOLUME_SIGNAL_1.signal(volume),
        GuitarString::A => VOLUME_SIGNAL_2.signal(volume),
        GuitarString::D => VOLUME_SIGNAL_3.signal(volume),
        GuitarString::G => VOLUME_SIGNAL_1.signal(volume),
        GuitarString::B => VOLUME_SIGNAL_2.signal(volume),
        GuitarString::e => VOLUME_SIGNAL_3.signal(volume),
    }
}

async fn process_message(message: &MessageFrame, rs485: &mut Rs485) {
    defmt::info!("Incoming Message: {}", message);

    if !is_string_relevant(message.string) {
        return;
    }

    match message.action {
        MessageAction::Presence if message.fret == Fret::NoFret => send_confirm_presence(message, rs485).await,
        MessageAction::Pluck => schedule_pluck(message.string),
        MessageAction::Volume => set_volume(message.string, message.pluck_volume).await,
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

#[embassy_executor::task(pool_size = 3)]
async fn stepper_task(mut stepper: PluckStepper, signal: &'static Signal<CriticalSectionRawMutex, ()>) -> ! {
    loop {
        signal.wait().await;
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn volume_task(mut pwm: VolumePwm, signal: &'static Signal<CriticalSectionRawMutex, PluckVolume>) -> ! {
    const PWM_MIDDLE_US: u32 = 1500;
    const PWM_RANGE_US: u32 = 500;

    let pwm_max_volume_us = match pwm.reversed {
        true => PWM_MIDDLE_US - PWM_RANGE_US,
        false => PWM_MIDDLE_US + PWM_RANGE_US,
    };

    let min_volume_count = PWM_CLOCK * (PWM_MIDDLE_US / 100) / (1_000_000 / 100);
    let max_volume_count = PWM_CLOCK * (pwm_max_volume_us / 100) / (1_000_000 / 100);

    loop {
        let volume = signal.wait().await;

        let volume_count = (min_volume_count * (PluckVolume::max_volume() + 1 - volume.volume()) as u32
            + max_volume_count * volume.volume() as u32)
            / (PluckVolume::max_volume() + 1) as u32;

        pwm.servo_pwm.set_duty_cycle(volume_count as u16).unwrap();
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let hw = hw::init();

    // rs485_test(uart, de).await;
    // pick_test(&mut hw).await;

    spawner.spawn(defmt::unwrap!(rs485_task(hw.rs485)));
    spawner.spawn(defmt::unwrap!(stepper_task(hw.channel_1.stepper, &PLUCK_SIGNAL_1)));
    spawner.spawn(defmt::unwrap!(stepper_task(hw.channel_2.stepper, &PLUCK_SIGNAL_2)));
    spawner.spawn(defmt::unwrap!(stepper_task(hw.channel_3.stepper, &PLUCK_SIGNAL_3)));
    spawner.spawn(defmt::unwrap!(volume_task(hw.channel_1.volume_pwm, &VOLUME_SIGNAL_1)));
    spawner.spawn(defmt::unwrap!(volume_task(hw.channel_2.volume_pwm, &VOLUME_SIGNAL_2)));
    spawner.spawn(defmt::unwrap!(volume_task(hw.channel_3.volume_pwm, &VOLUME_SIGNAL_3)));

    // let mut config = pwm::Config::default();
    // let divider = 8;
    // config.divider = divider.to_fixed();
    // config.top = 65535;
    // let mut pwm = Pwm::new_output_a(p.PWM_SLICE0, p.PIN_16, config);

    // const PWM_LOW_US: u32 = 1000;
    // const PWM_HIGH_US: u32 = 2000;

    // let mid_count = (sys_clk / 1_000_000 * PWM_MIDDLE_US / divider) as u16;
    // let low_count = (sys_clk / 1_000_000 * PWM_LOW_US / divider) as u16;
    // let high_count = (sys_clk / 1_000_000 * PWM_HIGH_US / divider) as u16;

    // loop {
    //     pwm.set_duty_cycle(mid_count).unwrap();
    //     Timer::after_secs(1).await;
    //     pwm.set_duty_cycle(low_count).unwrap();
    //     Timer::after_secs(1).await;
    // }
}
