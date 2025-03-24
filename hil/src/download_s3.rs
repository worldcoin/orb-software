use std::{io::IsTerminal, time::Duration};

use camino::Utf8Path;
use color_eyre::{
    eyre::{ensure, ContextCompat, WrapErr},
    Result,
};
use indicatif::ProgressBar;
use orb_s3_helpers::{ClientExt as _, ExistingFileBehavior, Progress, S3Uri};
use tracing::info;

pub async fn download_url(
    url: &str,
    out_path: &Utf8Path,
    existing_file_behavior: ExistingFileBehavior,
) -> Result<()> {
    let s3_parts: S3Uri = url.parse().wrap_err("invalid s3 url")?;
    let start_time = std::time::Instant::now();

    let client = orb_s3_helpers::client().await.unwrap();
    let is_interactive = std::io::stdout().is_terminal();
    let mut pb = None;
    client
        .download_multipart(&s3_parts, out_path, existing_file_behavior, |p| {
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
            on_progress(is_interactive, pb.as_mut().unwrap(), &p)
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
pub fn parse_filename(url: &str) -> Result<String> {
    let expected_prefix = "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/";
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
    Ok(format!("{}-{}", splits[0], splits[2]))
}

fn elapsed_time_as_str(time: Duration) -> String {
    let total_secs = time.as_secs();
    let minutes = total_secs / 60;
    let remaining_secs = total_secs % 60;
    format!("{minutes}m{remaining_secs}s")
}

fn on_progress(is_interactive: bool, pb: &mut ProgressBar, progress: &Progress) {
    if is_interactive {
        pb.set_position(progress.bytes_so_far);
    } else {
        let pct = (progress.bytes_so_far * 100) / progress.total_to_download;
        if pct % 5 == 0 {
            info!(
                "Downloaded: ({}/{} MiB) {}%",
                progress.bytes_so_far >> 20,
                progress.total_to_download >> 20,
                pct,
            );
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
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-07-heads-main-0-g4b8aae5/rts/rts-dev.tar.zst",
                "2024-05-07-heads-main-0-g4b8aae5-rts-dev.tar.zst"
            ),
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-08-remotes-pull-386-merge-0-geea20f1/rts/rts-prod.tar.zst",
                "2024-05-08-remotes-pull-386-merge-0-geea20f1-rts-prod.tar.zst"
            ),
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-08-tags-release-5.0.39-0-ga12b3d7/rts/rts-dev.tar.zst",
                "2024-05-08-tags-release-5.0.39-0-ga12b3d7-rts-dev.tar.zst"
            ),
        ];
        for (url, expected_filename) in examples {
            assert_eq!(parse_filename(url)?, expected_filename);
        }
        Ok(())
    }
}
