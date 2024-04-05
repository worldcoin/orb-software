# Contributing

We plan to accept contributions at a later date, but do not have bandwidth to
review PRs currently.

Likewise, we are providing this source code for the benefit of the community,
but cannot commit to any SemVer or API stability guarantees. Be warned: we may
change things in a backwards-incompatible way at any time!

## Coding Guidelines

- Code must pass CI - see the github actions workflow for the most up to date
  checks.
- There can be no copyleft or closed source dependencies.
- Prefer using cargo [workspace inheritance] when possible.
- Prefer cross-platform code. Please consult [deps tests](deps-tests) for more
  info.
- Any binaries that do not run on all platforms must be documented as such in
  their README.md file and added to the tests in `deps-tests`.
- Use `#![forbid(unsafe_code)]` whenever possible. This narrows the surface
  area for debugging memory safety issues.
- Prefer the [nix crate][nix crate] for safe unix APIs instead of raw unsafe
  libc. PRs that use `libc` will be rejected if an equivalent safe function in
  `nix` exists.
- PR names and the final squashed commit that gets merged, should start with an
  area prefix, like `ir-camera:`. This helps disambigutate which part of the
  monorepo changed at a glance.

## First time Setup

1. [Install nix][install nix]. This works for both mac and linux, windows is
only supported via [WSL2][WSL2].
2. Ensure that you have these lines in your `~/.config/nix/nix.conf`: 
   ```
   experimental-features = nix-command flakes max-jobs = auto
   ```
3. Install direnv: `nix profile install nixpkgs#direnv`
4. [Hook direnv](https://direnv.net/docs/hook.html) into your shell.
5. Tell direnv to use the nix flake with `cp .envrc.example .envrc`. You can
customize this file if you wish. We recommend filling in your cachix token if
you have one.
6. Follow the instructions on vendoring proprietary SDKs in the subsequent
section.
7. Run `direnv allow` in the repository's root directory.
8. If you are on macos, run the following:
   ```bash
   brew install dbus
   brew services start dbus
   ```

### Vendoring Proprietary SDKs

Although all of Worldcoin's code in this repo is open source, some of the
sensors on the orb rely on proprietary SDKs provided by their hardware vendors.
Luckily, these are accessible without any cost.

To get started, you will need to download these SDKs. The process for this
depends on if you are officially affiliated with Worldcoin.

#### If you have access to Worldcoin private repos

1. Create a [personal access token][pac] from github to allow you to use
private git repos over HTTPS.
2. Append the following to your `~/.config/nix/nix.conf`: 
   ```
   access-tokens = github.com=github_pat_YOUR_ACCESS_TOKEN_HERE
   ```
3. Test everything works so far by running `nix flake metadata
github:worldcoin/priv-orb-core`. You should see a tree of info. If not, you
probably don't have your personal access token set up right - post in
#public-orb-software on slack for help.

#### If you don't have access to Worldcoin private repos

1. Go to https://developer.thermal.com and create a developer account.
2. Download the 4.1.0.0 version of the SDK (its in the developer forums).
3. Extract its contents, and note down the dir that *contains* the
`Seek_Thermal_SDK_4.1.0.0` dir. Save this in an environment variable of your
choice, such as `SEEK_SDK_OVERRIDE`.
4. modify your `.envrc` like this: `use flake --override-input seekSdk
"$SEEK_SDK_OVERRIDE"`

## Building

We use `cargo zigbuild` for most things. The following cross-compiles a binary
in the `foobar` crate to the orb:

```bash 
cargo zigbuild --target aarch64-unknown-linux-gnu --release -p foobar
```

## Debugging

### Tokio Console

Some of the binaries have support for [tokio console][tokio console]. This is
useful when debugging async code. Arguably the most useful thing to use it for
is to see things like histograms of `poll()` latencies, which can reveal when
one is accidentally blocking in async code. Double check that the binary you
wish to debug actually supports tokio console - support has to be manually
added, it isn't magically available by default.

To use tokio console, you will need to scp a cross-compiled tokio-console
binary to the orb. To do this, just clone the [repo][tokio console] and use
`cargo zigbuild --target aarch64-unknown-linux-gnu --release --bin
tokio-console`, then scp it over.

Note:
> tokio-console supports remote debugging via grpc, but I haven't figured out
> how to get the orb to allow that yet - I assume we have a firewall in place
> to prevent arbitrary tcp access, even in dev orbs.

Then, you must build the binary you want to debug unstable tokio features
enabled. To do this, uncomment the line in
[.cargo/config.toml](.cargo/config.toml) about tokio unstable. 

Finally, make sure that the binary has the appropriate RUST_LOG level set up.
try using `RUST_LOG="info,tokio=trace,runtime=trace"`.

Finally, run your compiled binary and the compiled `tokio-console` binary on
the orb. You should see a nice TUI.

Note that it is recommended but not required to have symbols present to improve
the readability of debugging.

## Release Process

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

### How to cut a new release

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

### Release Q&A

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
[nix crate]: https://docs.rs/nix
[install nix]: https://zero-to-nix.com/start/install
[pac]: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-fine-grained-personal-access-token
[release workflow]: https://github.com/worldcoin/orb-software/actions/workflows/release.yaml
[retagging]: https://git-scm.com/docs/git-tag#_on_re_tagging
[semver metadata]: https://semver.org/spec/v2.0.0.html#spec-item-10 
[semver prerelease]: https://semver.org/spec/v2.0.0.html#spec-item-9
[tokio console]: https://github.com/tokio-rs/console?tab=readme-ov-file#extremely-cool-and-amazing-screenshots
[workspace inheritance]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table
[WSL2]: https://learn.microsoft.com/en-us/windows/wsl/install
