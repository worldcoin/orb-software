#![allow(clippy::uninlined_format_args)]
use std::{
    borrow::Cow,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use eyre::{ensure, WrapErr as _};
use orb_update_agent_core::{
    reexports::ed25519_dalek::VerifyingKey, Claim, ClaimVerificationContext,
    LocalOrRemote, Slot, Source, VersionMap,
};
use reqwest::{StatusCode, Url};
use tracing::{debug, info, warn};

use crate::{
    settings::{Backend, Settings},
    util,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed deserializing claim from json")]
    ReadJson(#[source] crate::json::Error),
    #[error("failed opening to read claim: `{}`", .path.display())]
    OpenPath { path: PathBuf, source: io::Error },
    #[error("failed initializing client to check for remote updates")]
    InitClient(#[from] crate::client::Error),
    #[error("failed to request an auth token from DBus: `{}`", .0)]
    DBusToken(#[source] zbus::Error),
    #[error("short-lived-token-daemon responded with empty token and errmsg: `{}`", .0)]
    DBusTokenNotAvailable(String),
    #[error(
        "No auth token provided: If running a remote update with --nodbus, please add --token \
         $(cat /usr/persistent/token)"
    )]
    NoAuthTokenProvided(),
    #[error("failed sending check update request")]
    SendCheckUpdateRequest(#[source] reqwest::Error),
    #[error("failed getting the check update response as text")]
    ResponseAsText(#[source] reqwest::Error),
    #[error("failed reading claim from path: {}", .path.display())]
    Local { path: PathBuf, source: Box<Error> },
    #[error("failed fetching claim from remote: {url}")]
    Remote { url: Url, source: Box<Error> },
    #[error("failed sending dbus request to check if downloads are allowed")]
    DbusRequest(#[source] zbus::Error),
    #[error("supervisor did not allow downloads; reason {reason}")]
    DownloadNotAllowed { reason: String },
    #[error("server responded with status `{status_code}` and msg: {msg}")]
    StatusCode {
        status_code: StatusCode,
        msg: String,
    },
    #[error("Unable to determine current version for slot {slot:?} - release version not set in version map")]
    MissingSlotVersion { slot: Slot },
    #[error("no new version available - system is up to date")]
    NoNewVersion,
}

impl Error {
    fn open_path(path: &Path, source: io::Error) -> Self {
        Self::OpenPath {
            path: path.to_owned(),
            source,
        }
    }

    fn local(path: &Path, source: impl Into<Box<Self>>) -> Self {
        Self::Local {
            path: path.to_owned(),
            source: source.into(),
        }
    }

    fn remote(url: &Url, source: impl Into<Box<Self>>) -> Self {
        Self::Remote {
            url: url.clone(),
            source: source.into(),
        }
    }

    fn status_code(status_code: StatusCode, msg: impl ToString) -> Self {
        Self::StatusCode {
            status_code,
            msg: msg.to_string(),
        }
    }
}

fn make_claim_destination(settings: &Settings) -> PathBuf {
    settings.workspace.join("claim.json")
}

fn from_path(
    path: &Path,
    verify_manifest_signature_against: Backend,
) -> Result<Claim, Error> {
    let reader = File::open(path)
        .map_err(|e| Error::open_path(path, e))
        .map(io::BufReader::new)?;

    let claim_verification_context = ClaimVerificationContext(
        pubkey_from_backend_type(verify_manifest_signature_against),
    );

    crate::json::deserialize_seed::<_, _, Claim>(claim_verification_context, reader)
        .map_err(Error::ReadJson)
}

fn from_remote(
    id: &str,
    slot: Slot,
    url: &Url,
    version_map: &VersionMap,
    verify_manifest_signature_against: Backend,
) -> Result<Option<(String, Claim)>, Error> {
    let current_version = version_map
        .get_slot_version(slot)
        .ok_or_else(|| Error::MissingSlotVersion { slot })?;

    let mut api_url = url.clone();
    api_url.set_path(&format!("/api/v2/orbs/{}/claim", id));
    api_url
        .query_pairs_mut()
        .clear()
        .append_pair("currentVersion", current_version);

    debug!(
        "sending check request to: {} with currentVersion: {}",
        api_url, current_version
    );

    let client = crate::client::normal()?;
    let resp = client
        .get(api_url.clone())
        .send()
        .map_err(Error::SendCheckUpdateRequest)?;

    let status = resp.status();
    if status.is_client_error() || status.is_server_error() {
        let msg: Cow<'_, str> = match resp.text() {
            Ok(text) => Cow::Owned(text),
            Err(e) => {
                warn!(
                    "failed reading response body as text while handling error remote status \
                     code: {e:?}"
                );
                Cow::Borrowed("")
            }
        };

        if status == StatusCode::NOT_FOUND && !msg.is_empty() {
            Err(Error::NoNewVersion)
        } else {
            Err(Error::status_code(status, msg))
        }
    } else {
        let resp_txt = resp.text().map_err(Error::ResponseAsText)?;
        debug!("server sent raw claim: {resp_txt}");

        #[allow(clippy::collapsible_if)]
        if let Ok(response) = serde_json::from_str::<serde_json::Value>(&resp_txt) {
            if let Some(status) = response.get("status").and_then(|s| s.as_str()) {
                if status == "up_to_date" {
                    debug!("system is up to date - no update available");
                    return Ok(None);
                }
            }
        }

        let claim_verification_context = ClaimVerificationContext(
            pubkey_from_backend_type(verify_manifest_signature_against),
        );
        let claim = crate::json::deserialize_seed(
            claim_verification_context,
            resp_txt.as_bytes(),
        )
        .map_err(Error::ReadJson)?;
        Ok(Some((resp_txt, claim)))
    }
}

fn to_disk(path: &Path, raw_claim: &str) -> eyre::Result<()> {
    fs::write(path, raw_claim).wrap_err("failed writing raw claim to disk")
}

fn get_local_claim_if_fresh(
    claim_dst: &Path,
    settings: &Settings,
) -> eyre::Result<Claim> {
    ensure!(
        settings.recovery,
        "this is currently only done in recovery mode"
    );
    let claim_file =
        fs::File::open(claim_dst).wrap_err("failed opening local claim file")?;
    let metadata = claim_file
        .metadata()
        .wrap_err("failed getting metadata from claim")?;
    let last_modified = metadata
        .modified()
        .wrap_err("failed getting last modification time from claim metadata")?;
    let claim_age = last_modified.elapsed().wrap_err(
        "failed calculating elapsed time from claim modification system time",
    )?;
    ensure!(
        claim_age <= Duration::from_secs(60 * 60 * 24 * 7),
        "local claim is older than 7 days",
    );
    let claim_verification_context = ClaimVerificationContext(
        pubkey_from_backend_type(settings.verify_manifest_signature_against),
    );
    crate::json::deserialize_seed::<_, _, Claim>(claim_verification_context, claim_file)
        .wrap_err("failed reading claim from json")
}

fn ensure_source_matches_record(path: &Path, source: &Source) -> eyre::Result<()> {
    let metadata = fs::metadata(path).wrap_err_with(|| {
        format!(
            "failed reading metadata for component source {} at {}",
            source.name,
            path.display()
        )
    })?;
    ensure!(
        metadata.len() == source.size,
        "file size does not match size in record: expected {}, on-disk {}",
        source.size,
        metadata.len(),
    );
    Ok(())
}

fn ensure_sources_match_claim(
    settings: &Settings,
    claim: Claim,
) -> eyre::Result<Claim> {
    for (_, source) in claim.iter_components_with_location() {
        if source.is_local() {
            debug!(
                "source for `{}` points to local file; skipping",
                source.name,
            );
            continue;
        }
        let path =
            util::make_component_path(&settings.downloads, &source.unique_name());
        ensure_source_matches_record(&path, source).wrap_err_with(|| {
            format!("source for component {} does not match record", source.name)
        })?;
    }
    Ok(claim)
}

pub fn get(settings: &Settings, version_map: &VersionMap) -> Result<Claim, Error> {
    match &settings.update_location {
        LocalOrRemote::Remote(url) => {
            let path = make_claim_destination(settings);
            match get_local_claim_if_fresh(&path, settings)
                .and_then(|claim| ensure_sources_match_claim(settings, claim))
            {
                Err(e) => warn!("cannot progress with on-disk update claim: {e:?}"),
                Ok(claim) => return Ok(claim),
            }
            info!("checking remote update at {url}, for orb {}", &settings.id);
            let result = from_remote(
                &settings.id,
                settings.active_slot,
                url,
                version_map,
                settings.verify_manifest_signature_against,
            )
            .map_err(|e| Error::remote(url, e))?;

            match result {
                Some((raw_txt, claim)) => {
                    info!("writing raw remote update claim to disk");
                    if let Err(e) = to_disk(&path, &raw_txt) {
                        warn!(
                            "failed writing remote claim to `{}`: {e:?}",
                            path.display()
                        );
                    }
                    Ok(claim)
                }
                None => {
                    info!("no update available - system is up to date");
                    info!("returning NoNewVersion error");
                    Err(Error::NoNewVersion)
                }
            }
        }
        LocalOrRemote::Local(path) => {
            info!("reading local update claim at {}", path.display());
            from_path(path, settings.verify_manifest_signature_against)
                .map_err(|e| Error::local(path, e))
        }
    }
}

fn pubkey_from_backend_type(backend: Backend) -> &'static VerifyingKey {
    let pubkeys = orb_update_agent_core::pubkeys::get_pubkeys();
    match backend {
        Backend::Prod => &pubkeys.prod,
        Backend::Stage => &pubkeys.stage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orb_update_agent_core::{Slot, VersionMap};

    #[test]
    fn test_url_construction() {
        let base_url = Url::parse("https://fleet.stage.orb.worldcoin.org").unwrap();
        let id = "3d8af1da";
        let slot = Slot::A;

        let versions_json = r#"{
            "releases": {
                "slot_a": "test-version",
                "slot_b": "other-version"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let versions: orb_update_agent_core::versions::VersionsLegacy =
            serde_json::from_str(versions_json).unwrap();
        let version_map = VersionMap::from_legacy(&versions);

        let current_version = version_map.get_slot_version(slot).unwrap_or("unknown");
        let mut api_url = base_url.clone();
        api_url.set_path(&format!("/api/v2/orbs/{}/claim", id));
        api_url
            .query_pairs_mut()
            .clear()
            .append_pair("currentVersion", current_version);

        assert_eq!(
            api_url.to_string(),
            "https://fleet.stage.orb.worldcoin.org/api/v2/orbs/3d8af1da/claim?currentVersion=test-version"
        );
    }

    #[test]
    fn test_up_to_date_response_parsing() {
        let response = r#"{
            "status": "up_to_date",
            "current_version": "dupa",
            "message": "No update available"
        }"#;

        let parsed_response: serde_json::Value =
            serde_json::from_str(response).unwrap();

        if let Some(status) = parsed_response.get("status").and_then(|s| s.as_str()) {
            assert_eq!(status, "up_to_date");

            let version = parsed_response
                .get("current_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let message = parsed_response
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("No update available");

            assert_eq!(version, "dupa");
            assert_eq!(message, "No update available");
        }
    }

    #[test]
    fn test_incomplete_versions_json_behavior() {
        let versions_with_empty_json = r#"{
            "releases": {
                "slot_a": "",
                "slot_b": "valid-version"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let versions_with_empty: orb_update_agent_core::versions::VersionsLegacy =
            serde_json::from_str(versions_with_empty_json).unwrap();
        let version_map = VersionMap::from_legacy(&versions_with_empty);
        assert_eq!(version_map.get_slot_version(Slot::A), Some(""));
        assert_eq!(version_map.get_slot_version(Slot::B), Some("valid-version"));

        let versions_complete_json = r#"{
            "releases": {
                "slot_a": "version-a",
                "slot_b": "version-b"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let versions_complete: orb_update_agent_core::versions::VersionsLegacy =
            serde_json::from_str(versions_complete_json).unwrap();
        let version_map = VersionMap::from_legacy(&versions_complete);
        assert_eq!(version_map.get_slot_version(Slot::A), Some("version-a"));
        assert_eq!(version_map.get_slot_version(Slot::B), Some("version-b"));
    }

    #[test]
    fn test_malformed_versions_json_parsing() {
        let malformed_json = r#"{
            "releases": {
                "slot_b": "some-version"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let result: Result<orb_update_agent_core::versions::Versions, _> =
            serde_json::from_str(malformed_json);
        assert!(result.is_err());

        let malformed_json2 = r#"{
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let result2: Result<orb_update_agent_core::versions::Versions, _> =
            serde_json::from_str(malformed_json2);
        assert!(result2.is_err());

        let malformed_json3 = r#"{
            "releases": {
                "slot_a": null,
                "slot_b": "some-version"  
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        let result3: Result<orb_update_agent_core::versions::Versions, _> =
            serde_json::from_str(malformed_json3);
        assert!(result3.is_err());
    }
}
