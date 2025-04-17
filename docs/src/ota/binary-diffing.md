# Binary Diffing CLI

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

See also [OTA Structure](./ota-structure.md) for more information on how binary diffs are represented in an
OTA.

## How to produce an OTA that uses binary diffs?

You can use either `orb-tools bidiff` or `orb-bidiff-cli` - both ways of accessing the
cli are equivalent. Please refer to the documentation of the tool's `--help` interface
for the most up-to-date docs.

**Be sure that your AWS credentials are configured** - you can follow the same instructions
from [orb-hil][aws setup].

This CLI will be able to retrive the full-size OTAs from several places:

* `ota://X.Y.Z+whatever` to download from s3 via the orb-os OTA version number
* `s3://foo/bar/` to download from s3 via a S3 URI
* or a local file path

The CLI will take several minutes to run, and then produces a new OTA directory
which contains all the components and a new, patched `claim.json`.

To see what the diffing process looks like, see
[this asciinema recording][diff asciinema]:
<script src="https://asciinema.org/a/KojBHU2hLFjoYSTgsqO1x85em.js" id="asciicast-KojBHU2hLFjoYSTgsqO1x85em" async="true"></script>

## How to get the orb to OTA with a binary diff?

Right now the backend doesn't support binary diffs yet, this is still WIP.
In the meantime, you can `scp -r` the contents of the directory that `orb-bidiff-cli`
produced onto your orb, typically onto the ssd at `/mnt/scratch/my-ota`.

Then invoke the update agent with:

```bash
cd /mnt/scratch/my-ota
sudo /usr/local/bin/orb-update-agent \
  --nodbus \
  --orb-id $ORB_ID \
  --update-location /mnt/scratch/my-ota/claim.json
```


[bidiff]: https://github.com/divvun/bidiff
[orb-bidiff-squashfs]: https://github.com/worldcoin/orb-software/tree/main/bidiff-squashfs
[diff asciinema]: https://asciinema.org/a/KojBHU2hLFjoYSTgsqO1x85em
[aws setup]: ../hil/cli.md#logging-in-to-aws
