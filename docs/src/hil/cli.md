# orb-hil cli

There is a CLI tool to facilitate hardware-in-loop operations. This tool lives
[in the orb-software repo][hil code] and releases can be downloaded
[here][hil releases]. 

It is a single, statically linked CLI tool with lots of features helpful for
development:

* Rebooting orbs into either normal or recovery mode
* Flashing orbs (including downloading from S3, extraction, etc)
* Executing commands over serial
* Automating the login process over serial

## Required peripherals

Different `orb-hil` subcommands require different hardware peripherals. We
strongly recommend at least getting an x86 linux machine and a serial adapter.
See the [hardware setup][hardware setup] page for more detailed info.

Here are the different hardware peripherals necessary for the different
subcommands of `orb-hil`:

* `orb-hil flash`: x86 linux machine
* `orb-hil reboot`: Serial adapter.
* `orb-hil login`: Serial adapter.
* `orb-hil cmd`: Serial adapter.

## Logging in to AWS

The `flash` subcommand can download S3 urls. To set this up, we recommend putting
the following into `~/.aws/config`:

```
[default]
sso_session = hil
sso_account_id = 510867353226
sso_role_name = ViewOnlyAccess
[profile hil]
sso_session = hil
sso_account_id = 510867353226
sso_role_name = ViewOnlyAccess
[sso-session hil]
sso_start_url = https://d-90676ede48.awsapps.com/start/#
sso_region = us-east-1
sso_registration_scopes = sso:account:access
```

You can now chose the appropriate aws profile for the hil by passing the
`AWS_PROFILE=hil` env var in any aws-related tasks. This works with both the
aws cli tool, and orb-hil.

To actually log in and get a fresh set of credentials:

```bash
AWS_PROFILE=hil aws sso login
```

[setup]: ./hardware-setup.md
[hil code]: https://github.com/worldcoin/orb-software/tree/main/hil
[hil releases]: https://github.com/worldcoin/orb-software/releases?q=hil&expanded=true
[hardware setup]: ./hardware-setup.md
