use core::convert::Infallible;

use embassy_stm32::peripherals::{TIM1, TIM14, TIM16, TIM17};
use embassy_stm32::timer::simple_pwm::SimplePwmChannel;
use embedded_hal::pwm::{ErrorType, SetDutyCycle};

pub enum GuitarStringPwm {
    ChA(SimplePwmChannel<'static, TIM14>),
    ChB(SimplePwmChannel<'static, TIM16>),
    ChC(SimplePwmChannel<'static, TIM17>),
    ChD(SimplePwmChannel<'static, TIM1>),
    ChE(SimplePwmChannel<'static, TIM1>),
    ChF(SimplePwmChannel<'static, TIM1>),
}

impl ErrorType for GuitarStringPwm {
    type Error = Infallible;
}

impl GuitarStringPwm {
    pub fn current_duty_cycle(&self) -> u16 {
        match self {
            GuitarStringPwm::ChA(pwm) => pwm.current_duty_cycle(),
            GuitarStringPwm::ChB(pwm) => pwm.current_duty_cycle(),
            GuitarStringPwm::ChC(pwm) => pwm.current_duty_cycle(),
            GuitarStringPwm::ChD(pwm) => pwm.current_duty_cycle(),
            GuitarStringPwm::ChE(pwm) => pwm.current_duty_cycle(),
            GuitarStringPwm::ChF(pwm) => pwm.current_duty_cycle(),
        }
    }
}

impl SetDutyCycle for GuitarStringPwm {
    fn max_duty_cycle(&self) -> u16 {
        match self {
            GuitarStringPwm::ChA(pwm) => SetDutyCycle::max_duty_cycle(pwm),
            GuitarStringPwm::ChB(pwm) => SetDutyCycle::max_duty_cycle(pwm),
            GuitarStringPwm::ChC(pwm) => SetDutyCycle::max_duty_cycle(pwm),
            GuitarStringPwm::ChD(pwm) => SetDutyCycle::max_duty_cycle(pwm),
            GuitarStringPwm::ChE(pwm) => SetDutyCycle::max_duty_cycle(pwm),
            GuitarStringPwm::ChF(pwm) => SetDutyCycle::max_duty_cycle(pwm),
        }
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Self::Error> {
        match self {
            GuitarStringPwm::ChA(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
            GuitarStringPwm::ChB(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
            GuitarStringPwm::ChC(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
            GuitarStringPwm::ChD(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
            GuitarStringPwm::ChE(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
            GuitarStringPwm::ChF(pwm) => SetDutyCycle::set_duty_cycle(pwm, duty),
        }
    }
}
