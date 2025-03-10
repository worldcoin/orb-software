# Cargo license stubs

Sometimes bindings to a native library don't actually match the native library's
license.

Cargo "license stubs" are a pattern of making a "fake/stubbed" crate, putting the
license of the original project in, and adding it as a dependency of the bindings.
This exposes the license info to to tools like `cargo deny` that inspect cargo's license
field to ensure license compliance.
