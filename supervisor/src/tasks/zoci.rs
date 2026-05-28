use color_eyre::Result;
use orb_info::orb_os_release::{OrbOsPlatform, OrbOsRelease, OrbRelease};
use serde::Deserialize;
use std::borrow::Cow;
use tokio::task::JoinHandle;
use tracing::{info, instrument, warn};
use zbus_systemd::systemd1::ManagerProxy;
use zenorb::{zenoh::query::Query, zoci::ZociQueryExt, Zenorb};

pub const UPDATE_AGENT_VERSION: &str = "ORB_UPDATE_AGENT_VERSION_OVERWRITE";

#[derive(Deserialize)]
struct UpdateRequest {
    pub version: String,
    #[serde(default)]
    pub restart: bool,
}

impl TryFrom<&Query> for UpdateRequest {
    type Error = color_eyre::Report;

    fn try_from(query: &Query) -> Result<Self> {
        match query.json::<UpdateRequest>() {
            Ok(req) => Ok(req),
            Err(_) => Ok(UpdateRequest {
                version: query.payload_str()?.into_owned(),
                restart: false,
            }),
        }
    }
}

#[derive(Clone)]
pub struct ZociContext {
    pub os_release: OrbOsRelease,
    pub system_conn: zbus::Connection,
    pub update_agent_unit: &'static str,
    pub target_version_env: &'static str,
}

pub async fn spawn_zoci_receiver(
    zenorb: &Zenorb,
    ctx: ZociContext,
) -> Result<Vec<JoinHandle<()>>> {
    zenorb
        .receiver(ctx)
        .queryable("job/gondor", run_handler)
        .run()
        .await
}

async fn export_update_version(
    conn: &zbus::Connection,
    var: &str,
    value: &str,
) -> Result<()> {
    let manager = ManagerProxy::new(conn).await?;
    manager
        .set_environment(vec![format!("{var}={value}")])
        .await?;
    Ok(())
}

async fn restart_unit(conn: &zbus::Connection, unit: &str) -> Result<()> {
    let manager = ManagerProxy::new(conn).await?;
    let _job_path = manager.restart_unit(unit.into(), "replace".into()).await?;
    Ok(())
}

/// Returns a [`Cow<'_, str>`] of the expected release version
///
/// if the provided version matches the expected version format of `*-<platform>-<release>`, it is
/// returned as is; If the suffix validation fails, the proper suffix is concatenated to `version`
/// using metadata retrieved from the running system
fn derive_version(
    target: &str,
    platform: OrbOsPlatform,
    release: OrbRelease,
) -> Cow<'_, str> {
    let mut suffix = target.rsplitn(3, '-');
    let already_suffixed = matches!(
        (suffix.next(), suffix.next(), suffix.next()),
        (Some(r), Some(p), Some(rest))
            if !rest.is_empty()
                && r.parse::<OrbRelease>().is_ok()
                && p.parse::<OrbOsPlatform>().is_ok()
    );
    if already_suffixed {
        target.into()
    } else {
        format!("{target}-{platform}-{release}").into()
    }
}

async fn run_handler(ctx: ZociContext, query: Query) -> Result<()> {
    let response = trigger_update(&ctx, &query).await.map_err(|e| {
        warn!("`gondor` handler failed: {e}");
        e.to_string()
    });

    query.res(response).await?;

    Ok(())
}

#[instrument(skip(ctx, query))]
async fn trigger_update(ctx: &ZociContext, query: &Query) -> Result<()> {
    let update_to = UpdateRequest::try_from(query)?;

    let version = derive_version(
        &update_to.version,
        ctx.os_release.orb_os_platform_type,
        ctx.os_release.release_type,
    );

    info!(
        "`gondor` handler executing with: version={} & restart={}",
        version, update_to.restart
    );

    export_update_version(&ctx.system_conn, ctx.target_version_env, &version).await?;

    if update_to.restart {
        restart_unit(&ctx.system_conn, ctx.update_agent_unit).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_when_input_carries_a_valid_suffix() {
        assert_eq!(
            derive_version(
                "to-0.0.0-abcabc-diamond-stage",
                OrbOsPlatform::Diamond,
                OrbRelease::Prod,
            )
            .as_ref(),
            "to-0.0.0-abcabc-diamond-stage",
        );
    }

    #[test]
    fn appends_suffix_when_missing() {
        assert_eq!(
            derive_version("to-main", OrbOsPlatform::Diamond, OrbRelease::Prod)
                .as_ref(),
            "to-main-diamond-prod",
        );
    }

    #[test]
    fn appends_when_last_two_segments_do_not_parse_as_enums() {
        assert_eq!(
            derive_version(
                "to-feature-foo-bar",
                OrbOsPlatform::Diamond,
                OrbRelease::Prod,
            )
            .as_ref(),
            "to-feature-foo-bar-diamond-prod",
        );
    }
}
