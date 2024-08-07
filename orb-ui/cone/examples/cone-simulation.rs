/// This is an example that shows how to initialize and
/// control devices connected to the cone through FTDI chips
use color_eyre::eyre;
use color_eyre::eyre::{eyre, Context};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use orb_cone::led::CONE_LED_COUNT;
use orb_cone::{ButtonState, ConeEvent};
use orb_rgb::Argb;

const CONE_LED_STRIP_DIMMING_DEFAULT: u8 = 10_u8;
const CONE_SIMULATION_UPDATE_PERIOD_S: u64 = 2;
const CONE_LED_STRIP_MAXIMUM_BRIGHTNESS: u8 = 20;

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

    loop {
        let (tx, mut rx) = broadcast::channel(10);
        if let Ok((mut cone, cone_handles)) = orb_cone::Cone::spawn(tx) {
            // spawn a thread to receive events
            let button_listener_task: JoinHandle<eyre::Result<()>> =
                task::spawn(async move {
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
                                return Err(eyre!(
                                    "Cone events channel closed, cone disconnected?"
                                ))
                            }
                            Err(RecvError::Lagged(skipped)) => {
                                tracing::warn!("🚨 Skipped {} cone events", skipped);
                            }
                        }
                    }
                });

            // create one shot to gracefully terminate simulation
            let (kill_sim_tx, mut kill_sim_rx) = tokio::sync::oneshot::channel::<()>();
            let simulation_task: JoinHandle<eyre::Result<()>> = tokio::task::spawn(
                async move {
                    let mut counter = 0;
                    loop {
                        tokio::select! {
                            _ = &mut kill_sim_rx => {
                                return Ok(());
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_secs(CONE_SIMULATION_UPDATE_PERIOD_S)) => {
                                let mut pixels = [Argb::default(); CONE_LED_COUNT];
                                match counter {
                                    0 => {
                                        cone.queue_lcd_fill(Argb::DIAMOND_USER_IDLE)?;
                                        for pixel in pixels.iter_mut() {
                                            *pixel = Argb::DIAMOND_USER_IDLE;
                                        }
                                    }
                                    1 => {
                                        cone.queue_lcd_fill(Argb::FULL_RED)?;
                                        for pixel in pixels.iter_mut() {
                                            *pixel = Argb::FULL_RED;
                                            pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                                        }
                                    }
                                    2 => {
                                        cone.queue_lcd_fill(Argb::FULL_GREEN)?;
                                        for pixel in pixels.iter_mut() {
                                            *pixel = Argb::FULL_GREEN;
                                            pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                                        }
                                    }
                                    3 => {
                                        cone.queue_lcd_fill(Argb::FULL_BLUE)?;
                                        for pixel in pixels.iter_mut() {
                                            *pixel = Argb::FULL_BLUE;
                                            pixel.0 = Some(CONE_LED_STRIP_DIMMING_DEFAULT);
                                        }
                                    }
                                    4 => {
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
                                                rand::random::<u8>()
                                                    % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                                                rand::random::<u8>()
                                                    % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                                                rand::random::<u8>()
                                                    % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                                            );
                                        }
                                    }
                                    5 => {
                                        cone.queue_lcd_qr_code(String::from(
                                            "https://www.worldcoin.org/",
                                        ))?;
                                        for pixel in pixels.iter_mut() {
                                            *pixel = Argb::DIAMOND_USER_AMBER;
                                        }
                                    }
                                    _ => {}
                                }
                                cone.queue_rgb_leds(&pixels)?;
                            }
                        } // end tokio::select!
                        counter = (counter + 1) % 6;
                    }
                },
            );

            tracing::info!("🍦 Cone up and running!");
            tracing::info!("Press ctrl-c to exit.");

            // upon completion of either task, cancel all the other tasks
            // and return the result
            let res = tokio::select! {
                res = button_listener_task => {
                    tracing::debug!("Button listener task completed");
                    res?
                },
                res = simulation_task => {
                    tracing::debug!("Simulation task completed");
                    res?
                },
                // Needed to cleanly call destructors.
                result = tokio::signal::ctrl_c() => {
                    tracing::debug!("ctrl-c received");
                    result.wrap_err("failed to listen for ctrl-c")
                }
            };

            // to drop the cone, stop the simulation
            // then wait for all tasks to stop
            drop(kill_sim_tx);
            cone_handles.join().await?;

            if res.is_ok() {
                return Ok(());
            }
        } else {
            tracing::error!("Failed to connect to cone...");
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
