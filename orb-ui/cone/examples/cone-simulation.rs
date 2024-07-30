/// This is an example that shows how to initialize and
/// control devices connected to the cone (FTDI chip)
use color_eyre::eyre;
use tokio::sync::mpsc;
use tokio::task;
use tracing::info;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use orb_cone::led::CONE_LED_COUNT;
use orb_cone::ConeEvents;
use orb_rgb::Argb;

const CONE_LED_STRIP_DIMMING_DEFAULT: u8 = 10_u8;
const CONE_LED_STRIP_RAINBOW_PERIOD_S: u64 = 2;
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

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut cone = orb_cone::Cone::new(tx)?;

    // spawn a thread to receive events
    task::spawn(async move {
        let mut button_pressed = false;
        loop {
            match rx.recv().await {
                Some(event) => match event {
                    ConeEvents::ButtonPressed(state) => {
                        if state != button_pressed {
                            info!(
                                "🔘 Button {}",
                                if state { "pressed" } else { "released" }
                            );
                            button_pressed = state;
                        }
                    }
                },
                None => {
                    tracing::error!("Cone events channel closed");
                    break;
                }
            }
        }
    });

    info!("🍦 Cone initialized");

    let mut counter = 0;
    loop {
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
                cone.queue_lcd_bmp(String::from("examples/logo.bmp"))?;
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
            5 => {
                cone.queue_lcd_qr_code(String::from("https://www.worldcoin.org/"))?;
                for pixel in pixels.iter_mut() {
                    *pixel = Argb::DIAMOND_CONE_AMBER;
                }
            }
            _ => {}
        }
        cone.queue_rgb_leds(&pixels)?;

        std::thread::sleep(std::time::Duration::from_secs(
            CONE_LED_STRIP_RAINBOW_PERIOD_S,
        ));
        counter = (counter + 1) % 6;
    }
}
