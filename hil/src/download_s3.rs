use std::{
    io::IsTerminal,
    time::{Duration, Instant},
};

use camino::Utf8Path;
use color_eyre::{
    eyre::{bail, ensure, ContextCompat, WrapErr},
    Result,
};
use indicatif::ProgressBar;
use orb_s3_helpers::{ClientExt as _, ExistingFileBehavior, Progress, S3Uri};
use tracing::info;

const NON_INTERACTIVE_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(60);

pub async fn download_url(
    url: &S3Uri,
    out_path: &Utf8Path,
    existing_file_behavior: ExistingFileBehavior,
) -> Result<()> {
    let s3_parts = url;
    let start_time = std::time::Instant::now();

    let client = orb_s3_helpers::client().await.unwrap();
    let is_interactive = std::io::stdout().is_terminal();
    let mut pb = None;
    let mut non_interactive_progress = NonInteractiveProgress::default();
    client
        .download_multipart(s3_parts, out_path, existing_file_behavior, |p| {
            if pb.is_none() {
                pb.insert(ProgressBar::new(p.total_to_download)).set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .with_key("eta", |state: &indicatif::ProgressState, w: &mut dyn std::fmt::Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-"),
    );

        }
            on_progress(
                is_interactive,
                pb.as_mut().unwrap(),
                &p,
                &mut non_interactive_progress,
            )
        })
        .await.wrap_err("failed to perform multipart download")?;
    pb.inspect(|pb| pb.finish_and_clear());

    let file_size = tokio::fs::File::open(out_path)
        .await?
        .metadata()
        .await?
        .len();
    info!(
        "Downloaded {}MiB, took {}",
        file_size >> 20,
        elapsed_time_as_str(start_time.elapsed())
    );

    Ok(())
}

/// Calculates the filename based on the s3 url.
pub fn parse_filename(url: &S3Uri) -> Result<String> {
    let url = url.to_string();
    let expected_prefix = "s3://worldcoin-orb-resources/worldcoin/orb-os/";
    let path = url
        .strip_prefix(expected_prefix)
        .wrap_err_with(|| format!("missing url prefix of {expected_prefix}"))?;
    let splits: Vec<_> = path.split('/').collect();
    ensure!(
        splits.len() == 3,
        "invalid number of '/' delineated segments in the url"
    );
    ensure!(
        splits[2].contains(".tar."),
        "it doesn't look like this url ends in a tarball"
    );
    let idx = if splits[0] == "rts" {
        1
    } else if splits[1] == "rts" {
        0
    } else {
        bail!("expected one of the segments in the path to be `rts`")
    };
    Ok(format!("{}-{}", splits[idx], splits[2]))
}

fn elapsed_time_as_str(time: Duration) -> String {
    let total_secs = time.as_secs();
    let minutes = total_secs / 60;
    let remaining_secs = total_secs % 60;
    format!("{minutes}m{remaining_secs}s")
}

#[derive(Debug, Default)]
struct NonInteractiveProgress {
    last_log_at: Option<Instant>,
}

fn should_log_progress_update(
    last_log_at: Option<Instant>,
    now: Instant,
    is_complete: bool,
) -> bool {
    if is_complete {
        return true;
    }

    match last_log_at {
        None => true,
        Some(last_log_at) => {
            now.duration_since(last_log_at) >= NON_INTERACTIVE_PROGRESS_LOG_INTERVAL
        }
    }
}

fn on_progress(
    is_interactive: bool,
    pb: &mut ProgressBar,
    progress: &Progress,
    non_interactive_progress: &mut NonInteractiveProgress,
) {
    if is_interactive {
        pb.set_position(progress.bytes_so_far);
    } else {
        let total_to_download = progress.total_to_download.max(1);
        let pct = (progress.bytes_so_far * 100) / total_to_download;
        let now = Instant::now();
        let is_complete = progress.bytes_so_far >= progress.total_to_download;

        if should_log_progress_update(
            non_interactive_progress.last_log_at,
            now,
            is_complete,
        ) {
            info!(
                "Downloaded: ({}/{} MiB) {}%",
                progress.bytes_so_far >> 20,
                progress.total_to_download >> 20,
                pct,
            );
            non_interactive_progress.last_log_at = Some(now);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_elapsed_time_as_str() {
        assert_eq!("0m0s", elapsed_time_as_str(Duration::ZERO));
        assert_eq!("0m0s", elapsed_time_as_str(Duration::from_millis(999)));
        assert_eq!("0m1s", elapsed_time_as_str(Duration::from_millis(1000)));
        assert_eq!("0m1s", elapsed_time_as_str(Duration::from_millis(1001)));

        assert_eq!("0m59s", elapsed_time_as_str(Duration::from_secs(59)));
        assert_eq!("1m0s", elapsed_time_as_str(Duration::from_secs(60)));
        assert_eq!("1m1s", elapsed_time_as_str(Duration::from_secs(61)));

        assert_eq!(
            "61m59s",
            elapsed_time_as_str(Duration::from_secs(61 * 60 + 59))
        );
    }

    #[test]
    fn test_parse() -> color_eyre::Result<()> {
        let examples = [
            // OLD
            (
                "s3://worldcoin-orb-resources/worldcoin/orb-os/2024-05-07-heads-main-0-g4b8aae5/rts/rts-dev.tar.zst",
                "2024-05-07-heads-main-0-g4b8aae5-rts-dev.tar.zst"
            ),
            (
                "s3://worldcoin-orb-resources/worldcoin/orb-os/2024-05-08-remotes-pull-386-merge-0-geea20f1/rts/rts-prod.tar.zst",
                "2024-05-08-remotes-pull-386-merge-0-geea20f1-rts-prod.tar.zst"
            ),
            (
                "s3://worldcoin-orb-resources/worldcoin/orb-os/2024-05-08-tags-release-5.0.39-0-ga12b3d7/rts/rts-dev.tar.zst",
                "2024-05-08-tags-release-5.0.39-0-ga12b3d7-rts-dev.tar.zst"
            ),

            // NEW
            (
                "s3://worldcoin-orb-resources/worldcoin/orb-os/rts/2025-08-14-heads-main-0-g0a8d01b-diamond/rts-diamond-dev.tar.zstd",
                "2025-08-14-heads-main-0-g0a8d01b-diamond-rts-diamond-dev.tar.zstd",
            ),
            (
                "s3://worldcoin-orb-resources/worldcoin/orb-os/rts/2025-08-14-heads-main-0-g0a8d01b-pearl/rts-pearl-dev.tar.gz",
                "2025-08-14-heads-main-0-g0a8d01b-pearl-rts-pearl-dev.tar.gz",
            ),
        ];
        for (url, expected_filename) in examples {
            let url: S3Uri = url.parse().unwrap();
            assert_eq!(parse_filename(&url)?, expected_filename);
        }
        Ok(())
    }

    #[test]
    fn test_should_log_progress_update() {
        let now = Instant::now();
        assert!(should_log_progress_update(None, now, false));
        assert!(!should_log_progress_update(
            Some(now),
            now + Duration::from_secs(59),
            false
        ));
        assert!(should_log_progress_update(
            Some(now),
            now + Duration::from_secs(60),
            false
        ));
        assert!(should_log_progress_update(
            Some(now),
            now + Duration::from_secs(1),
            true
        ));
    }
}
