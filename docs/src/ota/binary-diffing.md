# Binary Diffing

A full size orb-os image can often be 5-6.5 GiB in size. Binary diffing is used as a
compression mechanism, to reduce OTA sizes by only sending to orbs a "binary diff".

## What is a Binary Diff

A binary diff is analagous to a diff in `git`, except instead of operating at the
textual level, it operates at the bit/binary level.

The orb uses the [`bidiff`][bidiff] and `bipatch` crates to generate and apply binary
diffs, in addition to [our own][orb-bidiff-squashfs] modifications to better handle
squashfs files.

Further documentation on how `bidiff`, `bipatch`, and `orb-bidiff-squashfs` work can be
found in their respective crates.

[bidiff]: https://github.com/divvun/bidiff
[orb-bidiff-squashfs]: https://github.com/worldcoin/orb-software/tree/main/bidiff-squashfs
