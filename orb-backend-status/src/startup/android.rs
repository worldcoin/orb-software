use super::Config;
use android_system_properties::AndroidSystemProperties;
use clap::Parser;
use color_eyre::{
    eyre::{ensure, eyre, Context},
    Result,
};
use orb_backend_status::BUILD_INFO;
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{OrbId, OrbJabilId, OrbName};
use secrecy::SecretString;
use std::time::Duration;
use tracing::{debug, info, warn};
use zenorb::{zenoh::Wait, Zenorb};

#[derive(Parser)]
#[command(version = BUILD_INFO.version, about = "Forward Android OES events to the backend")]
pub struct Args;

fn zenoh_config() -> Result<zenorb::zenoh::Config> {
    zenorb::zenoh::Config::from_json5(
        r#"{
            mode: "client",
            connect: { endpoints: ["unixsock-stream//dev/socket/zenohd/zenohd.sock"] },
            listen: { endpoints: [] },
            scouting: {
                multicast: { enabled: false },
                gossip: { enabled: false },
            },
        }"#,
    )
    .map_err(|e| eyre!("invalid Android Zenoh configuration: {e}"))
}

pub async fn configure(orb_id: OrbId) -> Result<Config> {
    let orb_os_version = tokio::task::spawn_blocking(|| {
        AndroidSystemProperties::new().get("ro.build.fingerprint")
    })
    .await
    .wrap_err("Android property reader task failed")?
    .filter(|version| !version.trim().is_empty())
    .unwrap_or_else(|| {
        warn!("Android build fingerprint unavailable or empty. Using [unknown]");
        String::from("unknown")
    });

    let endpoint = Endpoints::new(Backend::Staging, &orb_id).status;
    let zenoh = zenoh_config()?;
    Ok(Config {
        orb_id,
        orb_name: OrbName::read_unfallable().await,
        orb_jabil_id: OrbJabilId("unknown".to_owned()),
        orb_os_version,
        endpoint,
        zenoh,
        metrics_socket: Some(
            "/data/vendor/datadog-agent-tfh/run/dsd.socket".to_owned(),
        ),
    })
}

pub async fn wait_for_token(session: &Zenorb) -> SecretString {
    info!("waiting for a backend token from protest");
    loop {
        match query_token(session).await {
            Ok(token) => return token,
            Err(error) => debug!(%error, "protest token unavailable; retrying"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn query_token(session: &Zenorb) -> Result<SecretString> {
    let session = session.clone();
    let replies = tokio::task::spawn_blocking(move || {
        session
            .get("protest/token")
            .timeout(Duration::from_secs(5))
            .wait()
    })
    .await?
    .map_err(|error| eyre!("failed to query protest: {error}"))?;
    let reply = replies
        .recv_async()
        .await
        .map_err(|error| eyre!("no reply from protest: {error}"))?
        .into_result()
        .map_err(|_| eyre!("protest returned a token error"))?;
    let value = reply
        .payload()
        .try_to_string()
        .map_err(|_| eyre!("protest token is not UTF-8"))?;
    let token = value.trim();
    ensure!(!token.is_empty(), "protest token is empty");
    Ok(SecretString::new(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_info::orb_id::test_orb_id;
    use secrecy::ExposeSecret;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn waits_for_provisioning() {
        let mut cfg = zenorb::router_cfg(0);
        cfg.insert_json5("listen/endpoints", "[]").unwrap();
        let session = Zenorb::from_cfg(cfg)
            .orb_id(test_orb_id())
            .liveliness(false)
            .with_name("orb-backend-status")
            .await
            .unwrap();
        assert!(query_token(&session).await.is_err());

        let responder = session
            .session()
            .declare_queryable(format!("{}/protest/token", session.orb_id()))
            .await
            .unwrap();
        let provision = async {
            let query = responder.recv_async().await.unwrap();
            query.reply_err("no token stored").await.unwrap();
            let query = responder.recv_async().await.unwrap();
            query
                .reply(query.key_expr(), "test-token\r\n")
                .await
                .unwrap();
        };
        let (token, ()) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(wait_for_token(&session), provision)
        })
        .await
        .unwrap();
        assert_eq!(token.expose_secret(), "test-token");
    }
}
