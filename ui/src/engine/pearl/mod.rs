mod operator_based;
mod self_serve;

use crate::engine::animations::alert::BlinkDurations;
use async_trait::async_trait;
use eyre::Result;
use futures::channel::mpsc;
use futures::channel::mpsc::Sender;
use futures::future::Either;
use futures::{future, StreamExt};
use orb_messages::main::{jetson_to_mcu, JetsonToMcu};
use orb_messages::mcu_message::Message;
use pid::{InstantTimer, Timer};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time;
use tokio::time::Duration;
use tokio_stream::wrappers::{IntervalStream, UnboundedReceiverStream};

use crate::engine::{
    animations, operator, Animation, AnimationsStack, CenterFrame, Event, EventHandler,
    OperatingMode, OperatorFrame, OrbType, RingFrame, Runner, RunningAnimation,
    Transition, LED_ENGINE_FPS, LEVEL_FOREGROUND, LEVEL_NOTICE, PEARL_CENTER_LED_COUNT,
    PEARL_RING_LED_COUNT,
};
use crate::sound;
use crate::sound::Player;
use orb_rgb::Argb;

struct WrappedMessage(Message);

impl From<CenterFrame<PEARL_CENTER_LED_COUNT>> for WrappedMessage {
    fn from(value: CenterFrame<PEARL_CENTER_LED_COUNT>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::CenterLedsSequence(
                    orb_messages::main::UserCenterLeDsSequence {
                        data_format: Some(
                            orb_messages::main::user_center_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<RingFrame<PEARL_RING_LED_COUNT>> for WrappedMessage {
    fn from(value: RingFrame<PEARL_RING_LED_COUNT>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::RingLedsSequence(
                    orb_messages::main::UserRingLeDsSequence {
                        data_format: Some(
                            orb_messages::main::user_ring_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

/// Dummy implementation, not used since Pearl cannot be connected to a cone
impl From<RingFrame<64>> for WrappedMessage {
    fn from(value: RingFrame<64>) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::ConeLedsSequence(
                    orb_messages::main::ConeLeDsSequence {
                        data_format: Some(
                            orb_messages::main::cone_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

impl From<OperatorFrame> for WrappedMessage {
    fn from(value: OperatorFrame) -> Self {
        WrappedMessage(Message::JMessage(
            JetsonToMcu {
                ack_number: 0,
                payload: Some(jetson_to_mcu::Payload::DistributorLedsSequence(
                    orb_messages::main::DistributorLeDsSequence {
                        data_format: Some(
                            orb_messages::main::distributor_le_ds_sequence::DataFormat::RgbUncompressed(
                                value.iter().flat_map(|&Argb(_, r, g, b)| [r, g, b]).collect(),
                            ))
                    }
                )),
            }
        ))
    }
}

pub async fn event_loop(
    rx: UnboundedReceiver<Event>,
    mcu_tx: Sender<Message>,
) -> Result<()> {
    let mut interval = time::interval(Duration::from_millis(1000 / LED_ENGINE_FPS));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut interval = IntervalStream::new(interval);
    let mut rx = UnboundedReceiverStream::new(rx);
    let mut runner = match sound::Jetson::spawn().await {
        Ok(sound) => Runner::<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT>::new(sound),
        Err(e) => return Err(eyre::eyre!("Failed to initialize sound: {:?}", e)),
    };
    loop {
        match future::select(rx.next(), interval.next()).await {
            Either::Left((None, _)) => {
                break;
            }
            Either::Left((Some(event), _)) => {
                if let Err(e) = runner.event(&event) {
                    tracing::error!("Error handling event: {:?}", e);
                }
            }
            Either::Right(_) => {
                if let Err(e) = runner.run(&mut mcu_tx.clone()).await {
                    tracing::error!("Error running UI: {:?}", e);
                }
            }
        }
    }
    Ok(())
}

impl Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    pub(crate) fn new(sound: sound::Jetson) -> Self {
        Self {
            timer: InstantTimer::default(),
            ring_animations_stack: AnimationsStack::new(),
            center_animations_stack: AnimationsStack::new(),
            cone_animations_stack: None,
            ring_frame: [Argb(None, 0, 0, 0); PEARL_RING_LED_COUNT],
            center_frame: [Argb(None, 0, 0, 0); PEARL_CENTER_LED_COUNT],
            cone_frame: None,
            operator_frame: OperatorFrame::default(),
            operator_idle: operator::Idle::new(OrbType::Pearl),
            operator_blink: operator::Blink::new(OrbType::Pearl),
            operator_pulse: operator::Pulse::new(OrbType::Pearl),
            operator_action: operator::Bar::new(OrbType::Pearl),
            operator_signup_phase: operator::SignupPhase::new(OrbType::Pearl),
            sound,
            capture_sound: sound::capture::CaptureLoopSound::default(),
            is_api_mode: false,
            paused: false,
            gimbal: None,
            operating_mode: OperatingMode::default(),
        }
    }

    fn set_ring(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = RingFrame<PEARL_RING_LED_COUNT>>,
    ) {
        self.ring_animations_stack.set(level, Box::new(animation));
    }

    fn set_center(
        &mut self,
        level: u8,
        animation: impl Animation<Frame = CenterFrame<PEARL_CENTER_LED_COUNT>>,
    ) {
        self.center_animations_stack.set(level, Box::new(animation));
    }

    fn stop_ring(&mut self, level: u8, transition: Transition) {
        self.ring_animations_stack.stop(level, transition);
    }

    fn stop_center(&mut self, level: u8, transition: Transition) {
        self.center_animations_stack.stop(level, transition);
    }
}

#[async_trait]
impl EventHandler for Runner<PEARL_RING_LED_COUNT, PEARL_CENTER_LED_COUNT> {
    #[allow(clippy::too_many_lines)]
    fn event(&mut self, event: &Event) -> Result<()> {
        tracing::debug!("UI event: {}", serde_json::to_string(event)?.as_str());
        match event {
            /* Common Events handled first, see below for operating-mode specific events */
            Event::Flow { mode } => {
                self.operating_mode = *mode;
            }
            Event::GoodInternet => {
                self.operator_idle.good_internet();
            }
            Event::SlowInternet => {
                self.operator_idle.slow_internet();
            }
            Event::NoInternet => {
                self.operator_idle.no_internet();
            }
            Event::GoodWlan => {
                self.operator_idle.good_wlan();
            }
            Event::SlowWlan => {
                self.operator_idle.slow_wlan();
            }
            Event::NoWlan => {
                self.operator_idle.no_wlan();
            }
            Event::BatteryCapacity { percentage } => {
                self.operator_idle.battery_capacity(*percentage);
            }
            Event::BatteryIsCharging { is_charging } => {
                self.operator_idle.battery_charging(*is_charging);
            }
            Event::Pause => {
                self.paused = true;
            }
            Event::Resume => {
                self.paused = false;
            }
            Event::Beacon => {
                let master_volume = self.sound.volume();
                self.sound.set_master_volume(50);
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::IrisScanningLoop01A),
                    Duration::ZERO,
                )?;
                self.sound.set_master_volume(master_volume);

                self.stop_ring(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_center(LEVEL_FOREGROUND, Transition::ForceStop);
                self.stop_ring(LEVEL_NOTICE, Transition::FadeOut(0.5));
                self.stop_center(LEVEL_NOTICE, Transition::FadeOut(0.5));

                self.set_center(
                    LEVEL_NOTICE,
                    animations::Alert::<PEARL_CENTER_LED_COUNT>::new(
                        Argb::PEARL_USER_QR_SCAN,
                        BlinkDurations::from(vec![0.0, 0.3, 0.45, 0.46]),
                        None,
                        false,
                    )?,
                );
                self.set_ring(
                    LEVEL_NOTICE,
                    animations::MilkyWay::<PEARL_RING_LED_COUNT>::default(),
                );
            }
            Event::RecoveryImage => {
                self.sound.queue(
                    sound::Type::Voice(sound::Voice::PleaseDontShutDown),
                    Duration::ZERO,
                )?;
                // check that ring is not already in recovery mode
                if self
                    .ring_animations_stack
                    .stack
                    .get_mut(&LEVEL_NOTICE)
                    .and_then(|RunningAnimation { animation, .. }| {
                        animation
                            .as_any_mut()
                            .downcast_mut::<animations::Spinner<PEARL_RING_LED_COUNT>>()
                    })
                    .is_none()
                {
                    self.set_ring(
                        LEVEL_NOTICE,
                        animations::Spinner::<PEARL_RING_LED_COUNT>::triple(
                            Argb::PEARL_CENTER_SUMMON_USER_AMBER,
                            None,
                        ),
                    );
                }
            }
            Event::NoInternetForSignup => {
                self.sound.queue(
                    sound::Type::Voice(
                        sound::Voice::InternetConnectionTooSlowToPerformSignups,
                    ),
                    Duration::ZERO,
                )?;
            }
            Event::SlowInternetForSignup => {
                self.sound.queue(
                    sound::Type::Voice(
                        sound::Voice::InternetConnectionTooSlowSignupsMightTakeLonger,
                    ),
                    Duration::ZERO,
                )?;
            }
            Event::SoundVolume { level } => {
                self.sound.set_master_volume(*level);
            }
            Event::SoundLanguage { lang } => {
                let language = lang.clone();
                let sound = self.sound.clone();
                // spawn a new task because we need some async work here
                tokio::task::spawn(async move {
                    match sound::SoundConfig::default()
                        .with_language(language.as_deref())
                    {
                        Ok(config) => {
                            if let Err(e) = sound.load_sound_files(config).await {
                                tracing::error!("Error loading sound files: {:?}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error creating sound config: {:?}", e);
                        }
                    }
                });
            }
            Event::SoundTest => {
                self.sound.queue(
                    sound::Type::Melody(sound::Melody::BootUp),
                    Duration::ZERO,
                )?;
            }
            Event::Gimbal { x, y } => {
                self.gimbal = Some((*x, *y));
            }

            /* Events that are handled differently depending on the operating mode */
            event => {
                if self.operating_mode == OperatingMode::Operator {
                    self.event_operator(event)?;
                } else {
                    self.event_self_serve(event)?;
                }
            }
        }
        Ok(())
    }

    async fn run(&mut self, interface_tx: &mut mpsc::Sender<Message>) -> Result<()> {
        let dt = self.timer.get_dt().unwrap_or(0.0);
        self.center_animations_stack.run(&mut self.center_frame, dt);
        if !self.paused {
            interface_tx.try_send(WrappedMessage::from(self.center_frame).0)?;
        }

        self.operator_idle
            .animate(&mut self.operator_frame, dt, false);
        self.operator_signup_phase
            .animate(&mut self.operator_frame, dt, false);
        self.operator_blink
            .animate(&mut self.operator_frame, dt, false);
        self.operator_pulse
            .animate(&mut self.operator_frame, dt, false);
        self.operator_action
            .animate(&mut self.operator_frame, dt, false);
        // 2ms sleep to make sure UART communication is over
        time::sleep(Duration::from_millis(2)).await;
        interface_tx.try_send(WrappedMessage::from(self.operator_frame).0)?;

        self.ring_animations_stack.run(&mut self.ring_frame, dt);
        if !self.paused {
            // ⚠️ self-serve animations need a low brightness that's achieved
            // by turning off half of the LEDs (one every two: 224 / 2 = 112)
            // this allows setting a higher brightness at the RGB level, for
            // a larger amplitude during wave animations
            if self.operating_mode == OperatingMode::SelfServe {
                for (i, led) in self.ring_frame.iter_mut().enumerate() {
                    if i % 2 == 0 {
                        *led = Argb::OFF;
                    }
                }
            }
            time::sleep(Duration::from_millis(2)).await;
            interface_tx.try_send(WrappedMessage::from(self.ring_frame).0)?;
        }
        if let Some(animation) = &mut self.cone_animations_stack {
            if let Some(frame) = &mut self.cone_frame {
                animation.run(frame, dt);
                if !self.paused {
                    time::sleep(Duration::from_millis(2)).await;
                    interface_tx.try_send(WrappedMessage::from(*frame).0)?;
                }
            }
        }
        // one last update of the UI has been performed since api_mode has been set,
        // (to set the api_mode UI state), so we can now pause the engine
        if self.is_api_mode && !self.paused {
            self.paused = true;
            tracing::info!("UI paused in API mode");
        } else if !self.is_api_mode && self.paused {
            self.paused = false;
            tracing::info!("UI resumed from API mode");
        }
        Ok(())
    }
}
