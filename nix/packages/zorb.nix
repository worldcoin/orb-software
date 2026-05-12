# Package definition for zorb binary
{ pkgs }:
pkgs.stdenv.mkDerivation rec {
  pname = "zorb";
  version = "0.1.0-beta.0";

  src = pkgs.fetchurl {
    url = "https://github.com/worldcoin/orb-software/releases/download/zorb%2Fv${version}/zorb_x86_64";
    sha256 = "sha256-JG7dCdrAyyrPVHT5MeIm3e1pQw2u+ifPo0Q5R2VXlSQ=";
  };

  dontUnpack = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin
    cp $src $out/bin/zorb
    chmod +x $out/bin/zorb

    runHook postInstall
  '';

  meta = with pkgs.lib; {
    description = "Helper for zenoh introspection and conditional execution";
    homepage = "https://github.com/worldcoin/orb-software";
    license = licenses.mit;
    mainProgram = "zorb";
    platforms = [ "x86_64-linux" ];
  };
}
