# orb-build-info

Helper library for collecting information about the artifact
being built.

## How to use

First, add the following dependencies:
```toml
[dependencies]
orb-build-info.workspace = true

[build-dependencies]
orb-build-info = { workspace = true, features = ["build-script"] }
```

Then make a `build.rs` script:
```rust
fn main() {
    orb_build_info::initialize().expect("failed to initialize")
}
```

Finally, in `lib.rs` or `main.rs`:

```rust
use orb_build_info::{BuildInfo, make_build_info};

const BUILD_INFO: BuildInfo = make_build_info!();
```

You can now access the `BUILD_INFO` constant anywhere in your crate, and do things
like report the git commit, etc. See how it is used in `orb-mcu-util` and other
binaries.
