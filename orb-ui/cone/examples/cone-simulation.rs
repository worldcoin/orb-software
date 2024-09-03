/// This is an example that shows how to initialize and
/// control devices connected to the cone through FTDI chips
use color_eyre::eyre;
use color_eyre::eyre::{eyre, Context};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use orb_cone::led::CONE_LED_COUNT;
use orb_cone::{ButtonState, Cone, ConeEvent};
use orb_rgb::Argb;

const CONE_LED_STRIP_DIMMING_DEFAULT: u8 = 10_u8;
const CONE_SIMULATION_UPDATE_PERIOD_S: u64 = 2;
const CONE_LED_STRIP_MAXIMUM_BRIGHTNESS: u8 = 20;

enum SimulationState {
    Idle = 0,
    Red,
    Green,
    Blue,
    Logo,
    QrCode,
    StateCount,
}

impl From<u8> for SimulationState {
    fn from(value: u8) -> Self {
        match value {
            0 => SimulationState::Idle,
            1 => SimulationState::Red,
            2 => SimulationState::Green,
            3 => SimulationState::Blue,
            4 => SimulationState::Logo,
            5 => SimulationState::QrCode,
            _ => SimulationState::Idle,
        }
    }
}

async fn simulation_task(cone: &mut Cone) -> eyre::Result<()> {
    let mut counter = SimulationState::Idle;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(
            CONE_SIMULATION_UPDATE_PERIOD_S,
        ))
        .await;

        let mut pixels = [Argb::default(); CONE_LED_COUNT];
        match counter {
            SimulationState::Idle => {
                cone.queue_lcd_fill(Argb::DIAMOND_USER_IDLE)?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::DIAMOND_USER_IDLE;
                }
            }
            SimulationState::Red => {
                cone.queue_lcd_fill(Argb::FULL_RED)?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::FULL_RED;
                    pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                }
            }
            SimulationState::Green => {
                cone.queue_lcd_fill(Argb::FULL_GREEN)?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::FULL_GREEN;
                    pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                }
            }
            SimulationState::Blue => {
                cone.queue_lcd_fill(Argb::FULL_BLUE)?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::FULL_BLUE;
                    pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                }
            }
            SimulationState::Logo => {
                // show logo if file exists
                let filename = "logo.bmp";
                if std::path::Path::new(filename).exists() {
                    cone.queue_lcd_bmp(String::from(filename))?;
                } else {
                    tracing::debug!("🚨 File not found: {filename}");
                    cone.queue_lcd_fill(Argb::FULL_BLACK)?;
                }
                for pixel in pixels.iter_mut() {
                    *pixel = Argb(
                        Some(CONE_LED_STRIP_DIMMING_DEFAULT),
                        // random
                        rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                        rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                        rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                    );
                }
            }
            SimulationState::QrCode => {
                cone.queue_lcd_qr_code(String::from("https://www.worldcoin.org/"))?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::DIAMOND_USER_AMBER;
                }
            }
            _ => {}
        }
        cone.queue_rgb_leds(&pixels)?;
        counter = SimulationState::from(
            (counter as u8 + 1) % SimulationState::StateCount as u8,
        );
    }
}

async fn listen_cone_events(
    mut rx: broadcast::Receiver<ConeEvent>,
) -> eyre::Result<()> {
    let mut button_state = ButtonState::Released;
    loop {
        match rx.recv().await {
            Ok(event) => match event {
                ConeEvent::Button(state) => {
                    if state != button_state {
                        tracing::info!("🔘 Button {:?}", state);
                        button_state = state;
                    }
                }
                ConeEvent::Cone(state) => {
                    tracing::info!("🔌 Cone {:?}", state);
                }
            },
            Err(RecvError::Closed) => {
                return Err(eyre!("Cone events channel closed, cone disconnected?"));
            }
            Err(RecvError::Lagged(skipped)) => {
                tracing::warn!("🚨 Skipped {} cone events", skipped);
            }
        }
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let registry = tracing_subscriber::registry();
    #[cfg(tokio_unstable)]
    let registry = registry.with(console_subscriber::spawn());
    registry
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let devices = ftdi_embedded_hal::libftd2xx::list_devices()?;
    for device in devices.iter() {
        tracing::debug!("Device: {:?}", device);
    }

    let (cone_events_tx, cone_events_rx) = broadcast::channel(10);
    let (mut cone, cone_handles) = Cone::spawn(cone_events_tx)?;

    tracing::info!("🍦 Cone up and running!");
    tracing::info!("Press ctrl-c to exit.");

    // upon completion of either task, select! will cancel all the other branches
    let res = tokio::select! {
        res = listen_cone_events(cone_events_rx) => {
            tracing::debug!("Button listener task completed");
            res
        },
        res = simulation_task(&mut cone) => {
            tracing::debug!("Simulation task completed");
            res
        },
        // Needed to cleanly call destructors.
        result = tokio::signal::ctrl_c() => {
            tracing::debug!("ctrl-c received");
            result.wrap_err("failed to listen for ctrl-c")
        }
    };

    // wait for all tasks to stop
    cone_handles.join().await?;

    res
}
