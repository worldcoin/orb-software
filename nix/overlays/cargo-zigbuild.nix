# rust 1.98 aarch64 links need cargo-zigbuild >= 0.21; drop once nixpkgs ships it.
final: _prev: {
  cargo-zigbuild = final.unstable.cargo-zigbuild.overrideAttrs (old: rec {
    version = "0.23.4";
    src = final.unstable.fetchFromGitHub {
      owner = "rust-cross";
      repo = "cargo-zigbuild";
      tag = "v${version}";
      hash = "sha256-vJLRjqcakU8E2aL3SAyi0twLZ765BYS/nVKDKYQ6eWo=";
    };
    cargoDeps = final.unstable.rustPlatform.fetchCargoVendor {
      inherit src;
      hash = "sha256-ARFcXDd2uv8e7rO01wfWONOFT3eN2SCGd1g727tgn8s=";
    };
  });
}
