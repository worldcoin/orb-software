/// This is an example that shows how to initialize and
/// control devices connected to the cone (FTDI chip)
use color_eyre::eyre;
use tracing::info;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use orb_cone::led::CONE_LED_COUNT;
use orb_cone::{ConeEvents, ConeLeds};
use orb_rgb::Argb;

const CONE_LED_STRIP_DIMMING_DEFAULT: u8 = 20_u8;
const CONE_LED_STRIP_RAINBOW_PERIOD_MS: u64 = 150;
const CONE_LED_STRIP_MAXIMUM_BRIGHTNESS: u8 = 20;

fn main() -> eyre::Result<()> {
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

    let (tx, rx) = std::sync::mpsc::channel();
    let mut cone = orb_cone::Cone::new(tx)?;

    // spawn a thread to receive events
    std::thread::spawn(move || {
        let mut button_pressed = false;
        loop {
            match rx.recv() {
                Ok(event) => match event {
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
                Err(e) => {
                    tracing::error!("Error receiving event: {:?}", e);
                    break;
                }
            }
        }
    });

    info!("🍦 Cone initialized");

    cone.lcd_load_image("examples/logo.bmp")?;
    loop {
        // let animate the 64-LED strip with a rainbow pattern by putting random colors
        let mut pixels = ConeLeds([Argb::default(); CONE_LED_COUNT]);
        for pixel in pixels.0.iter_mut() {
            *pixel = Argb(
                Some(CONE_LED_STRIP_DIMMING_DEFAULT),
                // random
                rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
                rand::random::<u8>() % CONE_LED_STRIP_MAXIMUM_BRIGHTNESS,
            );
        }
        cone.queue_rgb_leds(&pixels)?;

        std::thread::sleep(std::time::Duration::from_millis(
            CONE_LED_STRIP_RAINBOW_PERIOD_MS,
        ));
    }
}
