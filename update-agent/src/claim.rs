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
) -> Result<(String, Claim), Error> {
    let req_body = serde_json::json!({
        "id": id,
        "active_slot": slot.to_string(),
        "versions": version_map.to_legacy(),
    });

    debug!(
        "sending check request with body: {}",
        serde_json::to_string(&req_body)
            .expect("the json Value object contains only valid json"),
    );

    let client = crate::client::normal()?;
    let resp = client
        .post(url.clone())
        .json(&req_body)
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
        Err(Error::status_code(status, msg))
    } else {
        let resp_txt = resp.text().map_err(Error::ResponseAsText)?;
        debug!("server sent raw claim: {resp_txt}");
        let claim_verification_context = ClaimVerificationContext(
            pubkey_from_backend_type(verify_manifest_signature_against),
        );
        let claim = crate::json::deserialize_seed(
            claim_verification_context,
            resp_txt.as_bytes(),
        )
        .map_err(Error::ReadJson)?;
        Ok((resp_txt, claim))
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
            let (raw_txt, claim) = from_remote(
                &settings.id,
                settings.active_slot,
                url,
                version_map,
                settings.verify_manifest_signature_against,
            )
            .map_err(|e| Error::remote(url, e))?;
            info!("writing raw remote update claim to disk");
            if let Err(e) = to_disk(&path, &raw_txt) {
                warn!("failed writing remote claim to `{}`: {e:?}", path.display());
            }
            Ok(claim)
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
