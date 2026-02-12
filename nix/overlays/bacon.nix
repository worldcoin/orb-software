# Use bacon from nixpkgs-unstable for a newer version.
final: _prev: {
  bacon = final.unstable.bacon;
}
