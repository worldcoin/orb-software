# Release Process

Releases are done on a per-component basis, and triggered manually. There are a
couple types of release channels, here is how you decide which one to pick:

- `tmp`: Can be cut from any git ref. These releases are [intentionally
  deleted][delete job] after a week from creation. This allows developers to
  temporarily test out a release, but forces them to avoid shipping the release
  to production, as its inherently ephemeral. It also allows us to create as
  many releases as we want for testing purposes, without permanently polluting
  our release history. These releases are always marked as a draft.
- `beta`: Can only be cut from `main`. These are the go-to release type that
  should be consumed by third parties.

## How to cut a new release

1. Check that the `Cargo.toml` of the software component you wish to release is
up to date. We don't use prereleases (`-beta.0`) or metadata `+KK` in the
Cargo.toml, it should just be the regular X.Y.Z format.
2. When in doubt, bump the first non-zero digit. Cargo treats the first
non-zero digit as the "major" version, and unless you are quite sure that your
release has not introduced a breaking change, you should bump this number if
you changed anything in the actual code since the last release.
3. [Trigger the release workflow][release workflow]. This will provide you some
text boxes where you will input information about the release. You can also
control which git rev you are initiating the release on.

## Release Q&A

> Do I need to ask someone to cut a release?

No, you should feel free to cut a release at any time, as long as you followed
the guidelines in this document.

> Why are tags prefixed like `foo-bar/v...`?

This is a monorepo with multiple binaries. In order to allow consumption of
individual binaries that have independent versions (rather than a single
version number shared across the entire repo), we need per-binary releases. To
disambiguate these releases, the releases are prefixed with the name of the
binary.

> Why do we need to suffix version numbers with for example, `+KK`?

We colloquially refer to releases of the orb as "II", "JJ", "KK", etc. To avoid
needing to reference an inherently-brittle table somewhere about which software
component version was written with the intention of being put in a particular
release, we just add three extra characters to the version number here. If you
are unfamiliar on this naming scheme or find it weird, you should probably
[read up on semver][semver metadata]. 

> Why do the version numbers have `-beta.2` in them?

This is called a [prerelease][semver prerelease] in semver. The way we use it
is a bit more specific - we do this so that we can cut multiple releases for
the *same* version number of the underlying software component. For example,
one may release v0.0.1 of the `foo` daemon, and then realize that some setting
in the .service file was misconfigured. If we didn't always use the `-beta.X`
suffix, we would be forced to update the numerical version number of the
underlying software component, including updating the Cargo.toml, even though
the actual software didn't change. This is pretty annoying, and prone to people
getting lazy and not doing it right. So instead we make the numerical version
number less scary by letting you cut as many prereleases as you want on the
same numerical version.

> Its annoying to have such long version numbers like
> `orb-thermal-cam-ctrl/v0.0.43-beta.27+KK`! Why can't we keep it simple and
> just do `orb-thermal-cam-ctrl/v0.0.43`?

Its better to have a descriptive and consistent version scheme than one that is
short and inconsistent.

> Why don't we use `latest` tags?

`latest` tags require the commit that they point to to constantly change. The
[official git docs on retagging][retagging] call this practice "insane". It
causes problems for people's developer experience, but more importantly it
makes builds inherently non-reproducible. There should be a guarantee of
immutability for tags. You can easily cut a new release by clicking a few
buttons, you don't need a `latest` tag.

> Why do we use `on: workflow_dispatch` instead of `on: tag` for the release?

If we created releases when a tag is pushed, there is a window of time where
the tag exists without any associated release, since CI is still building the
artifacts. Additionally, if CI fails, the tag is now stranded and either needs
to stay around forever or get deleted (eliminating the guarantee of immutable
tags). Instead, we first have CI successfully build the release, and then both
tag and publish the release at the same time. This also allows more control
over the contents that a tag points to - for example, CI can enforce that tags
on `-beta` only can happen on the `main` branch.

> Doesnt deleting tags on the `tmp` channel defeat the point of immutability?

To a large extent yes. But we are being up-front here and explicitly warning
you that `tmp` tags are *mutable* and *ephemeral*, whereas the others  are not.
Use `tmp` tags at your own risk. They only exist for developer convenience.
Truly reproducible build systems shouldn't be using tags without pinning the
checksum regardless, and should be building from source anyway.

[delete job]: https://github.com/worldcoin/orb-software/blob/main/.github/workflows/delete-tmp-release.yaml
[release workflow]: https://github.com/worldcoin/orb-software/actions/workflows/release.yaml
[retagging]: https://git-scm.com/docs/git-tag#_on_re_tagging
[semver metadata]: https://semver.org/spec/v2.0.0.html#spec-item-10 
[semver prerelease]: https://semver.org/spec/v2.0.0.html#spec-item-9
