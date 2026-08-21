# Packages per-crate payloads (see `cargo x android-apex-payload`) into
# signed `.apex` files.
#
# `apexer` isn't a standalone downloadable tool: it and the protobuf
# schemas for apex_manifest/apex_build_info only exist in AOSP source, so we
# fetch just the `system/apex` repo and build the host tool from it,
# substituting nixpkgs packages for the rest of its usual (normally
# AOSP-build-only) toolchain: android-tools for avbtool, aapt for aapt2.
#
# Payload filesystem is erofs. nixpkgs' erofs-utils can't produce a
# canned_fs_config-capable mkfs.erofs (that needs a -DWITH_ANDROID build
# plus AOSP's libcutils from system/core, which isn't buildable outside a
# full AOSP tree - see git history for the ext4-based approach we used
# before finding this). Instead we fetch Google's own prebuilt mkfs.erofs
# host binary directly from kernel/prebuilts/build-tools (verified: it has
# both --file-contexts and --fs-config-file, and produces smaller images
# than ext4 thanks to lz4hc compression) and patch it to run under nixpkgs'
# glibc via autoPatchelfHook.
{ pkgs }:
let
  # Pinned for reproducibility - bump deliberately, and re-verify the hash
  # below (and that platformVersion still matches an available SDK
  # platform) when you do.
  aospRev = "android-16.0.0_r4";

  apexSrc = pkgs.fetchgit {
    url = "https://android.googlesource.com/platform/system/apex";
    rev = "refs/tags/${aospRev}";
    hash = "sha256-cKJfpbQR42ozKLaqSICeN5GLJtlDxAbeuCTEz1vXVpg=";
  };

  # apexer.py imports several helpers from AOSP's build/soong/scripts/
  # manifest.py (android_ns, find_child_with_attribute,
  # get_children_with_tag, get_indent, parse_manifest, write_xml). Not
  # worth cloning that entire (much larger) repo for one file, so we fetch
  # it directly via googlesource's raw-content endpoint (base64-encoded)
  # and decode it at build time.
  manifestPyBase64 = pkgs.fetchurl {
    url = "https://android.googlesource.com/platform/build/soong/+/refs/tags/${aospRev}/scripts/manifest.py?format=TEXT";
    hash = "sha256-MG7PUB2ZeNexpNgnKunBQl0gzFJA+BLfpTdhcvRnnlc=";
  };

  # Google's own prebuilt mkfs.erofs host binary, built with -DWITH_ANDROID
  # (confirmed via --help: has both --file-contexts and --fs-config-file).
  # Pinned to a specific prebuilts revision - independent of aospRev, but
  # keep them reasonably in sync when bumping either.
  erofsPrebuiltRev = "android-16.0.0_r0.4";
  mkfsErofsBinBase64 = pkgs.fetchurl {
    url = "https://android.googlesource.com/kernel/prebuilts/build-tools/+/refs/tags/${erofsPrebuiltRev}/linux-x86/bin/mkfs.erofs?format=TEXT";
    hash = "sha256-RH7THhG6cld3SDL9vaZxl7u1E1x3P8ClmMIveU7lT4c=";
  };
  # mkfs.erofs dynamically links libc++.so; everything else it needs
  # (libc, libm, libpthread, librt, libdl, libgcc_s) comes from nixpkgs'
  # glibc via autoPatchelfHook. Using AOSP's own libc++.so rather than
  # nixpkgs' to avoid any ABI mismatch with a binary Google built and
  # tested against this exact one.
  mkfsErofsLibcxxBase64 = pkgs.fetchurl {
    url = "https://android.googlesource.com/kernel/prebuilts/build-tools/+/refs/tags/${erofsPrebuiltRev}/linux-x86/lib64/libc%2B%2B.so?format=TEXT";
    hash = "sha256-msZUtf10GxcJ+uNPN5cV0aynTIXPZy/dYVaocfM8lXY=";
  };
  mkfsErofs = pkgs.stdenv.mkDerivation {
    pname = "mkfs-erofs-aosp-prebuilt";
    version = erofsPrebuiltRev;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.autoPatchelfHook ];
    buildInputs = [ pkgs.stdenv.cc.cc.lib ];
    installPhase = ''
      mkdir -p $out/bin $out/lib
      base64 -d ${mkfsErofsBinBase64} > $out/bin/mkfs.erofs
      chmod +x $out/bin/mkfs.erofs
      base64 -d ${mkfsErofsLibcxxBase64} > $out/lib/libc++.so
    '';
  };

  # apexer unconditionally shells out to `aapt2 link -I <android_jar_path>`
  # to build the outer APK-style container, so we need an android.jar for
  # some platform version - not tied to aospRev, just needs to be recent
  # enough to understand whatever AndroidManifest.xml apexer generates.
  platformVersion = "36";
  androidPlatform = pkgs.androidenv.composeAndroidPackages {
    platformVersions = [ platformVersion ];
    includeNDK = false;
    includeEmulator = false;
    includeSystemImages = false;
    includeSources = false;
    includeExtras = [ ];
    buildToolsVersions = [ ];
  };
  androidJar = "${androidPlatform.androidsdk}/libexec/android-sdk/platforms/android-${platformVersion}/android.jar";

  pythonWithProtobuf = pkgs.python3.withPackages (ps: [ ps.protobuf ]);

  apexerToolchain = pkgs.runCommand "apexer-toolchain" { } ''
    mkdir -p $out/bin
    ln -s ${mkfsErofs}/bin/mkfs.erofs $out/bin/
    ln -s ${pkgs.aapt}/bin/aapt2 $out/bin/
    ln -s ${pkgs.android-tools}/bin/avbtool $out/bin/
  '';

  apexer = pkgs.stdenv.mkDerivation {
    pname = "apexer";
    version = aospRev;
    src = apexSrc;
    nativeBuildInputs = [ pkgs.protobuf ];
    buildPhase = ''
      mkdir -p build
      protoc --python_out=build proto/apex_build_info.proto proto/apex_manifest.proto
    '';
    installPhase = ''
      mkdir -p $out/lib/apexer $out/bin
      cp apexer/*.py $out/lib/apexer/
      cp build/proto/*.py $out/lib/apexer/
      base64 -d ${manifestPyBase64} > $out/lib/apexer/manifest.py
      cat <<EOF > $out/bin/apexer
      #!${pkgs.runtimeShell}
      export PYTHONPATH="\$PYTHONPATH:$out/lib/apexer"
      export APEXER_TOOL_PATH="${apexerToolchain}/bin"
      exec ${pythonWithProtobuf}/bin/python3 "$out/lib/apexer/apexer.py" "\$@"
      EOF
      chmod +x $out/bin/apexer
    '';
  };

  # JSON -> compiled apex_manifest.pb, using apexer's own generated
  # protobuf module. Avoids needing AOSP's separate conv_apex_manifest host
  # tool (and whatever repo that would drag in) for this one conversion.
  compileApexManifest = pkgs.runCommand "compile-apex-manifest" { } ''
    mkdir -p $out/bin
    cat <<PYEOF > $out/compile_apex_manifest.py
    import sys, json
    sys.path.insert(0, "${apexer}/lib/apexer")
    from google.protobuf.json_format import Parse
    from apex_manifest_pb2 import ApexManifest
    data = json.load(sys.stdin)
    msg = Parse(json.dumps(data), ApexManifest())
    sys.stdout.buffer.write(msg.SerializeToString())
    PYEOF
    cat <<EOF > $out/bin/compile-apex-manifest
    #!${pkgs.runtimeShell}
    exec ${pythonWithProtobuf}/bin/python3 "$out/compile_apex_manifest.py"
    EOF
    chmod +x $out/bin/compile-apex-manifest
  '';

  # Packages one already-staged payload dir (as produced by `cargo x
  # android-apex-payload`) into `<name>.apex`, signing it with the key at
  # $APEX_SIGNING_KEY if set (CI wires this to a real, securely-managed key
  # from a GitHub Environment secret), or otherwise a throwaway key
  # generated fresh for this invocation - fine for local testing, but never
  # persisted or reused across runs.
  buildApex = pkgs.writeShellApplication {
    name = "build-apex";
    runtimeInputs = [
      compileApexManifest
      apexer
      pkgs.openssl
    ];
    text = ''
      set -euo pipefail
      if [ $# -ne 2 ]; then
        echo "usage: build-apex <payload-dir> <out.apex>" >&2
        exit 1
      fi
      payload=$1
      out=$2
      work=$(mktemp -d)
      trap 'rm -rf "$work"' EXIT

      key="''${APEX_SIGNING_KEY:-}"
      if [ -z "$key" ]; then
        key="$work/key.pem"
        openssl genrsa -out "$key" 4096 2>/dev/null
      fi

      compile-apex-manifest < "$payload/apex_manifest.json" > "$work/apex_manifest.pb"

      apexer -v \
        --manifest "$work/apex_manifest.pb" \
        --file_contexts "$payload/file_contexts" \
        --canned_fs_config "$payload/canned_fs_config" \
        --key "$key" \
        --payload_type image \
        --payload_fs_type erofs \
        --android_jar_path "${androidJar}" \
        --do_not_check_keyname \
        --force \
        "$payload/content" "$out"
    '';
  };
in
{
  inherit
    apexer
    compileApexManifest
    buildApex
    androidJar
    ;
}
