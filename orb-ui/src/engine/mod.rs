//! LED engine.

use crate::engine::rgb::Argb;
use crate::sound;
use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc::Sender;
use orb_messages::mcu_main::mcu_message::Message;
use pid::InstantTimer;
use serde::{Deserialize, Serialize};
use std::{any::Any, collections::BTreeMap};
use tokio::{sync::mpsc, task};

pub mod center;
mod diamond;
pub mod operator;
mod pearl;
mod rgb;
pub mod ring;

pub const PEARL_RING_LED_COUNT: usize = 224;
pub const PEARL_CENTER_LED_COUNT: usize = 9;

pub const DIAMOND_RING_LED_COUNT: usize = 76;
pub const DIAMOND_CENTER_LED_COUNT: usize = 23;
pub const DIAMOND_CONE_LED_COUNT: usize = 64;

#[derive(Default)]
pub enum OrbType {
    #[default]
    Pearl,
    Diamond,
}

pub const LED_ENGINE_FPS: u64 = 30;

const GAMMA: f64 = 2.5;

const LEVEL_BACKGROUND: u8 = 0;
const LEVEL_FOREGROUND: u8 = 10;
const LEVEL_NOTICE: u8 = 20;

const BIOMETRIC_PIPELINE_MAX_PROGRESS: f64 = 0.875;

