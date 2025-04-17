# OTA structure

OTAs are comprised of a [`claim.json`][claim], and a list of binary files which we refer to as
"components".

## Claim structure

The `claim.json` can also be divided into three main parts, the manifest and the sources.
* The `manifest` field, which contains metadata about how a component should be
  installed, as well as integrity information such as the hash of the component, its
  size, etc.
* The `sources` field, which contains metadata about the compressed version of components,
  along with where they can be downloaded. These also have hashes and other integrity
  information.
* The `signature` field, which enables the update agent to verify that the manifest has
  not been tampered with. This signature protects only the manifest, it does not
  protect the rest of the claim or the sources. Note that most of the orbs secure boot
  guarantees actually come from other places like dm-verity and our secure boot
  architecture, not from this manifest signature. In other words, the manifest signature
  is a nice "bonus" security measure.

When stored on s3, typically all of the compressed components (the sources), as well as
the claim.json, are stored together in a "directory".

## What is a component?

Once the source for a component is downloaded and potentially decompressed, it will be
installed differently depending on the component type. Typically these components are
things like partitions that should be `dd`ed to disk, firmware blobs to be sent over
CAN, etc.

For the most comprehensive documentation, see the [`Component`][Component] enum.

## How do Partial OTAs work?

A partial OTA is a method of reducing the size of an OTA by sending only a subset of
the full set of components in an OTA. Partial OTAs rely on shaky promises of
reproducibility in orb-os, where we *hope* that certain partitions that the build
produced didn't change, and therefore we can get away with not including the unchanged
partitions as components in the OTA.

With the advent of binary diffing, partial OTAs are essentially unecessary. They are
also even less useful on diamond orbs, whose root file system is mostly bundled into a
single, really large component instead of multiple overlayfs partitions.

## How do Binary Diffs work?

A component that is binary diffed is no different from a regular component, it just has
a different MIME type - `application/zstd-bidiff`. Like GPT components, these are
essentially just partition contents + the label of the partition. When "extracting" a
bidiff component we:
* inspect the component's label, to find a matching partition on the current booted slot
  which will become the base against which the patch will be applied.
* stream the component through a zstd decompressor and the `bipatch` crate, writing the
  result out to the same location on the SSD that any other component source gets
  extracted to.
* Proceed as normal, just like any other extracted component.

[claim]: https://github.com/worldcoin/orb-software/blob/64783406684f9c35fd947aa81e22e3e3c72ad615/update-agent/core/src/claim.rs#L370
[Component]: https://github.com/worldcoin/orb-software/blob/64783406684f9c35fd947aa81e22e3e3c72ad615/update-agent/core/src/components.rs#L60
