# orb-hil

## AWS credentials

You can set the `AWS_PROFILE` env var to customize which aws profile is used by the tool.
It is recommended to set up a dedicated AWS profile with the appropriate perms to download
ors-os artifacts from S3 and pass that as an env var. See [here][aws cli config] for more
info.

[aws cli config]: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html
