use crate::dbus::intf_impl::BackendStatusImpl;
use color_eyre::{eyre::eyre, Result};
use orb_messages::main::{ambient_light::Flags, mcu_to_jetson::Payload, AmbientLight};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// The zenoh key expression for front ALS (Ambient Light Sensor).
pub const FRONT_ALS_KEY_EXPR: &str = "mcu/main/front_als";

pub struct FrontAlsWatcher {
    pub task: tokio::task::JoinHandle<()>,
}

/// Spawn a front ALS watcher that subscribes to zenoh mcu/main/front_als topic.
pub async fn spawn_watcher(
    zsession: &zenorb::Zenorb,
    backend_status: BackendStatusImpl,
    shutdown_token: CancellationToken,
) -> Result<FrontAlsWatcher> {
    let ctx = WatcherCtx {
        current: Arc::new(Mutex::new(None)),
        backend_status,
    };

    let mut tasks = zsession
        .receiver(ctx)
        .querying_subscriber(
            FRONT_ALS_KEY_EXPR,
            Duration::from_millis(100),
            handle_front_als_event,
        )
        .run()
        .await?;

    let subscriber_task = tasks
        .pop()
        .ok_or_else(|| eyre!("expected subscriber task"))?;

    let task = tokio::spawn(async move {
        shutdown_token.cancelled().await;
        subscriber_task.abort();
    });

    Ok(FrontAlsWatcher { task })
}

#[derive(Clone)]
struct WatcherCtx {
    current: Arc<Mutex<Option<AmbientLight>>>,
    backend_status: BackendStatusImpl,
}

async fn handle_front_als_event(
    ctx: WatcherCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let key = sample.key_expr().to_string();

    let payload_str = match sample.payload().try_to_string() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to convert payload to string for key {key}: {e}");
            return Ok(());
        }
    };

    // Deserialize into the Payload enum from orb-messages
    let payload: Payload = match serde_json::from_str(payload_str.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Failed to parse Payload for key {key}: {e}, payload: {payload_str}"
            );
            return Ok(());
        }
    };

    // Extract AmbientLight from the FrontAls variant
    let als = match payload {
        Payload::FrontAls(als) => als,
        other => {
            warn!("Unexpected payload variant for front_als: {other:?}");
            return Ok(());
        }
    };

    let mut current = ctx.current.lock().await;
    *current = Some(als.clone());

    ctx.backend_status.update_front_als(Some(als));

    Ok(())
}

/// Convert the flag integer from protobuf to a human-readable string for the API.
pub fn flag_to_api_str(flag: i32) -> &'static str {
    match Flags::try_from(flag) {
        Ok(Flags::AlsOk) => "ok",
        Ok(Flags::AlsErrRange) => "err_range",
        Ok(Flags::AlsErrLedsInterference) => "err_leds_interference",
        Err(_) => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_front_als_payload_deserialize() {
        // Actual format from the MCU via zenoh (snake_case field names)
        let json = r#"{"FrontAls":{"ambient_light_lux":17,"flag":0}}"#;
        let payload: Payload = serde_json::from_str(json).unwrap();
        match payload {
            Payload::FrontAls(als) => {
                assert_eq!(als.ambient_light_lux, 17);
                assert_eq!(als.flag, 0); // AlsOk
            }
            _ => panic!("Expected FrontAls variant"),
        }
    }

    #[test]
    fn test_front_als_payload_err_range() {
        let json = r#"{"FrontAls":{"ambient_light_lux":500,"flag":1}}"#;
        let payload: Payload = serde_json::from_str(json).unwrap();
        match payload {
            Payload::FrontAls(als) => {
                assert_eq!(als.ambient_light_lux, 500);
                assert_eq!(als.flag, 1); // AlsErrRange
            }
            _ => panic!("Expected FrontAls variant"),
        }
    }

    #[test]
    fn test_front_als_payload_leds_interference() {
        let json = r#"{"FrontAls":{"ambient_light_lux":0,"flag":2}}"#;
        let payload: Payload = serde_json::from_str(json).unwrap();
        match payload {
            Payload::FrontAls(als) => {
                assert_eq!(als.ambient_light_lux, 0);
                assert_eq!(als.flag, 2); // AlsErrLedsInterference
            }
            _ => panic!("Expected FrontAls variant"),
        }
    }

    #[test]
    fn test_flag_to_api_str() {
        assert_eq!(flag_to_api_str(0), "ok");
        assert_eq!(flag_to_api_str(1), "err_range");
        assert_eq!(flag_to_api_str(2), "err_leds_interference");
        assert_eq!(flag_to_api_str(99), "unknown");
    }

    #[test]
    fn test_ambient_light_default() {
        let als = AmbientLight::default();
        assert_eq!(als.ambient_light_lux, 0);
        assert_eq!(als.flag, 0);
    }
}
