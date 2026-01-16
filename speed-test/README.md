# orb-speed-test

Orb Speed Test is a tool to check Internet download/upload speeds.

## Usage

```sh
Network speed test utility for Orb

Usage: orb-speed-test [OPTIONS]

Options:
      --format <FORMAT>            Output format [default: human] [possible values: json, human]
      --size <SIZE>                Test size in megabytes (MB) of uncompressed data
      --pcp                        Run PCP upload speed test instead of Cloudflare test
      --dbus-addr <DBUS_ADDR>      D-Bus socket address for PCP authentication (only used with --pcp) [default: unix:path=/tmp/worldcoin_bus_socket]
      --num-uploads <NUM_UPLOADS>  Number of uploads to perform for averaging (only used with --pcp) [default: 3]
  -h, --help                       Print help
  -V, --version                    Print version
```
