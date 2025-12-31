# Setting up AWS Credentials

> [!NOTE] This section only applies to developers affiliated with the worldcoin
> organization.

Certain tools, including `orb-tools`, `orb-hil`, `orb-bidiff-cli`,
`cargo x optee ta sign` and the official aws cli require setting up AWS credentials to
use them.

While its always possible to export the credentials on the command line, its typically
easier to leverage AWS's official "profile" system. Profiles are controled with the
`AWS_PROFILE` environment variable, and configured under the `~/.aws` directory.

For the intended contents of `~/.aws/config`, see the [internal docs][internal docs].

You can now chose the appropriate aws profile for any CLI tool by passing the
`AWS_PROFILE=<profilename>` env var in any aws-related tasks.

The available profiles above are:
* `hil` for the [`orb-hil` cli][hil]
* `bidiff-{stage,prod}` to bidiff OTAs with the [`orb-bidiff-cli`][bidiff]
* `trustzone-{stage,prod}` to sign optee TAs with `cargo x optee ta sign`

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
[internal docs]: https://github.com/worldcoin/orb-internal/blob/main/aws_config.ini
