use bluer::adv::{Advertisement, Type};
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
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

pub struct Args {
    pub zenoh: Zenorb,
    pub interval: Duration,
}

#[derive(Serialize, Deserialize)]
struct Advert {
    service_id: Uuid,
    payload: Option<Vec<u8>>,
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
    let mut last_payload = None;
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
                        service_data.insert(advert.service_id, payload);
                    }
                }
            }

            _ = interval.tick() => {
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

                info!("advertising ble broadcast for service: {service_id}");

                let advertisement = Advertisement {
                    advertisement_type: Type::Broadcast,
                    // Nice to have only: BlueZ may truncate this if ServiceData needs the room.
                    local_name: Some("Orb".to_owned()),
                    service_data: BTreeMap::from([(service_id, payload.clone())]),
                    timeout: Some(Duration::ZERO),
                    ..Default::default()
                };

                advertisement_handle = Some(adapter.advertise(advertisement).await?);
                last_service_id = Some(service_id);
                last_payload = Some(payload.clone());
            }
        }
    }
}
