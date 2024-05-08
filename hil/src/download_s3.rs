use camino::Utf8Path;
use cmd_lib::run_cmd;
use color_eyre::{
    eyre::{ensure, ContextCompat, WrapErr},
    Result, Section,
};

pub async fn download_url(url: &str, out_path: &Utf8Path) -> Result<()> {
    let parent_dir = out_path
        .parent()
        .expect("please provide the path to a file");
    ensure!(
        parent_dir.try_exists().unwrap_or(false),
        "parent directory {parent_dir} doesn't exist"
    );
    let out_path_copy = out_path.to_owned();
    let url_copy = url.to_owned();
    tokio::task::spawn_blocking(move || {
        download_using_awscli(&url_copy, &out_path_copy)
    })
    .await
    .wrap_err("task panicked")?
}

fn download_using_awscli(url: &str, out_path: &Utf8Path) -> Result<()> {
    let result = run_cmd! {
        info downloading $url to $out_path;
        aws s3 cp $url $out_path;
        info finished download!;
    };
    result
        .wrap_err("failed to call aws cli")
        .with_note(|| format!("url was {url}"))
        .with_note(|| format!("out_path was {out_path}"))
        .with_suggestion(|| "Are the AWS url and your credentials valid?")
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
#[cfg(test)]
mod test {
    use super::*;

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
