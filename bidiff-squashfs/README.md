# orb-bidiff-squashfs

The core libraries that support binary diffing. Leverages `bidiff` and
[`squashfs-tools-ng`][squashfs-tools-ng] internally. Patches can be applied with the
off-the-shelf `bipatch` crate from the `bidiff` repo or crates.io.

Note: the raw patches produced by `bidiff` or this library will be roughly the same
size as the original file, until compressed. Once compressed, the patch file size
will drop dramatically, as the compression is far more efficient due to the nature
of the patch file.

## LICENSE DISCLAIMER

We dynamically link against LGPL code due to the use of glib and `squashfs-tools-ng`.

[bidiff]: https://github.com/divvun/bidiff
[squashfs-tools-ng]: https://github.com/AgentD/squashfs-tools-ng
