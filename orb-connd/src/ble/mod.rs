use bluer::adv::{Advertisement, Type};
use color_eyre::{eyre::eyre, Result};
use serde::{Deserialize, Serialize};
use speare::mini;
use std::collections::BTreeMap;
use tracing::{error, info, warn};
use uuid::Uuid;
use zenorb::Zenorb;

pub struct Args {
    pub zenoh: Zenorb,
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

    let active = adapter.active_advertising_instances().await?;
    let supported = adapter.supported_advertising_instances().await?;

    info!("ble advertising instances. active: {active}, supported: {supported}");

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
        .declare_subscriber("ble_beacon")
        .await
        .map_err(|e| eyre!("{e}"))?;

    let mut service_data = BTreeMap::new();
    let mut _advertisement_handle = None;

    loop {
        let sample = subscriber
            .recv_async()
            .await
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
                service_data.remove(&advert.service_id);
            }

            Some(payload) => {
                service_data.insert(advert.service_id, payload);
            }
        }

        match (service_data.is_empty(), &_advertisement_handle) {
            (true, None) => (),

            (true, Some(_)) => {
                _advertisement_handle = None;
            }

            (false, _) => {
                _advertisement_handle = None; // force drop

                let ids: String = service_data.keys().map(|x| x.to_string()).collect();
                info!("advertising ble broadcast for services: {ids}");

                let advertisement = Advertisement {
                    advertisement_type: Type::Broadcast,
                    service_data: service_data.clone(),
                    ..Default::default()
                };

                _advertisement_handle = Some(adapter.advertise(advertisement).await?);
            }
        }
    }
}
