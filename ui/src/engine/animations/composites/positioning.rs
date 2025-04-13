use crate::engine::{
    animations::{
        alert_v2::{Alert, SquarePulseTrain},
        Static,
    },
    Animation, AnimationState, RingFrame,
};
use orb_rgb::Argb;
use std::time::Duration;

pub struct Positioning<const N: usize> {
    state: State<N>,
    alert_period: f64,
    error_color: Argb,
    in_range: bool,
    initial_delay: f64,
}

enum State<const N: usize> {
    Rising { animation: Alert<N> },
    StaticOn { animation: Static<N> },
    Alert { animation: Alert<N> },
    Falling { animation: Alert<N> },
    StaticOff { animation: Static<N> },
}

impl<const N: usize> Positioning<N> {
    pub fn new(error_color: Argb, alert_period: Duration) -> Self {
        Self {
            state: State::StaticOff {
                animation: Static::<N>::new(Argb::OFF, None),
            },
            error_color,
            alert_period: alert_period.as_secs_f64(),
            in_range: true,
            initial_delay: 0.0,
        }
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay.as_secs_f64();
        self
    }

    pub fn set_in_range(&mut self, in_range: bool) {
        if in_range != self.in_range {
            self.in_range = in_range;
        }
    }

    fn alert_animation(&self) -> Alert<N> {
        Alert::<N>::new(
            self.error_color,
            SquarePulseTrain::from(vec![
                (0.0, 0.0),
                (0.0, 0.3),
                (0.3, 0.3),
                (0.6, 0.3),
                (0.9, 0.3),
            ]),
        )
        .unwrap()
    }

    fn static_on(&self) -> Static<N> {
        Static::<N>::new(self.error_color, Some(self.alert_period))
    }

    fn static_off(&self) -> Static<N> {
        Static::<N>::new(Argb::OFF, None)
    }

    fn falling(&self) -> Alert<N> {
        Alert::<N>::new(
            self.error_color,
            SquarePulseTrain::from(vec![(0.0, 0.0), (0.0, 1.0)]),
        )
        .unwrap()
    }

    fn rising(&self) -> Alert<N> {
        Alert::<N>::new(self.error_color, SquarePulseTrain::from(vec![(0.0, 1.0)]))
            .unwrap()
    }
}

impl<const N: usize> Animation for Positioning<N> {
    type Frame = RingFrame<N>;
    fn animate(
        &mut self,
        frame: &mut Self::Frame,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        if self.initial_delay > 0.0 {
            self.initial_delay -= dt;
            return AnimationState::Running;
        }
        match &mut self.state {
            State::Rising { animation } => {
                if animation.animate(frame, dt, idle) == AnimationState::Finished {
                    self.state = State::StaticOn {
                        animation: self.static_on(),
                    };
                }
            }
            State::StaticOn { animation } => {
                if self.in_range {
                    self.state = State::Falling {
                        animation: self.falling(),
                    }
                } else if animation.animate(frame, dt, idle) == AnimationState::Finished
                {
                    self.state = State::Alert {
                        animation: self.alert_animation(),
                    };
                }
            }
            State::Alert { animation } => {
                if animation.animate(frame, dt, idle) == AnimationState::Finished {
                    self.state = State::StaticOn {
                        animation: self.static_on(),
                    }
                }
            }
            State::Falling { animation } => {
                if animation.animate(frame, dt, idle) == AnimationState::Finished {
                    self.state = State::StaticOff {
                        animation: self.static_off(),
                    };
                }
            }
            State::StaticOff { animation } => {
                if !self.in_range {
                    self.state = State::Rising {
                        animation: self.rising(),
                    }
                } else {
                    animation.animate(frame, dt, idle);
                }
            }
        }
        AnimationState::Running
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn stop(&mut self, _transition: crate::engine::Transition) -> eyre::Result<()> {
        Ok(())
    }
}
