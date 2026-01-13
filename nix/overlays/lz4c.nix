# adds lz4c -> lz4 symlink so that older software that still needs the symlink
# (like the jetson flashing scripts) doesn't break.
final: prev: {
  lz4c = prev.runCommandLocal "lz4c" { buildInputs = [ prev.lz4 ]; } ''
    mkdir -p $out/bin
    ln -s ${prev.lib.getExe prev.lz4} $out/bin/lz4c
  '';
}
