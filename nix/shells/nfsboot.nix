# Creates a FHS chroot with all the necessary tools used for flashing an orb.
# NOTE(@thebutlah): AFAICT this is not a dev shell. But it can act like one if
# you make the `runScript` bash, and then run it.
{ pkgs }:
(pkgs.mkShell {
  # Nix makes the following list of dependencies available to the development
  # environment.
  buildInputs = (
    with pkgs;
    [
      libguestfs-with-appliance
      abootimg
    ]
  );
})
