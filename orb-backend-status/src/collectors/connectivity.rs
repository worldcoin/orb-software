use super::ZenorbCtx;
use color_eyre::Result;
use orb_connd_events::Connection;
use rkyv::AlignedVec;
use tracing::debug;
use zenorb::zenoh;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalConnectivity {
    Connected { ssid: Option<String> },
    NotConnected,
}

impl GlobalConnectivity {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }

    pub fn ssid(&self) -> Option<&str> {
        match self {
            Self::Connected { ssid } => ssid.as_deref(),
            Self::NotConnected => None,
        }
    }
}

pub(crate) async fn handle_connection_event(
    ctx: ZenorbCtx,
    sample: zenoh::sample::Sample,
) -> Result<()> {
    let payload = sample.payload().to_bytes();
    let mut bytes = AlignedVec::with_capacity(payload.len());
    bytes.extend_from_slice(&payload);

    let archived = rkyv::check_archived_root::<Connection>(&bytes)
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

    let connectivity = match archived {
        orb_connd_events::ArchivedConnection::ConnectedGlobal(kind) => {
            let ssid = match kind {
                orb_connd_events::ArchivedConnectionKind::Wifi { ssid } => {
                    Some(ssid.to_string())
                }
                orb_connd_events::ArchivedConnectionKind::Ethernet
                | orb_connd_events::ArchivedConnectionKind::Cellular { .. } => None,
            };
            GlobalConnectivity::Connected { ssid }
        }
        _ => GlobalConnectivity::NotConnected,
    };

    let prev = ctx.connectivity_tx.borrow().clone();
    if prev != connectivity {
        debug!("global connectivity changed: {connectivity:?}");

        let prev_ssid = prev.ssid();
        let new_ssid = connectivity.ssid();
        if prev_ssid != new_ssid {
            ctx.backend_status
                .update_active_ssid(new_ssid.map(String::from));
            ctx.backend_status.set_send_immediately();
        }
    }

    ctx.connectivity_tx
        .send(connectivity)
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

    Ok(())
}
