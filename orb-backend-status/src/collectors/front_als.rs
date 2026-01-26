use crate::dbus::intf_impl::BackendStatusImpl;
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

/// The zenoh key expression for front ALS (Ambient Light Sensor).
pub const FRONT_ALS_KEY_EXPR: &str = "mcu/main/front_als";

/// Wrapper for the FrontAls payload from protobuf oneof.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FrontAlsPayload {
    #[serde(rename = "FrontAls")]
    pub front_als: AmbientLight,
}

/// Ambient light sensor data from the front unit.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AmbientLight {
    /// Ambient light in lux (approximate, sensor is behind the Orb face).
    #[serde(alias = "ambientLightLux")]
    pub ambient_light_lux: u32,
    /// Status flag from the sensor.
    pub flag: AmbientLightFlag,
}

/// Ambient light sensor status flags.
/// Supports both integer (from protobuf) and string representations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AmbientLightFlag {
    #[default]
    AlsOk,
    /// Likely too much light in the sensor, consider the value as ~500.
    AlsErrRange,
    /// Front LEDs are turned on, interfering with ALS so value cannot be trusted.
    AlsErrLedsInterference,
}

impl<'de> serde::Deserialize<'de> for AmbientLightFlag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum FlagValue {
            Int(i32),
            Str(String),
        }

        match FlagValue::deserialize(deserializer)? {
            FlagValue::Int(0) => Ok(AmbientLightFlag::AlsOk),
            FlagValue::Int(1) => Ok(AmbientLightFlag::AlsErrRange),
            FlagValue::Int(2) => Ok(AmbientLightFlag::AlsErrLedsInterference),
            FlagValue::Int(n) => {
                Err(D::Error::custom(format!("unknown flag value: {n}")))
            }
            FlagValue::Str(s) => match s.as_str() {
                "ALS_OK" => Ok(AmbientLightFlag::AlsOk),
                "ALS_ERR_RANGE" => Ok(AmbientLightFlag::AlsErrRange),
                "ALS_ERR_LEDS_INTERFERENCE" => {
                    Ok(AmbientLightFlag::AlsErrLedsInterference)
                }
                _ => Err(D::Error::custom(format!("unknown flag string: {s}"))),
            },
        }
    }
}

impl serde::Serialize for AmbientLightFlag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as string for our API output
        serializer.serialize_str(self.as_api_str())
    }
}

impl AmbientLightFlag {
    /// Convert to a human-readable string for the API.
    pub fn as_api_str(&self) -> &'static str {
        match self {
            AmbientLightFlag::AlsOk => "ok",
            AmbientLightFlag::AlsErrRange => "err_range",
            AmbientLightFlag::AlsErrLedsInterference => "err_leds_interference",
        }
    }
}

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

    let payload = match sample.payload().try_to_string() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to convert payload to string for key {key}: {e}");
            return Ok(());
        }
    };

    let wrapper: FrontAlsPayload = match serde_json::from_str(payload.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to parse FrontAlsPayload for key {key}: {e}, payload: {payload}");
            return Ok(());
        }
    };
    let als = wrapper.front_als;

    let mut current = ctx.current.lock().await;
    if current.as_ref() != Some(&als) {
        debug!(
            "front_als: lux={}, flag={}",
            als.ambient_light_lux,
            als.flag.as_api_str()
        );
    }
    *current = Some(als.clone());

    ctx.backend_status.update_front_als(Some(als));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_front_als_payload_deserialize() {
        // Actual format from the MCU via zenoh
        let json = r#"{"FrontAls":{"ambient_light_lux":17,"flag":0}}"#;
        let payload: FrontAlsPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.front_als.ambient_light_lux, 17);
        assert_eq!(payload.front_als.flag, AmbientLightFlag::AlsOk);
    }

    #[test]
    fn test_front_als_payload_err_range() {
        let json = r#"{"FrontAls":{"ambient_light_lux":500,"flag":1}}"#;
        let payload: FrontAlsPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.front_als.ambient_light_lux, 500);
        assert_eq!(payload.front_als.flag, AmbientLightFlag::AlsErrRange);
    }

    #[test]
    fn test_front_als_payload_leds_interference() {
        let json = r#"{"FrontAls":{"ambient_light_lux":0,"flag":2}}"#;
        let payload: FrontAlsPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.front_als.ambient_light_lux, 0);
        assert_eq!(
            payload.front_als.flag,
            AmbientLightFlag::AlsErrLedsInterference
        );
    }

    #[test]
    fn test_ambient_light_deserialize_direct() {
        // Direct AmbientLight struct (for internal use)
        let json = r#"{"ambient_light_lux": 200, "flag": 0}"#;
        let als: AmbientLight = serde_json::from_str(json).unwrap();
        assert_eq!(als.ambient_light_lux, 200);
        assert_eq!(als.flag, AmbientLightFlag::AlsOk);
    }

    #[test]
    fn test_ambient_light_flag_as_api_str() {
        assert_eq!(AmbientLightFlag::AlsOk.as_api_str(), "ok");
        assert_eq!(AmbientLightFlag::AlsErrRange.as_api_str(), "err_range");
        assert_eq!(
            AmbientLightFlag::AlsErrLedsInterference.as_api_str(),
            "err_leds_interference"
        );
    }

    #[test]
    fn test_ambient_light_default() {
        let als = AmbientLight::default();
        assert_eq!(als.ambient_light_lux, 0);
        assert_eq!(als.flag, AmbientLightFlag::AlsOk);
    }

    #[test]
    fn test_ambient_light_equality() {
        let als1 = AmbientLight {
            ambient_light_lux: 100,
            flag: AmbientLightFlag::AlsOk,
        };
        let als2 = AmbientLight {
            ambient_light_lux: 100,
            flag: AmbientLightFlag::AlsOk,
        };
        let als3 = AmbientLight {
            ambient_light_lux: 200,
            flag: AmbientLightFlag::AlsErrRange,
        };
        assert_eq!(als1, als2);
        assert_ne!(als1, als3);
    }
}
