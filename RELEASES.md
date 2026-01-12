# Release Management and Principles

When orb-software was created, we wanted to ensure that we can simplify the
development and release process of our software, and avoid the situation where
a single change needs to be coordinated across many repos. In order to
accomplish this, there needs to be a coherent strategy in place for dealing
with versioning of libraries as well as the naming of their artifacts. This
document describes the rationale and design of our release strategy.

## How should we deal with [semver](https://semver.org/)?

Many tools bake the assumption that dependencies use semver into their DNA.
This is because unlike other naming schemes, semver encodes *version
compatibility* into its semantics. Many tools take advantage of the semantic
compatibility - for example, cargo will automatically use the latest
semver-compatible version of a library when generating or updating its
lockfile. Other tools do similar things.

However while convenient and often necessary, this automatic upgrade, or
version unification, is dangerous if the semver guarantees about compatibility
aren’t actually upheld. A lockfile helps mitigate this, but is not a perfect
solution - developers often regenerate their lockfiles themselves, and in
certain cases tools will introduce upgrades the developer might not expect. For
example with cargo, if I have crate A at `v0.1.0` in my lockfile, and crate B
depends on `v0.1.1`, and I add crate b to my project, cargo will automatically
unify the two versions of crate A to both point to `v0.1.1`. But if `v0.1.0` and
`v0.1.1` are not truly compatible with each other, best case is a compile error,
and worst case is a sneaky runtime error (which our nonexistent test suite will
not detect).

An easy solution to this is to not to pretend to commit to stability when we
aren’t actually being rigorous about checking compatibility. If instead every
release is a breaking change, there is no way that cargo or other tools will
upgrade them. We can accomplish this by versioning our code as `v0.0.X`,
incrementing the patch number each release. In legacy cases where we did not do
this (for example, token daemon binary already has a nonzero major number) we
can still accomplish this by incrementing the first non-zero digit.

However this implies that if we want to stay up to date, we must be sure that
we are constantly bumping version numbers across many repos, otherwise we will
be running different old versions of code everywhere. This becomes tedious
really fast. Luckily, monorepos come in handy here too. When using path
dependencies we no longer need to version individual crates, but only version
the final binaries that have accompanying artifacts. This is because for local
builds using path dependencies, cargo will always use whatever currently exists
on the filesystem instead of the version number, even if you specify both
`version` and `path`.

Hopefully the above context sufficiently motivates why marking all releases as
a breaking change under semver is important.

## What is our naming scheme for tags in orb-software?

The simplest versioning strategy, and the one that was originally adopted, was
to have a *single* repo-wide version that all of the binaries and artifacts in
orb-software would be versioned under. This had several advantages:

- A single `vX.Y.Z` tag is sufficient to cut a release for the entire repo.
- Consistency - no one has to wonder what version different crates are on.
- I can force everyone to use `v0.0.X`, avoiding a future situation where we have
versions like `10224.0.0`.
- CI is simpler - all the binaries across the whole repo
can be put into the same release, as separate files.

Unfortunately, this didn’t last long. The main issue with this approach is when
we want to merge pre-existing projects into orb-software. For example, the repo
`orb-update-agent` is on version `v5.2.1` as of the time of writing. If there is
a single release number for all of orb-software, it implies that the release
number of the update agent must downgrade to match the rest of the repo, or the
rest of the repo must upgrade to match the update agent. Neither solution is
good. This is particularly dangerous if we downgrade the version number to
match the repo - people generally assume that version numbers always increment,
and now describing `v1.0.0` of orb-update-agent is ambiguous - did you mean the
one in the orb-update-agent repo or the one in orb-software (if we ever got to
that version number).

So now, we are going to need to support multiple independently versioned
artifacts on multiple independent tags. In other words instead of a global
`v0.0.Z` tag, we will need tags in the following format:

`my-software-component/vX.Y.Z`, where `my-software-component` will be the particular
artifact name or binary name (generally we will only need to version binaries),
like `orb-update-agent`.

> Note: we are using the `/` because it is an invalid character in semver, so
> it provides a clear way to disambiguate the component name from the semver
> string. There is some room for confusion there, because it is common for
> branches to follow a similar prefix, for example `thebutlah/my-branch-name`.
> This is fine because tags and branches do not share the same namespace. To
> avoid ambiguity when consuming git dependencies, either specify the commit
> revision (`rev = "d3adb33f"` in cargo ) or specify the tag (`tag =
> "foo/v0.0.11"` in cargo), don't refer to tags via the `rev` property.

Avoid nonzero major and minor versions, unless doing so would cause the version
number to decrease.

## How do we deploy artifacts in CI?

Implementing the above naming scheme for tags is not entirely trivial. The strategy
we came up with is the following:

- On tagged commits or commits to `main`, activate LTO and other optimizations we
expect to use in our final distributed artifacts.
- Regardless of whether its on
`main` or a tag, publish artifacts associated with this workflow. These show up
in the summary section of the workflow. These artifacts should always be
published to the workflow summary, because they are useful in development for
debugging and for conveniently kicking off a build.
- Have a release job in the workflow
that only runs on `main` or a tag. This job is responsible for creating new
releases.
- Inspect the current ref name. Assert that starts with `*/`, and then
split at the `/` to get the component name and the semver string.
- Download the artifacts associated with the workflow. Filter them to only the single artifact
that matches the component name.
- Upload this to the release, named according to
its component name (i.e. same name as the artifact)
- Hash the file with sha256,
upload another file called `component-name.sha256`. This allows automated tools
to easily get the checksums of files without needing to download the entire
binary.

## Enforcing immutability of tags and releases

For guaranteed validity of release hashes and to help avoid spurious breakage
in other workflows and tools, we need to ensure that once a tag is published,
it isn’t deleted or mutated, and neither are its release artifacts. There are
many reasons to do this - avoiding spurious breakages in other build systems
and repos, improving reproducibility of builds, “best practice”, not making
your coworkers hate you, making `git pull –tags` work out of the box without
`--force`. The list goes on, and even the [official git docs][git retag] call
mutating a tag “insane”.

To ensure immutability of tags, we will use github’s tag rulesets, to ensure
that any tag not matching [a regex][tag regex] will be denied from being
pushed, and no tags can be deleted or updated after being pushed.

Because tags are immutable, we will not support the strategy we adopted in
other repos where there is a tag called `latest` and its contents get replaced.
The `latest` tag was useful to allow other repos to consume software that we
have merged but didn't want to fully release. Unfortunately, what ended up
happening is that this eliminated reproduciblity in the other repos' builds,
and was a big footgun. If you desperately need to test out some code, just cut
a prerelease by following semver conventions - i.e. use the format
`my-software-component/vX.Y.Z-prerelease-description.0` where the `.0` part
can be incremented as needed. DO NOT delete the tag or mutate its contents
later. I will find you.

[git retag]: https://git-scm.com/docs/git-tag#_on_re_tagging
[tag regex]: regexr.com/7ro70
