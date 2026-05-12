# Package definition for zorb binary
{ pkgs }:
pkgs.stdenv.mkDerivation rec {
  pname = "zorb";
  version = "0.1.0-tmp.0";

  src = pkgs.fetchurl {
    url = "https://github.com/worldcoin/orb-software/releases/download/zorb%2Fv${version}/zorb_x86_64";
    sha256 = "sha256-O06lkfW8kvSMkXu0LMQDrX7tQSz0RcezQnAwTTVaXYE=";
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
