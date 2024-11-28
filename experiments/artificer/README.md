# Artificer

A CLI tool to download and extract artifacts from many sources, in a reproducible
manner.

See also the [design doc](DESIGN.md).

## Project Status

This was hacked together in a day during the hackathon.

What is done:
- Parsing for artificer.toml and artificer.lock and has accompanying round-trip tests.
- Github artifact fetching functionality.
- Stacked download progress bars.

What isn't done:
- Extractors.
- Out-dir doesn't get generated.
- Lockfile doesn't get generated from toml.
- Download caching or hashing.
