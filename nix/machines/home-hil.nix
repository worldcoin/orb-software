# home-manager configuration for all HILs.
{ pkgs, lib, ... }:
let
  xdg = ../xdg;
  packages = ../packages;
in
{
  home = {
    username = "worldcoin";
    homeDirectory = "/home/worldcoin";
    packages = import "${packages}/hil.nix" {
      inherit pkgs;
    };
    sessionVariables = {
      EDITOR = "nvim";
      VISUAL = "nvim";
    };
    shellAliases = {
      a64 = "echo aarch64-unknown-linux-gnu";
      x86 = "echo x86_64-unknown-linux-gnu";
    };
  };

  programs.home-manager.enable = true;
  # shell stuff
  programs.zsh = {
    enable = true;
    autosuggestion.enable = true;
    enableCompletion = true;
    oh-my-zsh.enable = true;
    initContent = lib.mkOrder 1000 ''
      set -o vi
    '';
  };
  programs.starship = {
    enable = true;
    settings = lib.trivial.importTOML "${xdg}/starship.toml";
  };
  programs.zoxide = {
    enable = true;
    enableBashIntegration = true;
    enableZshIntegration = true;
    options = [ "--cmd cd" ];
  };
  programs.atuin = {
    enable = true;
    enableBashIntegration = true;
    enableZshIntegration = true;
    package = pkgs.unstable.atuin;
  };
  programs.yazi = {
    enable = true;
    enableBashIntegration = true;
    enableZshIntegration = true;
  };
  programs.zellij = {
    enable = true;
    enableBashIntegration = true;
    enableZshIntegration = true;
    package = pkgs.unstable.zellij;
  };
  programs.direnv = {
    enable = true;
    enableBashIntegration = true; # see note on other shells below
    enableZshIntegration = true;
    nix-direnv.enable = true;
  };

  xdg.enable = true;
  xdg.configFile = {
    "nvim" = {
      source = pkgs.fetchFromGitHub {
        owner = "thebutlah";
        repo = "init.lua";
        rev = "ea6cc4e6f98cd99e7ab26dd1a750d34919adc454";
        hash = "sha256-P6rhEBTOuXf28L+0EYtdyt3q0bxSKnNFFmuykPpFrQ0=";
      };
    };
    "zellij/config.kdl" = {
      source = "${xdg}/zellij.kdl";
    };
    "atuin/config.toml" = {
      source = "${xdg}/atuin.toml";
    };
  };

  fonts.fontconfig.enable = true;

  # Nicely reload system units when changing configs
  systemd.user.startServices = "sd-switch";

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.11";
}