macro_rules! event_enum {
    (
        $(#[$($enum_attrs:tt)*])*
        $vis:vis enum $name:ident {
            $(
                $(#[doc = $doc:expr])?
                #[event_enum(method = $method:ident)]
                $(#[$($event_attrs:tt)*])*
                $event:ident $({$($field:ident: $ty:ty),*$(,)?})?,
            )*
        }
    ) => {
        $(#[$($enum_attrs)*])*
        #[derive(Debug, Deserialize, Serialize)]
        $vis enum $name {
            $(
                $(#[doc = $doc])?
                $(#[$($event_attrs)*])*
                $event $({$($field: $ty,)*})?,
            )*
        }

        /// LED engine interface.
        pub trait Engine: Send + Sync {
            $(
                $(#[doc = $doc])?
                fn $method(&self, $($($field: $ty,)*)?);
            )*

            /// Returns a new handler to the shared queue.
            fn clone(&self) -> Box<dyn Engine>;
        }

        impl Engine for PearlJetson {
            $(
                $(#[doc = $doc])?
                fn $method(&self, $($($field: $ty,)*)?) {
                    let event = $name::$event $({$($field,)*})?;
                    self.tx.send(event).expect("LED engine is not running");
                }
            )*

            fn clone(&self) -> Box<dyn Engine> {
                Box::new(PearlJetson { tx: self.tx.clone() })
            }
        }


        impl Engine for DiamondJetson {
            $(
                $(#[doc = $doc])?
                fn $method(&self, $($($field: $ty,)*)?) {
                    let event = $name::$event $({$($field,)*})?;
                    self.tx.send(event).expect("LED engine is not running");
                }
            )*

            fn clone(&self) -> Box<dyn Engine> {
                Box::new(DiamondJetson { tx: self.tx.clone() })
            }
        }

        impl Engine for Fake {
            $(
                $(#[doc = $doc])?
                #[allow(unused_variables)]
                fn $method(&self, $($($field: $ty,)*)?) {}
            )*

            fn clone(&self) -> Box<dyn Engine> {
                Box::new(Fake)
            }
        }
    };
}

/// QR-code scanning schema.
#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize)]
pub enum QrScanSchema {
    /// Operator QR-code scanning.
    Operator,
    /// User QR-code scanning.
    User,
    /// WiFi QR-code scanning.
    Wifi,
}

event_enum! {
    #[allow(dead_code)]
    pub enum Event {
        /// Orb boot up.
        #[event_enum(method = bootup)]
        Bootup,
        /// Orb token was acquired
        #[event_enum(method = boot_complete)]
        BootComplete,
        /// Start of the signup phase, triggered on button press
        #[event_enum(method = signup_start)]
        SignupStart,
        /// Start of QR scan.
        #[event_enum(method = qr_scan_start)]
        QrScanStart {
            schema: QrScanSchema,
        },
        /// QR scan completed.
        #[event_enum(method = qr_scan_completed)]
        QrScanCompleted {
            schema: QrScanSchema,
        },
        /// QR scan succeeded.
        #[event_enum(method = qr_scan_success)]
        QrScanSuccess {
            schema: QrScanSchema,
        },
        /// QR scan is valid but unexpected.
        #[event_enum(method = qr_scan_unexpected)]
        QrScanUnexpected {
            schema: QrScanSchema,
        },
        /// QR scan failed.
        #[event_enum(method = qr_scan_fail)]
        QrScanFail {
            schema: QrScanSchema,
        },
        /// Biometric capture half of the objectives completed.
        #[event_enum(method = biometric_capture_half_objectives_completed)]
        BiometricCaptureHalfObjectivesCompleted,
        /// Biometric capture all of the objectives completed.
        #[event_enum(method = biometric_capture_all_objectives_completed)]
        BiometricCaptureAllObjectivesCompleted,
        /// Biometric capture progress.
        #[event_enum(method = biometric_capture_progress)]
        BiometricCaptureProgress {
            progress: f64,
        },
        /// Biometric capture occlusion.
        #[event_enum(method = biometric_capture_occlusion)]
        BiometricCaptureOcclusion {
            occlusion_detected: bool
        },
        /// User not in distance range.
        #[event_enum(method = biometric_capture_distance)]
        BiometricCaptureDistance {
            in_range: bool
        },
        /// Biometric capture succeeded.
        #[event_enum(method = biometric_capture_success)]
        BiometricCaptureSuccess,
        /// Starting enrollment.
        #[event_enum(method = starting_enrollment)]
        StartingEnrollment,
        /// Biometric pipeline progress.
        #[event_enum(method = biometric_pipeline_progress)]
        BiometricPipelineProgress {
            progress: f64,
        },
        /// Biometric pipeline succeed.
        #[event_enum(method = biometric_pipeline_success)]
        BiometricPipelineSuccess,
        /// Signup unique.
        #[event_enum(method = signup_unique)]
        SignupUnique,
        /// Signup failure.
        #[event_enum(method = signup_fail)]
        SignupFail,
        /// Orb software versions are deprecated.
        #[event_enum(method = version_deprecated)]
        SoftwareVersionDeprecated,
        /// Orb software versions are outdated.
        #[event_enum(method = version_blocked)]
        SoftwareVersionBlocked,
        /// Idle mode.
        #[event_enum(method = idle)]
        Idle,
        /// Orb shutdown.
        #[event_enum(method = shutdown)]
        Shutdown {
            requested: bool,
        },

        /// Good internet connection.
        #[event_enum(method = good_internet)]
        GoodInternet,
        /// Slow internet connection.
        #[event_enum(method = slow_internet)]
        SlowInternet,
        /// No internet connection.
        #[event_enum(method = no_internet)]
        NoInternet,
        /// Good wlan connection.
        #[event_enum(method = good_wlan)]
        GoodWlan,
        /// Slow wlan connection.
        #[event_enum(method = slow_wlan)]
        SlowWlan,
        /// No wlan connection.
        #[event_enum(method = no_wlan)]
        NoWlan,

        /// Battery level indicator.
        #[event_enum(method = battery_capacity)]
        BatteryCapacity {
            percentage: u32,
        },
        /// Battery charging indicator.
        #[event_enum(method = battery_is_charging)]
        BatteryIsCharging {
            is_charging: bool,
        },

        /// Pause sending messages to the MCU. LED animations are still computed in the background
        #[event_enum(method = pause)]
        Pause,
        /// Resume sending messages to the MCU.
        #[event_enum(method = resume)]
        Resume,

        /// In recovery image
        #[event_enum(method = recovery)]
        RecoveryImage,
    }
}

/// Returned by [`Animation::animate`]
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum AnimationState {
    /// The animation is finished and shouldn't be called again
    Finished,
    /// The animation is still running
    Running,
}

impl AnimationState {
    /// if it is the `Running` variant
    #[must_use]
    pub fn is_running(&self) -> bool {
        *self == AnimationState::Running
    }
}

/// Generic animation.
pub trait Animation: Send + 'static {
    /// Animation frame type.
    type Frame;

    /// Upcasts a reference to self to the dynamic object [`Any`].
    fn as_any(&self) -> &dyn Any;

    /// Upcasts a mutable reference to self to the dynamic object [`Any`].
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Calculates the next animation frame according to the time delta.
    /// Returns [`AnimationState::Finished`] if the animation is finished
    /// and shouldn't be called again.
    fn animate(
        &mut self,
        frame: &mut Self::Frame,
        dt: f64,
        idle: bool,
    ) -> AnimationState;

    /// Sets a transition effect from the previous animation to this animation.
    fn transition_from(&mut self, _superseded: &dyn Any) {}

    /// Signals the animation to stop. It shouldn't necessarily stop
    /// immediately.
    fn stop(&mut self) {}
}

/// LED engine for the Orb hardware.
pub struct PearlJetson {
    tx: mpsc::UnboundedSender<Event>,
}

pub struct DiamondJetson {
    tx: mpsc::UnboundedSender<Event>,
}

/// LED engine interface which does nothing.
pub struct Fake;

/// Frame for the front LED ring.
pub type RingFrame<const RING_LED_COUNT: usize> = [Argb; RING_LED_COUNT];

/// Frame for the center LEDs.
pub type CenterFrame<const CENTER_LED_COUNT: usize> = [Argb; CENTER_LED_COUNT];

pub type OperatorFrame = [Argb; 5];

type DynamicAnimation<Frame> = Box<dyn Animation<Frame = Frame>>;

struct Runner<const RING_LED_COUNT: usize, const CENTER_LED_COUNT: usize> {
    timer: InstantTimer,
    ring_animations_stack: AnimationsStack<RingFrame<RING_LED_COUNT>>,
    center_animations_stack: AnimationsStack<CenterFrame<CENTER_LED_COUNT>>,
    cone_animations_stack: Option<AnimationsStack<RingFrame<DIAMOND_CONE_LED_COUNT>>>,
    ring_frame: RingFrame<RING_LED_COUNT>,
    cone_frame: Option<RingFrame<DIAMOND_CONE_LED_COUNT>>,
    center_frame: CenterFrame<CENTER_LED_COUNT>,
    operator_frame: OperatorFrame,
    operator_connection: operator::Connection,
    operator_battery: operator::Battery,
    operator_blink: operator::Blink,
    operator_pulse: operator::Pulse,
    operator_action: operator::Bar,
    operator_signup_phase: operator::SignupPhase,
    sound: sound::Jetson,
    paused: bool,
}

#[async_trait]
trait EventHandler {
    fn event(&mut self, event: &Event) -> Result<()>;

    async fn run(&mut self, interface_tx: &mut Sender<Message>) -> Result<()>;
}

struct AnimationsStack<Frame: 'static> {
    stack: BTreeMap<u8, RunningAnimation<Frame>>,
}

struct RunningAnimation<Frame> {
    animation: DynamicAnimation<Frame>,
    kill: bool,
}

impl PearlJetson {
    /// Creates a new LED engine.
    #[must_use]
    pub(crate) fn spawn(interface_tx: &mut Sender<Message>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        task::spawn(pearl::event_loop(rx, interface_tx.clone()));
        Self { tx }
    }
}

impl DiamondJetson {
    /// Creates a new LED engine.
    #[must_use]
    pub(crate) fn spawn(interface_tx: &mut Sender<Message>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        task::spawn(diamond::event_loop(rx, interface_tx.clone()));
        Self { tx }
    }
}

pub trait EventChannel: Sync + Send {
    fn clone_tx(&self) -> mpsc::UnboundedSender<Event>;
}

impl EventChannel for PearlJetson {
    fn clone_tx(&self) -> mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }
}

impl EventChannel for DiamondJetson {
    fn clone_tx(&self) -> mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }
}

impl<Frame: 'static> AnimationsStack<Frame> {
    fn new() -> Self {
        Self {
            stack: BTreeMap::new(),
        }
    }

    fn stop(&mut self, level: u8, force: bool) {
        if let Some(RunningAnimation { animation, kill }) = self.stack.get_mut(&level) {
            animation.stop();
            *kill = *kill || force;
        }
    }

    fn set(&mut self, level: u8, mut animation: DynamicAnimation<Frame>) {
        if let Some(&top_level) = self.stack.keys().next_back() {
            if top_level <= level {
                let RunningAnimation {
                    animation: superseded,
                    ..
                } = self
                    .stack
                    .get(&level)
                    .or_else(|| self.stack.values().next_back())
                    .unwrap();
                animation.transition_from(superseded.as_any());
            }
        }
        self.stack.insert(
            level,
            RunningAnimation {
                animation,
                kill: false,
            },
        );
    }

    fn run(&mut self, frame: &mut Frame, dt: f64) {
        let mut top_level = None;
        // Running the top animation.
        let mut completed_animation: Option<DynamicAnimation<Frame>> = None;
        while let Some((&level, RunningAnimation { animation, kill })) =
            self.stack.iter_mut().next_back()
        {
            top_level = Some(level);
            if let Some(completed_animation) = &completed_animation {
                animation.transition_from(completed_animation.as_any());
            }
            if !*kill && animation.animate(frame, dt, false).is_running() {
                break;
            }
            let RunningAnimation { animation, .. } = self.stack.remove(&level).unwrap();
            if completed_animation.is_none() {
                completed_animation = Some(animation);
            }
        }
        // Idling the background animations.
        if let Some(top_level) = top_level {
            self.stack
                .retain(|&level, RunningAnimation { animation, .. }| {
                    if level == top_level {
                        true
                    } else {
                        animation.animate(frame, dt, true).is_running()
                    }
                });
        }
    }
}
