use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals::{TIM1, TIM14, TIM16, TIM17};
use embassy_stm32::time::khz;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel};
use embassy_stm32::usart::Uart;
use embassy_stm32::{Config, bind_interrupts, dma, peripherals, usart};

pub mod strings;
pub type Rs485 = Uart<'static, Async>;

bind_interrupts!(struct Irqs {
    USART2 => usart::InterruptHandler<peripherals::USART2>;
    DMA1_CHANNEL2_3 => dma::InterruptHandler<peripherals::DMA1_CH2>, dma::InterruptHandler<peripherals::DMA1_CH3>;
});

pub struct Hw {
    pub rs485: Rs485,
    pub ch_a: SimplePwmChannel<'static, TIM14>,
    pub ch_b: SimplePwmChannel<'static, TIM16>,
    pub ch_c: SimplePwmChannel<'static, TIM17>,
    pub ch_d: SimplePwmChannel<'static, TIM1>,
    pub ch_e: SimplePwmChannel<'static, TIM1>,
    pub ch_f: SimplePwmChannel<'static, TIM1>,
    pub led: Output<'static>,
}

pub fn init() -> Hw {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;

        // config.rcc.hse = Some(Hse {
        //     freq: Hertz(16_000_000),
        //     mode: HseMode::Oscillator,
        // });

        config.rcc.pll = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV1,
            mul: PllMul::MUL8,
            divp: None,
            divr: Some(PllRDiv::DIV2), // 16 / 1 * 8 / 2 = 64 Mhz,
            divq: None,
        });
        config.rcc.sys = Sysclk::PLL1_R;
    }

    let p = embassy_stm32::init(config);

    let mut config = usart::Config::default();
    config.baudrate = 115200;
    let usart = Uart::new_with_de(p.USART2, p.PA3, p.PA2, p.PA1, p.DMA1_CH2, p.DMA1_CH3, Irqs, config).unwrap();

    let pb3_pwm = PwmPin::new(p.PB3, OutputType::PushPull);
    let pa11_pwm = PwmPin::new(p.PA11, OutputType::PushPull);
    let pa8_pwm = PwmPin::new(p.PA8, OutputType::PushPull);
    let pa7_pwm = PwmPin::new(p.PA7, OutputType::PushPull);
    let pa6_pwm = PwmPin::new(p.PA6, OutputType::PushPull);
    let pa4_pwm = PwmPin::new(p.PA4, OutputType::PushPull);

    let t1 = SimplePwm::new(
        p.TIM1,
        Some(pa8_pwm),
        Some(pb3_pwm),
        None,
        Some(pa11_pwm),
        khz(20),
        Default::default(),
    );

    let t1ch1234 = t1.split();

    let t14 = SimplePwm::new(p.TIM14, Some(pa4_pwm), None, None, None, khz(20), Default::default());

    let t16 = SimplePwm::new(p.TIM16, Some(pa6_pwm), None, None, None, khz(20), Default::default());

    let t17 = SimplePwm::new(p.TIM17, Some(pa7_pwm), None, None, None, khz(20), Default::default());

    let mut ch_f = t1ch1234.ch2;
    let mut ch_e = t1ch1234.ch4;
    let mut ch_d = t1ch1234.ch1;
    let mut ch_c = t17.split().ch1;
    let mut ch_b = t16.split().ch1;
    let mut ch_a = t14.split().ch1;

    ch_f.set_duty_cycle_fully_off();
    ch_e.set_duty_cycle_fully_off();
    ch_d.set_duty_cycle_fully_off();
    ch_c.set_duty_cycle_fully_off();
    ch_b.set_duty_cycle_fully_off();
    ch_a.set_duty_cycle_fully_off();
    ch_f.enable();
    ch_e.enable();
    ch_d.enable();
    ch_c.enable();
    ch_b.enable();
    ch_a.enable();

    let led = Output::new(p.PB7, Level::Low, Speed::Low);

    Hw {
        rs485: usart,
        ch_f,
        ch_e,
        ch_d,
        ch_c,
        ch_b,
        ch_a,
        led,
    }
}
