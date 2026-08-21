# Packages per-crate payloads (see `cargo x android-apex-payload`) into
# signed `.apex` files.
#
# `apexer` isn't a standalone downloadable tool: it and the protobuf
# schemas for apex_manifest/apex_build_info only exist in AOSP source, so we
# fetch just the `system/apex` repo and build the host tool from it,
# substituting nixpkgs packages for the rest of its usual (normally
# AOSP-build-only) toolchain: android-tools for mke2fs.android/e2fsdroid
# (ext4 payload) and avbtool, e2fsprogs for resize2fs, aapt for aapt2.
#
# Payload filesystem is ext4, not erofs: getting --canned_fs_config support
# out of mkfs.erofs requires it to be built with -DWITH_ANDROID, which
# transitively needs AOSP's libcutils/libbase/liblog (system/core) - a much
# bigger and less certain undertaking than reusing android-tools' already
# AOSP-built, working e2fsdroid/mke2fs.android binaries for ext4.
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

  # apexer.py's ext4 path loads this via `pkgutil.get_data('apexer',
  # 'mke2fs.conf')` - expects it alongside apexer.py in the same package
  # dir. Same single-file-fetch approach as manifest.py above, rather than
  # cloning all of system/extras for it.
  mke2fsConfBase64 = pkgs.fetchurl {
    url = "https://android.googlesource.com/platform/system/extras/+/refs/tags/${aospRev}/ext4_utils/mke2fs.conf?format=TEXT";
    hash = "sha256-AhGACInsNDQ9dd5PUictSo+SBt8MKSFh+EpywjFY5Og=";
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
    ln -s ${pkgs.android-tools}/bin/mke2fs.android $out/bin/mke2fs
    ln -s ${pkgs.android-tools}/bin/e2fsdroid $out/bin/
    ln -s ${pkgs.e2fsprogs}/bin/resize2fs $out/bin/
    ln -s ${pkgs.aapt}/bin/aapt2 $out/bin/
    ln -s ${pkgs.android-tools}/bin/avbtool $out/bin/

    # apexer calls `sefcontext_compile -o <out> <in>` to compile
    # file_contexts into binary form, but android-tools' e2fsdroid expects
    # the plain-text format instead - so "compiling" here is just a copy.
    cat <<'EOF' > $out/bin/sefcontext_compile
    #!${pkgs.runtimeShell}
    cp -- "$3" "$2"
    EOF
    chmod +x $out/bin/sefcontext_compile
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
      base64 -d ${mke2fsConfBase64} > $out/lib/apexer/mke2fs.conf
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
  # android-apex-payload`) into `<name>.apex`, signing it with a throwaway
  # key generated on first use. Real device deployment needs a real,
  # securely-managed signing key instead - this is for local testing.
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

      if [ ! -f "$work/key.pem" ]; then
        openssl genrsa -out "$work/key.pem" 4096 2>/dev/null
      fi

      compile-apex-manifest < "$payload/apex_manifest.json" > "$work/apex_manifest.pb"

      apexer -v \
        --manifest "$work/apex_manifest.pb" \
        --file_contexts "$payload/file_contexts" \
        --canned_fs_config "$payload/canned_fs_config" \
        --key "$work/key.pem" \
        --payload_type image \
        --payload_fs_type ext4 \
        --android_jar_path "${androidJar}" \
        --do_not_check_keyname \
        --force \
        "$payload/content" "$out"
    '';
  };
in
{
  inherit apexer compileApexManifest buildApex androidJar;
}
