# home-manager configuration for all HILs.
{ pkgs, lib, ... }: {
  home = {
    username = "worldcoin";
    homeDirectory = "/home/worldcoin";
    sessionVariables = {
      EDITOR = "nvim";
      VISUAL = "nvim";
    };
  };

  programs.home-manager.enable = true;
  # shell stuff
  programs.zsh = {
    enable = true;
    autosuggestion.enable = true;
    enableCompletion = true;
    oh-my-zsh.enable = true;
    initExtra = ''
      set -o vi
    '';
  };
  programs.starship = {
    enable = true;
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
  };

  # Nicely reload system units when changing configs
  systemd.user.startServices = "sd-switch";

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.11";
}
