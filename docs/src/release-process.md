# Release Process

Releases are done on a per-component basis, and triggered manually, with the commit used to build the release attached to the generate artifact.

## How to cut a new release
Simply [trigger the release workflow][release workflow]. This will provide you some
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


[delete job]: https://github.com/worldcoin/orb-software/blob/main/.github/workflows/delete-tmp-release.yaml
[release workflow]: https://github.com/worldcoin/orb-software/actions/workflows/single-release.yaml
[retagging]: https://git-scm.com/docs/git-tag#_on_re_tagging
