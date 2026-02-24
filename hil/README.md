# orb-hil

## AWS credentials

You can set the `AWS_PROFILE` env var to customize which aws profile is used by the tool.
It is recommended to set up a dedicated AWS profile with the appropriate perms to download
ors-os artifacts from S3 and pass that as an env var. See [here][aws cli config] for more
info.

[aws cli config]: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html

## Examples

### Login via serial (before all other commands)
``` shell
orb-hil login --password ${{ secrets.ORB_DEV_PASSWORD }} --serial-path ${CI_SERIAL_TTY_PATH} --timeout ${CI_BOOT_TIMEOUT}
```

### Run a command via serial
``` shell
orb-hil cmd --serial-path ${CI_SERIAL_TTY_PATH} --timeout ${CI_BOOT_TIMEOUT} "ip a"
```

### Run a command via SSH (password auth)
``` shell
orb-hil cmd --transport ssh --orb-id bba85baa --password "${ORB_DEV_PASSWORD}" "ip a"
```

### Run a command via Teleport (`tsh`)
``` shell
orb-hil cmd --transport teleport --orb-id bba85baa --username root "pwd"
```
