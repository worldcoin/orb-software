use bluer::adv::{Advertisement, Type};
use color_eyre::{eyre::eyre, Result};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use speare::mini;
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Unbounded},
    time::Duration,
};
use tokio::time::{self, MissedTickBehavior};
use tracing::{error, info, warn};
use uuid::Uuid;
use zenorb::Zenorb;

const MAX_ADVERTISEMENT_LEN: usize = 31;
const MAX_PAYLOAD_LEN: usize = 10;
const INTERVAL_SUPPORT_RETRY_DELAY: Duration = Duration::from_secs(30);

pub struct Args {
    pub zenoh: Zenorb,
    pub interval: Duration,
}

#[derive(Serialize, Deserialize)]
struct Advert {
    service_id: Uuid,
    payload: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "deserialize_advertising_interval")]
    advertising_interval_ms: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
struct ServiceData {
    payload: Vec<u8>,
    advertising_interval: Option<Duration>,
}

fn deserialize_advertising_interval<'de, D>(
    deserializer: D,
) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let interval = Option::<u64>::deserialize(deserializer)?;
    if interval.is_some_and(|value| !(20..=10_240).contains(&value)) {
        return Err(D::Error::custom(
            "advertising_interval_ms must be between 20 and 10240 milliseconds",
        ));
    }
    Ok(interval)
}

async fn interval_override_supported(adapter: &bluer::Adapter) -> Option<bool> {
    // BlueZ 5.64 hides this property when experimental APIs are off.
    // Diamond's Linux 5.15 also provides the required extended advertising commands.
    match adapter.supported_advertising_features().await {
        Ok(features) => {
            let supported = features.is_some();
            if !supported {
                warn!("BLE interval override is unavailable; using adapter defaults. Install a Diamond OS with BlueZ experimental advertising support");
            }
            Some(supported)
        }
        Err(error) => {
            warn!(%error, "failed to check BLE interval override support; using adapter defaults and retrying in 30 seconds");
            None
        }
    }
}

pub async fn advertiser(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting ble advertiser");

    let session = bluer::Session::new()
        .await
        .inspect_err(|e| warn!("failed to create bluer session: {e}"))?;

    let adapter = match session.default_adapter().await {
        Err(bluer::Error {
            kind: bluer::ErrorKind::NotFound,
            ..
        }) => {
            warn!("no bluetooth adapter found. ble advertiser task will quit early.");
            return Ok(());
        }

        Err(e) => {
            error!("failed to acquire default ble adapter: {e:?}");
            return Err(e.into());
        }

        Ok(a) => a,
    };

    if !adapter
        .is_powered()
        .await
        .inspect_err(|e| warn!("failed to check ble adapter power status: {e}"))?
    {
        adapter
            .set_powered(true)
            .await
            .inspect_err(|e| warn!("failed to power ble adapter: {e}"))?;
    }

    let subscriber = ctx
        .zenoh
        .declare_subscriber("*/ble_beacon")
        .await
        .map_err(|e| eyre!("{e}"))?;
    info!(
        "BLE advertisement size limits: max advertisement {MAX_ADVERTISEMENT_LEN} bytes, max payload {MAX_PAYLOAD_LEN} bytes"
    );

    let mut service_data = BTreeMap::new();
    let mut advertisement_handle = None;
    let mut last_service_id = None;
    let mut last_service_data = None;
    let mut supports_interval_override = None;
    let mut next_interval_support_probe = time::Instant::now();
    let mut interval = time::interval(ctx.interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            sample = subscriber.recv_async() => {
                let sample = sample
                    .map_err(|e| eyre!("{e}"))
                    .inspect_err(|e| {
                        warn!("ble advertiser failed receive zenoh sample: {e}")
                    })?;

                let payload = sample.payload().to_bytes();
                let advert = match serde_json::from_slice::<Advert>(&payload) {
                    Err(e) => {
                        warn!("ble advertiser received malformed json: {e}");
                        continue;
                    }

                    Ok(p) => p,
                };

                match advert.payload {
                    None => {
                        info!("removing ble advert from service: {}", advert.service_id);
                        service_data.remove(&advert.service_id);
                    }

                    Some(payload) if payload.len() > MAX_PAYLOAD_LEN => {
                        warn!(
                            "ignoring oversized BLE advertisement payload for {}: {} bytes exceeds {MAX_PAYLOAD_LEN} byte maximum",
                            advert.service_id,
                            payload.len(),
                        );
                    }

                    Some(payload) => {
                        info!("adding ble advert for service: {}", advert.service_id);
                        let advertising_interval =
                            advert.advertising_interval_ms.map(Duration::from_millis);
                        service_data.insert(advert.service_id, ServiceData {
                            payload,
                            advertising_interval,
                        });
                    }
                }
            }

            _ = interval.tick() => {
                let next = last_service_id
                    .and_then(|id| service_data.range((Excluded(id), Unbounded)).next())
                    .or_else(|| service_data.first_key_value());

                let Some((&service_id, data)) = next else {
                    drop(advertisement_handle.take());
                    last_service_id = None;
                    last_service_data = None;
                    continue;
                };

                if data.advertising_interval.is_some()
                    && supports_interval_override.is_none()
                    && time::Instant::now() >= next_interval_support_probe
                {
                    supports_interval_override = interval_override_supported(&adapter).await;
                    next_interval_support_probe = time::Instant::now() + INTERVAL_SUPPORT_RETRY_DELAY;
                }
                let mut data = ServiceData {
                    payload: data.payload.clone(),
                    advertising_interval: data.advertising_interval
                        .filter(|_| supports_interval_override == Some(true)),
                };

                let advertisement_unchanged = last_service_id == Some(service_id)
                    && last_service_data.as_ref() == Some(&data);

                if advertisement_unchanged {
                    continue;
                }

                // Dropping the handle stops advertising asynchronously. The next advertisement
                // may briefly overlap with the old one; our module supports up to 8 at once.
                drop(advertisement_handle.take());

                let advertisement = Advertisement {
                    advertisement_type: Type::Broadcast,
                    service_data: BTreeMap::from([(service_id, data.payload.clone())]),
                    min_interval: data.advertising_interval,
                    max_interval: data.advertising_interval,
                    timeout: Some(Duration::ZERO),
                    ..Default::default()
                };

                advertisement_handle = Some(match adapter.advertise(advertisement.clone()).await {
                    Ok(handle) => handle,
                    Err(error) if data.advertising_interval.is_some() => {
                        warn!(%error, advertising_interval_ms = ?data.advertising_interval.map(|value| value.as_millis()),
                            "BLE interval override registration failed; using adapter defaults and retrying in 30 seconds");
                        data.advertising_interval = None;
                        supports_interval_override = None;
                        next_interval_support_probe = time::Instant::now() + INTERVAL_SUPPORT_RETRY_DELAY;
                        adapter.advertise(Advertisement {
                            min_interval: None,
                            max_interval: None,
                            ..advertisement
                        }).await?
                    }
                    Err(error) => return Err(error.into()),
                });
                last_service_id = Some(service_id);
                last_service_data = Some(data.clone());
                info!(?service_id, advertising_interval_ms = ?data.advertising_interval.map(|value| value.as_millis()), "registered BLE advertisement");
            }
        }
    }
}
