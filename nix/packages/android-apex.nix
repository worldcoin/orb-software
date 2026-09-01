# Packages per-crate payloads staged by `cargo x android-apex` into
# signed `.apex` files.
#
# `apexer` and its apex_manifest/apex_build_info protobuf schemas only
# exist in AOSP source, not as a standalone tool, so we fetch just the
# `system/apex` repo and build it, substituting nixpkgs for the rest of
# its usual AOSP-build-only toolchain: android-tools for avbtool, aapt for
# aapt2.
#
# Payload filesystem is erofs. nixpkgs' erofs-utils can't build a
# canned_fs_config-capable mkfs.erofs (needs AOSP's libcutils, not
# buildable outside a full AOSP tree - see git history for the ext4
# approach used before this). Instead we fetch Google's own prebuilt
# mkfs.erofs from kernel/prebuilts/build-tools and patch it to run under
# nixpkgs' glibc via autoPatchelfHook.
{ pkgs }:
let
  # googlesource's raw-content endpoint only serves files base64-encoded
  # (`?format=TEXT`); this fetches and decodes in one step.
  fetchGoogleSourceFile =
    { url, hash }:
    pkgs.runCommand "google-source-file" { } ''
      base64 -d ${pkgs.fetchurl { inherit url hash; }} > $out
    '';

  # Pinned for reproducibility - bump deliberately, and re-verify the hash
  # below when you do.
  aospRev = "android-16.0.0_r4";

  apexSrc = pkgs.fetchgit {
    url = "https://android.googlesource.com/platform/system/apex";
    rev = "refs/tags/${aospRev}";
    hash = "sha256-cKJfpbQR42ozKLaqSICeN5GLJtlDxAbeuCTEz1vXVpg=";
  };

  # apexer.py imports helpers from AOSP's build/soong/scripts/manifest.py -
  # not worth cloning that whole (much larger) repo for one file.
  manifestPy = fetchGoogleSourceFile {
    url = "https://android.googlesource.com/platform/build/soong/+/refs/tags/${aospRev}/scripts/manifest.py?format=TEXT";
    hash = "sha256-MG7PUB2ZeNexpNgnKunBQl0gzFJA+BLfpTdhcvRnnlc=";
  };

  # AOSP's published AVB test key (external/avb/test/data) - test-only,
  # matches apexer's SHA256_RSA4096 signing. Never a real key.
  testKey = fetchGoogleSourceFile {
    url = "https://android.googlesource.com/platform/external/avb/+/refs/tags/${aospRev}/test/data/testkey_rsa4096.pem?format=TEXT";
    hash = "sha256-5qt2JnvmWaLN1QclfuLxm4oKynyxPAjnBp2tiirUaiA=";
  };

  # testKey's public half, extracted once at build time. apexer only embeds
  # an `apex_pubkey` entry in the outer container when given `--pubkey` (see
  # buildApex below) - without it, apexd's boot-time VerifyApexVerity compares
  # the AVB footer's real key against an empty string, which can never match,
  # so the apex silently never activates (isActive=false, no mount, no error
  # in the userspace log - see docs/src/android.md).
  testPubkey =
    pkgs.runCommand "apex-test-pubkey"
      {
        nativeBuildInputs = [
          pkgs.android-tools
          pkgs.openssl
        ];
      }
      ''
        avbtool extract_public_key --key ${testKey} --output $out
      '';

  # AOSP's published test cert/key pair (build/target/product/security) -
  # test-only, standard AOSP "testkey" used to APK-sign the outer container
  # (see buildApex below). Never a real key.
  testCertX509 = fetchGoogleSourceFile {
    url = "https://android.googlesource.com/platform/build/+/refs/tags/${aospRev}/target/product/security/testkey.x509.pem?format=TEXT";
    hash = "sha256-vjogVTJxUkt/DdXLQcPC+iyqQfxeGPo/OAbUl92CDtM=";
  };
  testCertPk8 = fetchGoogleSourceFile {
    url = "https://android.googlesource.com/platform/build/+/refs/tags/${aospRev}/target/product/security/testkey.pk8?format=TEXT";
    hash = "sha256-kdco/lAWlHmJdQeAAvj6mqfdI7RZEBVAG2O30GrFKOk=";
  };

  # Google's own prebuilt mkfs.erofs (nixpkgs' erofs-utils lacks the
  # -DWITH_ANDROID build canned_fs_config needs). Pinned separately from
  # aospRev: this repo's tags use a different numbering scheme.
  erofsPrebuiltRev = "android-16.0.0_r0.4";
  mkfsErofsBin = fetchGoogleSourceFile {
    url = "https://android.googlesource.com/kernel/prebuilts/build-tools/+/refs/tags/${erofsPrebuiltRev}/linux-x86/bin/mkfs.erofs?format=TEXT";
    hash = "sha256-RH7THhG6cld3SDL9vaZxl7u1E1x3P8ClmMIveU7lT4c=";
  };
  # AOSP's own libc++.so, to avoid an ABI mismatch with nixpkgs' - the
  # rest of mkfs.erofs's deps come from nixpkgs' glibc via autoPatchelfHook.
  mkfsErofsLibcxx = fetchGoogleSourceFile {
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
      install -m755 ${mkfsErofsBin} $out/bin/mkfs.erofs
      cp ${mkfsErofsLibcxx} $out/lib/libc++.so
    '';
  };

  # apexer shells out to `aapt2 link -I <android_jar_path>` for the outer
  # APK-style container, so we need some android.jar - not tied to aospRev,
  # just recent enough for whatever AndroidManifest.xml apexer generates.
  platformVersion = "36";
  androidPlatform = pkgs.androidenv.composeAndroidPackages {
    platformVersions = [ platformVersion ];
    includeNDK = false;
    includeEmulator = false;
    includeSystemImages = false;
    includeSources = false;
    includeExtras = [ ];
  };
  androidJar = "${androidPlatform.androidsdk}/libexec/android-sdk/platforms/android-${platformVersion}/android.jar";

  # AOSP's own `signapk` tool (build/make/tools/signapk, prebuilt jar +
  # native lib) - matches how orb-engine signs its outer APK-style
  # container (see orb-engine/nix/apex-tools.nix), instead of the SDK's
  # `apksigner`, which applies newer v2/v3 signature schemes that real
  # AOSP-built system/vendor packages don't use.
  androidPlatformPrebuiltsSdk = pkgs.fetchgit {
    url = "https://android.googlesource.com/platform/prebuilts/sdk";
    rev = "refs/tags/${aospRev}";
    hash = "sha256-I4spD5aMre+SF2zC994FVTXSj8IJKoF8ZkhHraOO+ZY=";
    sparseCheckout = [
      "tools/lib"
      "tools/linux/lib64"
    ];
  };

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
      cp ${manifestPy} $out/lib/apexer/manifest.py
      cat <<EOF > $out/bin/apexer
      #!${pkgs.runtimeShell}
      export PYTHONPATH="\$PYTHONPATH:$out/lib/apexer"
      export APEXER_TOOL_PATH="${apexerToolchain}/bin"
      exec ${pythonWithProtobuf}/bin/python3 "$out/lib/apexer/apexer.py" "\$@"
      EOF
      chmod +x $out/bin/apexer
    '';
  };

  # apexer.py shells out to /usr/bin/fallocate, /bin/cp, /bin/ls - absent
  # on a bare NixOS host, so run it inside a synthetic FHS root.
  apexerFHS = pkgs.buildFHSEnv {
    name = "apexer";
    targetPkgs = pkgs: [
      apexer
      pkgs.coreutils
      pkgs.util-linux
    ];
    runScript = "apexer";
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
  # android-apex`) into `<name>.apex`, always signed with the AOSP test
  # key/cert above - never use this for a real release.
  buildApex = pkgs.writeShellApplication {
    name = "build-apex";
    runtimeInputs = [
      compileApexManifest
      apexerFHS
      pkgs.jdk21_headless
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

      compile-apex-manifest < "$payload/apex_manifest.json" > "$work/apex_manifest.pb"

      # apexer only AVB-signs the inner payload (--key below); APK-sign the
      # outer container here too, so both apexd and PackageManager accept it.
      apexer -v \
        --manifest "$work/apex_manifest.pb" \
        --file_contexts "$payload/file_contexts" \
        --canned_fs_config "$payload/canned_fs_config" \
        --key "${testKey}" \
        --pubkey "${testPubkey}" \
        --payload_type image \
        --payload_fs_type erofs \
        --android_jar_path "${androidJar}" \
        --do_not_check_keyname \
        --test_only \
        --force \
        "$payload/content" "$work/unsigned.apex"

      java \
        -Djava.library.path="${androidPlatformPrebuiltsSdk}/tools/linux/lib64" \
        -jar "${androidPlatformPrebuiltsSdk}/tools/lib/signapk.jar" \
        -a 4096 \
        "${testCertX509}" \
        "${testCertPk8}" \
        "$work/unsigned.apex" \
        "$out"
    '';
  };
in
{
  inherit buildApex;
}
