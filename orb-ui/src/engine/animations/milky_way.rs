use crate::engine::{Animation, Transition};
use crate::engine::{AnimationState, RingFrame};
use eyre::eyre;
use orb_rgb::Argb;
use std::any::Any;
use std::f64::consts::PI;

/// Milky Way animation.
/// The animation is a randomized ring of LEDs, where each LED is a different color.
pub struct MilkyWay<const N: usize> {
    phase: f64,
    frame: RingFrame<N>,
    config: MilkyWayConfig,
    transition: Option<Transition>,
    transition_time: f64,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct MilkyWayConfig {
    /// initial background color from which the randomized ring is generated
    /// color oscillates between this and background + *_delta values
    pub background: Argb,
    /// maximum delta in colors between two animated frames
    pub fade_delta: i16,
    /// delta in colors to generate the first frame
    pub initial_delta: i16,
    /// minimum and maximum values for each red channel
    pub red_min_max: (u8, u8),
    /// minimum and maximum values for each green channel
    pub green_min_max: (u8, u8),
    /// minimum and maximum values for each blue channel
    pub blue_min_max: (u8, u8),
}

impl MilkyWayConfig {
    pub fn default() -> Self {
        MilkyWayConfig {
            background: Argb(Some(10), 30, 20, 5),
            fade_delta: 2,
            initial_delta: 5,
            red_min_max: (10, 40),
            green_min_max: (1, 40),
            blue_min_max: (1, 10),
        }
    }
}

fn rand_delta(delta_max: i16) -> i16 {
    let sign = if rand::random::<i16>() % 2 == 0 {
        1
    } else {
        -1
    };
    (rand::random::<i16>() % delta_max) * sign
}

fn generate_random(frame: &mut [Argb], config: &MilkyWayConfig) {
    let new_color = |config: &MilkyWayConfig| {
        Argb(
            config.background.0,
            ((config.background.1 as i16 + rand_delta(config.initial_delta)) as u8)
                .clamp(config.red_min_max.0, config.red_min_max.1),
            ((config.background.2 as i16 + rand_delta(config.initial_delta)) as u8)
                .clamp(config.green_min_max.0, config.green_min_max.1),
            ((config.background.3 as i16 + rand_delta(config.initial_delta)) as u8)
                .clamp(config.blue_min_max.0, config.blue_min_max.1),
        )
    };

    let mut c = new_color(config);
    for (i, led) in frame.iter_mut().enumerate() {
        if i % 2 == 0 {
            c = new_color(config);
        }
        *led = c;
    }
}

impl<const N: usize> MilkyWay<N> {
    /// Create idle ring
    #[expect(dead_code)]
    #[must_use]
    pub fn new(config: MilkyWayConfig) -> Self {
        // generate initial randomized frame
        let mut frame: [Argb; N] = [config.background; N];
        generate_random(&mut frame, &config);

        Self {
            phase: 0.0,
            transition: None,
            transition_time: 1.5,
            frame,
            config,
        }
    }

    #[expect(dead_code)]
    pub fn fade_in(self, duration: f64) -> Self {
        Self {
            transition: Some(Transition::FadeIn(duration)),
            transition_time: 0.0,
            ..self
        }
    }
}

impl<const N: usize> Default for MilkyWay<N> {
    fn default() -> Self {
        let mut frame: [Argb; N] = [MilkyWayConfig::default().background; N];
        generate_random(&mut frame, &MilkyWayConfig::default());
        Self {
            phase: 0.0,
            transition: None,
            transition_time: 1.5,
            frame,
            config: MilkyWayConfig::default(),
        }
    }
}

impl<const N: usize> Animation for MilkyWay<N> {
    type Frame = RingFrame<N>;

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    #[allow(clippy::cast_precision_loss)]
    fn animate(
        &mut self,
        frame: &mut RingFrame<N>,
        dt: f64,
        idle: bool,
    ) -> AnimationState {
        match self.transition {
            Some(Transition::ForceStop) => AnimationState::Finished,
            Some(Transition::StartDelay(delay)) => {
                self.transition_time += dt;
                if self.transition_time >= delay {
                    self.transition = None;
                }
                AnimationState::Running
            }
            Some(Transition::FadeOut(duration)) => {
                // apply sine wave to stop the animation smoothly
                self.phase += dt;
                let scaling_factor = (self.transition_time / duration * PI / 2.0).cos();
                for (led, background_led) in frame.iter_mut().zip(&self.frame) {
                    *led = *background_led * scaling_factor;
                }
                if self.phase >= duration {
                    AnimationState::Finished
                } else {
                    AnimationState::Running
                }
            }
            Some(Transition::FadeIn(duration)) => {
                // apply sine wave to start the animation smoothly
                self.phase += dt;
                let scaling_factor = (self.transition_time / duration * PI / 2.0).sin();
                for (led, background_led) in frame.iter_mut().zip(&self.frame) {
                    *led = *background_led * scaling_factor;
                }
                if self.phase >= duration {
                    self.transition = None;
                }
                AnimationState::Running
            }
            _ => {
                let mut color = self.frame[0];
                for (i, led) in &mut self.frame.iter_mut().enumerate() {
                    if i % 2 == 0 {
                        color = Argb(
                            led.0,
                            ((led.1 as i16 + rand_delta(self.config.fade_delta)) as u8)
                                .clamp(
                                    self.config.red_min_max.0,
                                    self.config.red_min_max.1,
                                ),
                            ((led.2 as i16 + rand_delta(self.config.fade_delta)) as u8)
                                .clamp(
                                    self.config.green_min_max.0,
                                    self.config.green_min_max.1,
                                ),
                            ((led.3 as i16 + rand_delta(self.config.fade_delta)) as u8)
                                .clamp(
                                    self.config.blue_min_max.0,
                                    self.config.blue_min_max.1,
                                ),
                        );
                    }

                    *led = color;
                }

                if !idle {
                    frame.copy_from_slice(&self.frame);
                }
                AnimationState::Running
            }
        }
    }

    fn stop(&mut self, transition: Transition) -> eyre::Result<()> {
        if transition == Transition::PlayOnce || transition == Transition::Shrink {
            return Err(eyre!(
                "Transition {:?} not supported for Milky Way animation",
                transition
            ));
        }

        self.transition_time = 0.0;
        self.transition = Some(transition);

        Ok(())
    }
}
