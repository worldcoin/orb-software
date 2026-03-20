# Package definition for orb-hil binary
{ pkgs }:
pkgs.stdenv.mkDerivation rec {
  pname = "orb-hil";
  version = "0.0.2-beta.19";

  src = pkgs.fetchurl {
    url = "https://github.com/worldcoin/orb-software/releases/download/orb-hil%2Fv${version}/orb-hil_x86_64";
    sha256 = "sha256-8Q6THMhmZnmFMqTKH6QwCfZvUmerzjQe1yewu6qsxp0=";
  };

  dontUnpack = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/bin
    cp $src $out/bin/orb-hil
    chmod +x $out/bin/orb-hil

    runHook postInstall
  '';

  meta = with pkgs.lib; {
    description = "Hardware-in-loop testing tool for Orb";
    homepage = "https://github.com/worldcoin/orb-software";
    license = licenses.mit;
    mainProgram = "orb-hil";
    platforms = [ "x86_64-linux" ];
  };
}
