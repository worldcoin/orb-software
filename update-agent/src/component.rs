//! Runtime specific representation of manifest components.
//!
//! Whereas [ManifestComponent] represents a component listed in the manifest, the [Component] type
//! defined here also includes its source and location on disk.
use std::{
    fs::{metadata, remove_file, File, OpenOptions},
    io::{self, copy},
    num::ParseIntError,
    path::{Path, PathBuf},
    time::Duration,
};

use eyre::{ensure, WrapErr as _};
use orb_update_agent_core::{
    components, manifest::InstallationPhase, Claim, LocalOrRemote, ManifestComponent,
    MimeType, Slot, Source,
};
use reqwest::{
    header::{ToStrError, CONTENT_LENGTH, RANGE},
    Url,
};
use tracing::{info, warn};

use crate::{dbus, update::Update as _, util};

const CHUNK_SIZE: u32 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed initializing client to check for remote updates")]
    InitClient(#[from] crate::client::Error),
    #[error("Component `{0}` had a size of `{1}` in claim, but remote reported a size of `{2}`")]
    ClaimSizeRemoteLenMismatch(String, u64, u64),
    #[error("response did not include content length header: {0}")]
    MissingContentLengthHeader(Url),
    #[error("response contained non-string content length value: {0}")]
    NonStringContentLengthValue(Url, #[source] ToStrError),
    #[error("response content length `{0}` could not be parsed as integer: {1}")]
    InvalidContentLengthValue(String, Url, #[source] ParseIntError),
    #[error("could not open write target: {}", .0.display())]
    OpenWriteTarget(PathBuf, #[source] io::Error),
    #[error("failed constructing an iterator over the http byte ranges")]
    InvalidHttpRange(#[from] util::HttpRangeError),
    #[error("failed requesting range `{0}` {0:#}: {1}")]
    RangeRequest(util::Range, Url, #[source] reqwest::Error),
    #[error("failed sending the initial request to estimate component length: `{0}`")]
    InitialLengthRequest(Url, #[source] reqwest::Error),
    #[error(
        "request for range `{0}` {0:#} returned status code `{1}`, expected range 200-299: {2}"
    )]
    ResponseStatus(util::Range, reqwest::StatusCode, Url),
    #[error("failed retrieving the response body for range `{0}` {0:#} as bytes: {1}")]
    GetBytes(util::Range, Url, #[source] reqwest::Error),
    #[error("failed copying retrieved chunk `{0}` {0:#} to target `{target}`: {2}", target = .1.display())]
    MergeChunk(util::Range, PathBuf, Url, #[source] io::Error),
    #[error("failed verifying source component `{name}` against claim")]
    HashMismatch { name: String, source: eyre::Report },
    #[error(
        "MIME type of component `{name}` was set to `{actual_type}`; only `application/x-xz` MIME \
         types are supported"
    )]
    MimeUnknown { name: String, actual_type: String },
}

pub struct Component {
    manifest_component: ManifestComponent,
    source: Source,
    system_component: components::Component,
    on_disk: PathBuf,
}

impl Component {
    pub fn manifest_component(&self) -> &ManifestComponent {
        &self.manifest_component
    }

    pub fn system_component(&self) -> &components::Component {
        &self.system_component
    }

    pub fn name(&self) -> &str {
        self.manifest_component.name()
    }

    fn process_compressed(&mut self) -> eyre::Result<()> {
        let uncompressed_path = self.on_disk.with_extension("uncompressed");
        let uncompressed_path_verified =
            get_verified_component_path(&uncompressed_path);

        match check_existing_component(&uncompressed_path, self.manifest_component.size)
        {
            Ok(()) => {
                info!(
                    "found verification file at `{}`, skipping hash verification of decompressed \
                     `{}`",
                    uncompressed_path_verified.display(),
                    self.manifest_component.name,
                );

                self.on_disk = uncompressed_path;
                return Ok(());
            }
            Err(e) => {
                info!(
                    "verifying existing component at `{}` failed, reprocessing: {e:?}",
                    self.manifest_component.name,
                );
            }
        }

        info!("extracting {}", self.manifest_component.name());
        extract(&self.on_disk, &uncompressed_path).wrap_err_with(|| {
            format!(
                "failed decompressing component at `{}`",
                self.on_disk.display()
            )
        })?;
        info!(
            "checking sha256 hash of extracted {}",
            self.manifest_component.name()
        );
        if let Err(e) =
            util::check_hash(&uncompressed_path, self.manifest_component.hash())
                .wrap_err_with(|| {
                    format!(
                        "failed verifying hask of extracted component file at `{}`",
                        uncompressed_path.display(),
                    )
                })
        {
            if self.source.is_remote() {
                info!("source was remote, deleting extracted component");
                if let Err(e) = remove_file(&uncompressed_path) {
                    warn!(
                        "failed removing extracted component `{}` with error: {e:?}",
                        uncompressed_path.display(),
                    );
                }
            }
            return Err(e);
        }
        self.on_disk = uncompressed_path;

        if let Err(e) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(uncompressed_path_verified)
        {
            warn!(
                "failed marking component `{}` as verified: {e:?}",
                self.manifest_component.name
            )
        }

        Ok(())
    }

    pub fn process(&mut self) -> eyre::Result<()> {
        match self.source.mime_type {
            MimeType::XZ => self.process_compressed(),
            MimeType::OctetStream => Ok(()),
        }
    }

    fn do_install(&self, slot: Slot, claim: &Claim) -> eyre::Result<()> {
        let mut component_file = File::options()
            .read(true)
            .create(false)
            .open(&self.on_disk)
            .wrap_err_with(|| {
                format!(
                    "failed to open path to component `{}`",
                    self.on_disk.display(),
                )
            })?;
        // FIXME: Panic has some surprising behaviour pre-2021; update rust edition to 2021
        //        and fix the format string.
        claim
            .system_components()
            .get(self.name())
            .unwrap_or_else(|| {
                panic!(
                    "claim system components should contain all components listed in manifest; \
                     missing component: `{name}`",
                    name = self.name(),
                )
            })
            .update(slot, &mut component_file)
            .wrap_err("failed to execute update step of component")?;
        Ok(())
    }

    pub fn run_update(
        &self,
        slot: Slot,
        claim: &Claim,
        recovery: bool,
    ) -> eyre::Result<()> {
        let name = self.name();
        match (self.manifest_component.installation_phase(), recovery) {
            (InstallationPhase::Normal, true) => info!(
                "skipping installation of component `{name}`, because installation phase is \
                 normal but recovery is set"
            ),
            (InstallationPhase::Normal, false) => {
                info!(
                    "installing component `{name}` because installation phase is normal and \
                     recovery is unset"
                );
                self.do_install(slot, claim)
                    .wrap_err("failed copying update")?;
            }
            (InstallationPhase::Recovery, true) => {
                info!(
                    "installing component `{name}` because installation phase is recovery and \
                     recovery is set"
                );
                self.do_install(slot, claim)
                    .wrap_err("failed copying update")?;
            }
            (InstallationPhase::Recovery, false) => {
                info!(
                    "skipping installation of component `{name}` because installation phase is \
                     recovery and recovery is unset"
                );
            }
        }
        Ok(())
    }
}

fn check_existing_component(
    component_path: &Path,
    expected_size: u64,
) -> eyre::Result<()> {
    let verified_component_path = get_verified_component_path(component_path);
    ensure!(
        verified_component_path.exists(),
        "component at {} does not exists",
        verified_component_path.display()
    );
    let component_size = metadata(component_path)
        .wrap_err(format!(
            "failed reading file metadata for `{}`",
            component_path.display()
        ))?
        .len();
    ensure!(
        component_size == expected_size,
        "component size ({component_size}) of `{}` does not match expected size ({expected_size})",
        component_path.display()
    );
    Ok(())
}

fn get_verified_component_path(component_path: &Path) -> PathBuf {
    component_path.with_extension("verified")
}

fn extract<P: AsRef<Path>>(path: P, uncompressed_download_path: P) -> eyre::Result<()> {
    let compressed_download = File::options()
        .read(true)
        .write(false)
        .create(false)
        .open(&path)
        .wrap_err_with(|| {
            format!(
                "failed to open component for decompression at `{}`",
                path.as_ref().display()
            )
        })?;

    let mut decoder = xz2::read::XzDecoder::new(compressed_download);
    let mut uncompressed_download = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&uncompressed_download_path)
        .wrap_err_with(|| {
            format!(
                "failed to open target to store decompressed component at `{}`",
                path.as_ref().display()
            )
        })?;
    copy(&mut decoder, &mut uncompressed_download).wrap_err_with(|| {
        format!("failed to decompress file at `{}`", path.as_ref().display())
    })?;
    Ok(())
}

#[expect(clippy::result_large_err)]
pub fn download<P: AsRef<Path>>(
    url: &Url,
    name: &str,
    unique_name: &str,
    size: u64,
    dst_dir: P,
    supervisor_proxy: Option<&dbus::SupervisorProxyBlocking<'static>>,
    download_delay: Duration,
) -> Result<PathBuf, Error> {
    let component_path = util::make_component_path(dst_dir, unique_name);
    let component_file_len = match metadata(&component_path)
        .map(|metadata| metadata.len())
    {
        Ok(len) if len == size => {
            info!(
                "component with matching size form claim found on disk, skipping download of \
                 `{name}`"
            );
            return Ok(component_path);
        }
        Ok(len) => Some(len),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(
                "failed to query metadata of `{}`: {e:?}",
                component_path.display()
            );
            None
        }
    };

    let client = crate::client::normal()?;

    // We issue a GET request and ignore the response body instead of a HEAD request because AWS S3
    // pre-signed URLs include the HTTP action in the URL signature. Otherwise the server would
    // need to provide multiple URLs, one for the GET and one for the HEAD requests.
    let response = client
        .get(url.clone())
        .send()
        .map_err(|e| Error::InitialLengthRequest(url.clone(), e))?;

    let component_remote_len = response
        .headers()
        .get(CONTENT_LENGTH)
        .ok_or_else(|| Error::MissingContentLengthHeader(url.clone()))
        .and_then(|header_val| {
            let header_val = header_val
                .to_str()
                .map_err(|e| Error::NonStringContentLengthValue(url.clone(), e))?;
            header_val.parse::<u64>().map_err(|e| {
                Error::InvalidContentLengthValue(header_val.to_string(), url.clone(), e)
            })
        })?;
    if size != component_remote_len {
        return Err(Error::ClaimSizeRemoteLenMismatch(
            name.into(),
            size,
            component_remote_len,
        ));
    }

    let mut open_options = File::options();
    open_options.write(true).create(true);

    let start_bytes = match component_file_len {
        Some(len) if len == size => {
            info!(
                "file on disk for component `{}` matches size recorded in claim; skipping download",
                name,
            );
            return Ok(component_path);
        }
        Some(len) if len > size => {
            warn!(
                "length of file on disk exceeds Content-Length header; removing file and \
                 restarting download"
            );
            if let Err(e) = remove_file(&component_path) {
                warn!(
                    "failed removing components blob at `{}`: {e:?}",
                    component_path.display()
                );
            }
            open_options.truncate(true);
            0
        }
        Some(len) if len % (CHUNK_SIZE as u64) != 0 => {
            warn!(
                "length of file on disk is not a multiple of hard coded chunk size; removing file \
                 and restarting download"
            );
            if let Err(e) = remove_file(&component_path) {
                warn!(
                    "failed removing components file at `{}`: {e:?}",
                    component_path.display()
                );
            }
            open_options.truncate(true);
            0
        }
        Some(len) => {
            open_options.append(true);
            len
        }
        None => {
            open_options.truncate(true);
            0
        }
    };

    if start_bytes == 0 {
        info!("starting download to: {}", component_path.display());
    } else {
        info!("resuming download to: {}", component_path.display());
    }

    let dst = open_options
        .open(&component_path)
        .map_err(|e| Error::OpenWriteTarget(component_path.clone(), e))?;

    let mut current_delay = download_delay;
    let mut allowed_before = true;

    let remaining_chunks =
        ((component_remote_len - start_bytes) / CHUNK_SIZE as u64) as usize;

    let mut progress_percent = 0;
    for (i, range) in
        util::HttpRangeIter::try_new(start_bytes, component_remote_len - 1, CHUNK_SIZE)?
            .enumerate()
    {
        if let Some(current_progress_percent) = (i * 100).checked_div(remaining_chunks)
        {
            if progress_percent != current_progress_percent {
                progress_percent = current_progress_percent;
                info!("downloading component `{name}`: {progress_percent}%");
            }
        } else {
            info!("downloading component `{name}`: 100%");
        }

        // We are using `downloads_allowed` as a proxy to set/unset the sleep duration for now.
        // Downloads are no longer blocked entirely. Dbus error when communicating are reported
        // but leave the currently set duration unchanged.
        if let Some(proxy) = supervisor_proxy {
            match proxy.background_downloads_allowed() {
                Ok(allowed_now) => {
                    match (allowed_now, allowed_before) {
                        (true, false) => info!("orb no longer in use; stop throttling downloads"),
                        (false, true) => info!("orb in use again; throttling downloads"),
                        _ => {}
                    }
                    if allowed_now {
                        if current_delay != Duration::ZERO {
                            info!("stop throttling downloads");
                        }
                        current_delay = Duration::ZERO;
                    } else if current_delay != download_delay {
                        current_delay = download_delay;
                        info!("throttling downloads: new delay {}ms", download_delay.as_millis());
                    }
                    allowed_before = allowed_now;
                }
                Err(e) => warn!(
                    "checking supervisor for download restrictions failed; leaving download delay \
                     unchanged: {e:?}"
                ),
            }
        }

        let response = client
            .get(url.clone())
            .header(RANGE, range.to_string())
            .send()
            .map_err(|e| Error::RangeRequest(range, url.clone(), e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::ResponseStatus(range, status, url.clone()));
        }

        let response = response
            .bytes()
            .map_err(|e| Error::GetBytes(range, url.clone(), e))?;
        copy(&mut response.as_ref(), &mut &dst).map_err(|e| {
            Error::MergeChunk(range, component_path.clone(), url.clone(), e)
        })?;

        std::thread::sleep(current_delay);
    }
    Ok(component_path)
}

// Fetches a component by finding it on disk or downloading it from remote.
#[expect(clippy::result_large_err)]
pub fn fetch<P: AsRef<Path>>(
    manifest_component: &ManifestComponent,
    system_component: &components::Component,
    source: &Source,
    dst_dir: P,
    supervisor: Option<&dbus::SupervisorProxyBlocking<'static>>,
    download_delay: Duration,
) -> Result<Component, Error> {
    let path = match &source.url {
        LocalOrRemote::Local(path) => path.clone(),
        LocalOrRemote::Remote(url) => download(
            url,
            &source.name,
            &source.unique_name(),
            source.size,
            dst_dir,
            supervisor,
            download_delay,
        )?,
    };
    info!(
        "checking sha256 hash of downloaded `{}`",
        manifest_component.name()
    );
    let path_verified = get_verified_component_path(&path);

    if path_verified.exists() {
        info!(
            "found verification file at `{}`, skipping hash verification of `{}`",
            path_verified.display(),
            source.name,
        );
    } else {
        if let Err(e) =
            util::check_hash(&path, &source.hash).map_err(|e| Error::HashMismatch {
                name: source.name.clone(),
                source: e,
            })
        {
            if source.url.is_remote() {
                warn!(
                    "deleting downloaded source blob of component `{}` because hash verification \
                     failed; see logs for more info",
                    source.name
                );
                if let Err(rm_err) = remove_file(&path) {
                    warn!(
                        "failed deleting source blob of component `{}` at `{}`: {rm_err:?}",
                        source.name,
                        path.display(),
                    );
                }
            }
            return Err(e);
        }
        if let Err(e) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path_verified)
        {
            warn!(
                "failed marking component `{}` as verified: {e:?}",
                source.name
            )
        }
    }

    Ok(Component {
        manifest_component: manifest_component.clone(),
        system_component: system_component.clone(),
        source: source.clone(),
        on_disk: path,
    })
}
