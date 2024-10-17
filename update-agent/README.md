# Orb Update Agent

`orb-update-agent` fetches and installs orb updates. It is a Rust binary that is
run by systemd [on boot][service file]. It fetches a `claim.json` from the backend,
which contains a list of [components][components] that are archive files pulled from S3.

## Running

Update agent can be configured through a config file, environment variables, or through
command line arguments. Hereby cli arguments take precedence over env vars, which in turn
take precedence over a config file.

When deploying update agent through a systemd service file, it is recommended to default
to the config file due to the above mentioned precedence. If one of the settings has to be
changed on a production orb with a read-only file system (thus making it impossible to
edit the systemd service file), one can inject the required option into the global
systemd environment, for example setting the delay between download chunks to 0ms:

```sh
# As root
$ systemctl set-environment ORB_UPDATE_AGENT_DOWNLOAD_DELAY=0
```

### Config file

`update-agent` by default looks for a config file at `/etc/orb_update_agent.conf`. See
the unit tests for the config file format.

A different config file can be used by specifying the `--config <path-to-file>` command line
option.

Note that, while possible, it is discouraged to set options `id` and `active-slot` through the config
file. Instead, prefer to set them dynamically through the command line argument, `--active-slot $(get-slot)`,
or by injecting an environment variable `UPDATE_AGENT_ID=$(orb-id)` (for example, using systemd).

### Environment variables

Environment variables are specified as `ORB_UPDATE_AGENT_<option>=<value>` (capitalized here
because of unix convention, but actually case insensitive). The different options are the same as for the config file.

For example, to override the download path set in the config file, use:

```sh
ORB_UPDATE_AGENT_DOWNLOADS=/usr/persistent/downloads ./update-agent
```

### Testing

Tests which require special host environments or hardware in the loop are #[ignore]d
by default. Pass `-- --ignored` to cargo test to run them anyway.

The tests in `update-agent` are not cross platform, and won't work on macos. Set
`RUSTFLAGS='--cfg docker_runner'` to use docker to run the tests.

Example for macos users:
```bash
RUSTFLAGS='--cfg docker_runner' RUST_BACKTRACE=1 cargo-zigbuild test --target aarch64-unknown-linux-gnu --all
```

Example for linux users:
```bash
RUST_BACKTRACE=1 cargo test --all
```

#### MCU update

Follow the steps with this command: 

```shell
./tools/tests.sh --mcu-update
```

[service file]: ./debian/worldcoin-update-agent.service
[components]: ./components.json
