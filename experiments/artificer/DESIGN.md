# Artificer Design Doc

## Goals

- Provide a CLI tool to download artifacts from the web and optionally extract or
  post-process them (for example, `tar -xvf artifact.tar.gz`)
- By default, if an artifact changes, the build should error. It should be impossible
  for a change in an artifact to slip by silently without us knowing about it. In other
  words, our builds should be hermetic/reproducible by default.
- We should be able to opt-into weakened reproducibility guarantees on a case-by-case
  basis, in the event that certain dependencies are expected to change underneath us.
  When they do change, we should get clear warnings.
- The system should support a number of hermetic extractors that always produce the same
  output given the same input artifact.
- We should be able to extend the system with custom (potentially non hermetic)
  extractors, easily, as bash scripts.

## Caching Strategy

We will use a content-addressed-storage (CAS) to store downloads in the cache. 
Specifically, we will use [cacache](https://docs.rs/cacache) to provide this
functionality. This makes it possible to share this cache across multiple machines,
atomically write large files into the cache, and verify that the files have not been
mutated when we retrieve them from the cache. Additionally, it is 100% rust and
supports async.

This cache will be located in `$XDG_CACHE_HOME/artificer/store`. Access to it does not
need to be protected or daemonized, due to the aforementioned guarantee that the cache
will validate its integrity when reading/creating links to data.

## Spec and Lockfile

The "spec" is the description of all artifacts and their extractors. This will live at
`artificer.toml`. An example is given in the source code. 

The "lockfile" lives at `artificer.lock` and describes the [subresource integrity][ssri]
checksums of a particular version of an artifact. This ensures that if an artifact is
mutated, we will know because the checksums will not match.

For every artifact in the spec, we will look in the lockfile to see if that artifact
is present in the lockfile:

- Present: Retrieve from cache if cached. Otherwise download, error if checksum doesn't
  match lockfile, cache download if it matched.
- Absent: Download, store checksum in lock, cache download. 

Whenever a download doesn't match the lockfile, we will error and print the expected vs
downloaded checksums. This lets the user update the spec to the new checksum and rerun
artificer.

Note: When we retrieve from cache, if we find that the entry was corrupted we treat
that entry as absent and delete it.

### ...Except for hash=false

The "present in lockfile" scenario only applies when the hash is unspecified or
explicit. When `hash=false`, we will re-download the artifact *every time*. We do still
calculate the lockfile entry for this file, so that we can warn the user when the
contents of the file have changed.

## Artifact out-dir

The `out-dir` field of artificer.toml describes where artifacts will be stored. The
directory names inside `out-dir` are governed by the name of the various artifacts. 

The directory structure will look like:
```
out-dir/
  .artificer-metadata/
    DO_NOT_EDIT
    out.lock

  my-artifact/
    ...
  foo-artifact/
    ...
  ...

```

The special folder `.artificer-metadata` is reserved for artificer specfic metadata.
Most importantly, the `out.lock` is a redundant copy of the main `artificer.lock`,
which represents the state of the `out-dir`. This is important when, for example,
the main lockfile is comitted in git and updates - we can use the lockfile in the
`out-dir` to know if any artifacts need to be extracted again.

Note that `out.lock` does not hash the actual contents of the out-dir, as that would
be prohibitively expensive.

## Extractors

All extractors deposit their output in the `out-dir/<artifact-name>` dir.

To allow developers to work with the extracted outputs, the contents of an artifact
dir are left untouched except in these scenarios:

- Empty dir: We assume this is the first time we are running, and run the extractor.
- Divergence between `artificer.lock` and `out.lock`: We have a new artifact, so we will
  ask for confirmation to delete the dir contents and run the extractor.

This allows developers to mutate the extracted artifacts for development purposes.

### Types of extractors

No-op extractors simply place a hard link to (or copy?) the downloaded artifact inside
the `out-dir` (for example, `out-dir/thermal-cam-util/thermal-cam-util`).

Built-in extractors have hard coded common logic to handle extraction.

Custom extractors will run the specified command with a working directory of
`out-dir/<artifact-name>` and pass as the first argument a temporary hardlink or symlink
to the artifact in the cache.

[ssri]: https://w3c.github.io/webappsec-subresource-integrity/
