use embassy_rp::{
    self, bind_interrupts,
    clocks::ClockConfig,
    config, dma,
    gpio::{
        Level::{High, Low},
        Output,
    },
    peripherals::{DMA_CH0, DMA_CH1, UART1},
    pwm::{self, Pwm, PwmOutput},
    uart::{self, BufferedUart, Uart},
};
use fixed::traits::ToFixed;

bind_interrupts!(pub struct Irqs {
    UART1_IRQ  => uart::InterruptHandler<UART1>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
});

pub const SYS_CLOCK: u32 = 125_000_000;
pub const PWM_DIVIDER: u32 = 8;
pub const PWM_CLOCK: u32 = SYS_CLOCK / PWM_DIVIDER;

pub struct Hw {
    pub channel_1: Channel,
    pub channel_2: Channel,
    pub channel_3: Channel,
    pub stepper_ms1: Output<'static>,
    pub stepper_ms2: Output<'static>,
    pub rs485: Rs485,
}

pub struct Channel {
    pub stepper: PluckStepper,
    pub volume_pwm: VolumePwm,
}

pub struct PluckStepper {
    pub dir: Output<'static>,
    pub step: Output<'static>,
    pub en: Output<'static>,
    pub reversed: bool,
}

pub struct VolumePwm {
    pub servo_pwm: PwmOutput<'static>,
    pub reversed: bool,
}

pub struct Rs485 {
    pub uart: Uart<'static, uart::Async>,
    pub de: Output<'static>,
}

pub fn init() -> Hw {
    let clocks = ClockConfig::system_freq(SYS_CLOCK).unwrap();
    let config = config::Config::new(clocks);
    let p = embassy_rp::init(config);

    let rs485 = {
        let mut config = uart::Config::default();
        config.baudrate = 115200;

        let uart = Uart::new(p.UART1, p.PIN_20, p.PIN_21, Irqs, p.DMA_CH0, p.DMA_CH1, config);

        let de = Output::new(p.PIN_22, Low);

        Rs485 { uart, de }
    };

    let mut config = pwm::Config::default();
    config.divider = PWM_DIVIDER.to_fixed();
    config.top = 65535;
    let pwm_0 = Pwm::new_output_ab(p.PWM_SLICE0, p.PIN_16, p.PIN_17, config).split();

    let mut config = pwm::Config::default();
    config.divider = 8i32.to_fixed();
    config.top = 65535;
    let pwm_1 = Pwm::new_output_a(p.PWM_SLICE1, p.PIN_18, config).split();

    let channel_1 = {
        let stepper = {
            let step = Output::new(p.PIN_14, Low);
            let dir = Output::new(p.PIN_11, Low);
            let en = Output::new(p.PIN_15, High);
            PluckStepper {
                step,
                dir,
                en,
                reversed: false,
            }
        };

        let servo_pwm = pwm_1.0.unwrap();
        let volume_pwm = VolumePwm {
            servo_pwm,
            reversed: false,
        };

        Channel { stepper, volume_pwm }
    };

    let channel_2 = {
        let stepper = {
            let step = Output::new(p.PIN_6, Low);
            let dir = Output::new(p.PIN_5, Low);
            let en = Output::new(p.PIN_10, High);
            PluckStepper {
                step,
                dir,
                en,
                reversed: false,
            }
        };

        let servo_pwm = pwm_0.1.unwrap();
        let volume_pwm = VolumePwm {
            servo_pwm,
            reversed: false,
        };

        Channel { stepper, volume_pwm }
    };

    let channel_3 = {
        let stepper = {
            let step = Output::new(p.PIN_3, Low);
            let dir = Output::new(p.PIN_2, Low);
            let en = Output::new(p.PIN_4, High);
            PluckStepper {
                step,
                dir,
                en,
                reversed: false,
            }
        };

        let servo_pwm = pwm_0.0.unwrap();
        let volume_pwm = VolumePwm {
            servo_pwm,
            reversed: false,
        };

        Channel { stepper, volume_pwm }
    };

    // MS1=high, MS2=high -> 16 microsteps
    let stepper_ms1 = Output::new(p.PIN_1, High);
    let stepper_ms2 = Output::new(p.PIN_0, High);

    Hw {
        channel_1,
        channel_2,
        channel_3,
        stepper_ms1,
        stepper_ms2,
        rs485,
    }
}
