# Setting up AWS Credentials

> [!NOTE] This section only applies to developers affiliated with the worldcoin
> organization.

Certain tools, including `orb-tools`, `orb-hil`, `orb-bidiff-cli`, and the official aws cli
require setting up AWS credentials to use them.

While its always possible to export the credentials on the command line, its typically
easier to leverage AWS's official "profile" system. Profiles are controled with the
`AWS_PROFILE` environment variable, and configured under the `~/.aws` directory.

We recommend putting the following into `~/.aws/config`:

```
[default]
sso_session = my-sso
sso_account_id = 510867353226
sso_role_name = ViewOnlyAccess

[profile hil]
sso_session = my-sso
sso_account_id = 510867353226
sso_role_name = ViewOnlyAccess

[profile bidiff-stage]
sso_session = my-sso
sso_account_id = 510867353226
sso_role_name = ViewOnlyAccess

[profile bidiff-prod]
sso_session = my-sso
sso_account_id = 573252405782
sso_role_name = ViewOnlyAccess

[sso-session my-sso]
sso_start_url = https://d-90676ede48.awsapps.com/start/#
sso_region = us-east-1
sso_registration_scopes = sso:account:access
```

You can now chose the appropriate aws profile for any CLI tool by passing the
`AWS_PROFILE=<profilename>` env var in any aws-related tasks.

The available profiles above are:
* `hil` for the [`orb-hil` cli][hil]
* `bidiff-stage` to bidiff stage OTAs with the [`orb-bidiff-cli`][bidiff]
* `bidiff-prod` to bidiff prod OTAs with the [`orb-bidiff-cli`][bidiff]

## Examples
For example, to use the HIL CLI:

```bash
AWS_PROFILE=hil aws sso login --use-device-code
AWS_PROFILE=hil cargo run -p orb-hil
```

To diff prod OTAs:

```bash
AWS_PROFILE=bidiff-prod aws sso login --use-device-code
AWS_PROFILE=bidiff-prod cargo run -p orb-bidiff-cli
```


[hil]: ./hil/cli.md
[bidiff]: ./ota/binary-diffing.md
