use crate::systemd::Systemd;
use bluer::adv::{Advertisement, Type};
use color_eyre::{eyre::eyre, Result};
use serde::{de::Error as _, Deserialize, Deserializer};
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
const RETRY_DELAY: Duration = Duration::from_secs(30);

pub struct Args {
    pub zenoh: Zenorb,
    pub systemd: Systemd,
    pub interval: Duration,
}

#[derive(Deserialize)]
struct Advert {
    service_id: Uuid,
    payload: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "deserialize_advertising_interval")]
    advertising_interval_ms: Option<Option<u64>>,
}

fn deserialize_advertising_interval<'de, D>(
    deserializer: D,
) -> Result<Option<Option<u64>>, D::Error>
where
    D: Deserializer<'de>,
{
    let interval = Option::<u64>::deserialize(deserializer)?;
    if interval.is_some_and(|value| !(20..=10_240).contains(&value)) {
        return Err(D::Error::custom(
            "advertising_interval_ms must be between 20 and 10240 milliseconds",
        ));
    }
    Ok(Some(interval))
}

async fn connect_bluetooth() -> Result<Option<(bluer::Session, bluer::Adapter)>> {
    let session = bluer::Session::new()
        .await
        .inspect_err(|e| warn!("failed to create bluer session: {e}"))?;

    let adapter = match session.default_adapter().await {
        Err(bluer::Error {
            kind: bluer::ErrorKind::NotFound,
            ..
        }) => {
            warn!("no bluetooth adapter found");
            return Ok(None);
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

    Ok(Some((session, adapter)))
}

pub async fn advertiser(ctx: mini::Ctx<Args>) -> Result<()> {
    info!("starting ble advertiser");

    let mut bluetooth = connect_bluetooth().await?;
    if bluetooth.is_none() {
        return Ok(());
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
    let mut last_payload = None;
    let mut requested_interval = None;
    let mut applied_interval = None;
    let mut next_apply = time::Instant::now();
    let mut next_connect = time::Instant::now();
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
                        if let Some(requested) = advert.advertising_interval_ms
                            && requested_interval != Some(requested)
                        {
                            requested_interval = Some(requested);
                            next_apply = time::Instant::now();
                        }
                        info!("adding ble advert for service: {}", advert.service_id);
                        service_data.insert(advert.service_id, payload);
                    }
                }
            }

            _ = interval.tick() => {
                if let Some(requested) = requested_interval
                    && applied_interval != Some(requested)
                    && time::Instant::now() >= next_apply
                {
                    let instance = requested.map_or_else(|| "default".to_owned(), |ms| ms.to_string());
                    let unit = format!("worldcoin-bluetooth-interval@{instance}.service");
                    applied_interval = match ctx.systemd.restart_service(&unit, Duration::from_secs(20)).await {
                        Ok(()) => {
                            info!(advertising_interval_ms = %instance, "configured BLE adapter advertising interval");
                            Some(requested)
                        },
                        Err(error) => {
                            warn!(%error, %unit, "failed to apply BLE interval; retrying in 30 seconds");
                            None
                        }
                    };
                    next_apply = time::Instant::now() + RETRY_DELAY;
                    // The helper can restart BlueZ even when a later step fails.
                    drop(advertisement_handle.take());
                    bluetooth = None;
                    last_service_id = None;
                    last_payload = None;
                    next_connect = time::Instant::now();
                }
                if bluetooth.is_none() && time::Instant::now() >= next_connect {
                    bluetooth = match connect_bluetooth().await {
                        Ok(bluetooth) => {
                            next_connect = time::Instant::now() + ctx.interval;
                            bluetooth
                        }
                        Err(error) => {
                            warn!(%error, "failed to reconnect to BlueZ; retrying in 30 seconds");
                            next_connect = time::Instant::now() + RETRY_DELAY;
                            None
                        }
                    };
                }
                let Some((_, adapter)) = &bluetooth else { continue };

                let next = last_service_id
                    .and_then(|id| service_data.range((Excluded(id), Unbounded)).next())
                    .or_else(|| service_data.first_key_value());

                let Some((&service_id, payload)) = next else {
                    drop(advertisement_handle.take());
                    last_service_id = None;
                    last_payload = None;
                    continue;
                };

                let advertisement_unchanged = last_service_id == Some(service_id)
                    && last_payload.as_ref() == Some(payload);

                if advertisement_unchanged {
                    continue;
                }

                // Dropping the handle stops advertising asynchronously. The next advertisement
                // may briefly overlap with the old one; our module supports up to 8 at once.
                drop(advertisement_handle.take());

                let advertisement = Advertisement {
                    advertisement_type: Type::Broadcast,
                    service_data: BTreeMap::from([(service_id, payload.clone())]),
                    timeout: Some(Duration::ZERO),
                    ..Default::default()
                };

                match adapter.advertise(advertisement).await {
                    Ok(handle) => {
                        advertisement_handle = Some(handle);
                        last_service_id = Some(service_id);
                        last_payload = Some(payload.clone());
                    }
                    Err(error) => {
                        warn!(%error, "failed to register BLE advertisement; retrying in 30 seconds");
                        bluetooth = None;
                        last_service_id = None;
                        last_payload = None;
                        next_connect = time::Instant::now() + RETRY_DELAY;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Advert;
    use serde_json::{from_value, json};

    #[test]
    fn advertising_interval_distinguishes_omission_reset_and_valid_values() {
        let mut message =
            json!({"service_id": super::Uuid::nil(), "payload": [1, 2, 3]});
        let parsed: Advert = from_value(message.clone()).unwrap();
        assert_eq!(parsed.advertising_interval_ms, None);
        for value in [json!(null), json!(20), json!(10_240)] {
            message["advertising_interval_ms"] = value.clone();
            let parsed: Advert = from_value(message.clone()).unwrap();
            assert_eq!(parsed.advertising_interval_ms, Some(value.as_u64()));
        }
        for value in [
            json!(0),
            json!(19),
            json!(10_241),
            json!(-1),
            json!(20.5),
            json!("20"),
        ] {
            message["advertising_interval_ms"] = value;
            assert!(from_value::<Advert>(message.clone()).is_err(), "{message}");
        }
    }
}
