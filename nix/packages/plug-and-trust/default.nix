{ pkgs }:
pkgs.stdenv.mkDerivation rec {
  pname = "plug-and-trust";
  version = "04.07.01";

  src = pkgs.fetchFromGitHub {
    owner = "NXP";
    repo = "plug-and-trust";
    rev = "v${version}";
    hash = "sha256-AbXqddP7veu9R3jYczhuRa8VtocsQIKCxptekAoEMCw=";
  };

  nativeBuildInputs = with pkgs; [
    cmake
    pkg-config
  ];

  buildInputs = with pkgs; [
    openssl
  ];

  cmakeFlags = [
    "-DPTMW_HostCrypto=OPENSSL"
    "-DPTMW_SE05X_Auth=None"
    "-DPTMW_SE05X_Ver=07_02"
    "-DPTMW_Applet=SE050_C"
    "-DBUILD_SHARED_LIBS=OFF"
  ];

  configurePhase = ''
    runHook preConfigure

    mkdir -p cmake-src
    cp ${./CMakeLists.txt} cmake-src/CMakeLists.txt
    cp ${./plug-and-trust.pc.in} cmake-src/plug-and-trust.pc.in

    cmake -S cmake-src -B build \
      -DSIMW_LIB_DIR="$PWD" \
      -DCMAKE_INSTALL_PREFIX="$out" \
      -DPLUG_AND_TRUST_VERSION="${version}" \
      "''${cmakeFlagsArray[@]}"

    runHook postConfigure
  '';

  buildPhase = ''
    runHook preBuild

    cmake --build build

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    cmake --install build

    mkdir -p $out/include/plug-and-trust $out/lib
    cp -r sss/inc sss/port/default hostlib/hostLib/inc $out/include/plug-and-trust/

    runHook postInstall
  '';

  meta = with pkgs.lib; {
    description = "NXP Plug & Trust middleware mini package (SE05x) library";
    homepage = "https://github.com/NXP/plug-and-trust";
    license = licenses.bsd3;
    platforms = platforms.linux;
  };
}
