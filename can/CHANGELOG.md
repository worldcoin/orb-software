# CHANGELOG

## `0.2.2`

### Fixed

+ Use `std::os::fd::OwnedFd` for can sockets so that the file descriptors underlying the various CAN streams
  are correctly closed when the streams are dropped. Before, file descriptors were not closed on drop, leaking
  them.

## `0.2.1`

### Fixed

+ Reading the filters associated with a CAN socket would return early with an error if the
  provided buffer was too small. It now checks if the underlying io error was `ERANGE`
  matching the [linux socketcan implementation] and resizes the buffer holding the filters
  accordingly.

[linux socketcan implementation]: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/net/can/raw.c?h=v6.2#n732

## `0.2.0`

### Changed

+ `impl Read for &FrameStream` instead of `for FrameStream`. This brings it in line with
  other `Read` implementation in the standard library and is necessary to make it work with
  `tokio::io::unix::AsyncFd`.