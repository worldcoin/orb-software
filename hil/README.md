# orb-hil

## AWS credentials

You can set the `AWS_PROFILE` env var to customize which aws profile is used by the tool.
It is recommended to set up a dedicated AWS profile with the appropriate perms to download
ors-os artifacts from S3 and pass that as an env var. See [here][aws cli config] for more
info.

[aws cli config]: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html

## Debug board configuration

With debug board v1.1 comes an embedded EEPROM that can be programmed to set a configuration
to the FTDI chip, such as the serial number.
This serial number enables selection among a few different ones connected to the host.

Use the `ftdi` command to read & write the config:

```sh
# write
orb-hil ftdi write ftdi_config.json

# read
orb-hil ftdi read [--file ftdi_config.json]
```

Here is an example of an FTDI configuration (set a working serial):

```json
{
  "vendor_id": 1027,
  "product_id": 24593,
  "serial_number_enable": true,
  "max_current_ma": 500,
  "self_powered": false,
  "remote_wakeup": false,
  "pull_down_enable": false,
  "manufacturer": "FTDI",
  "manufacturer_id": "FT",
  "description": "FT4232H",
  "serial_number": "YOUR_SERIAL_HERE"
}
```
