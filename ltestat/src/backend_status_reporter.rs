use crate::{
    modem::Modem,
    utils::{retry_for, State},
};
use color_eyre::{eyre::eyre, Result};
use orb_backend_status_dbus::{types, BackendStatusProxy};
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use zbus::Connection;

pub fn start(modem: State<Modem>, report_interval: Duration) -> JoinHandle<Result<()>> {
    task::spawn(async move {
        let be_status: BackendStatusProxy<'_> =
            retry_for(Duration::MAX, Duration::from_secs(20), async || {
                let conn = Connection::system()
                    .await
                    .inspect_err(|e| println!("TODO: {e}"))?;

                let proxy = BackendStatusProxy::new(&conn)
                    .await
                    .inspect_err(|e| println!("TODO: {e}"))?;

                Ok(proxy)
            })
            .await?;

        loop {
            let result: Result<()> = async {
               let x = modem.read(|m| m.id.clone())
                .map_err(|e| {
                        eyre!("failed to read ConnectionState from State<Modem>: {e:?}")
                    })?;

                Ok(())
            }.await;

            if let Err(e) = result {
                println!("failed to repot to backend status: {e}");
            }
        }
    })
}

async fn report(modem: State<Modem>, be_status: BackendStatusProxy<'_>) -> Result<()> {
    be_status.provide_lte_info(lte_info)

    Ok(())
}
