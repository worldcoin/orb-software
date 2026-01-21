# home-manager configuration for all HILs.
{ pkgs, lib, ... }:
let
  xdg = ../xdg;
  packages = ../packages;
in {
  home = {
    username = "worldcoin";
    homeDirectory = "/home/worldcoin";
    packages = import "${packages}/hil.nix" { inherit pkgs; };
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
    package = pkgs.unstable.zellij;
  };
  programs.direnv = {
    enable = true;
    enableBashIntegration = true; # see note on other shells below
    enableZshIntegration = true;
    nix-direnv.enable = true;
  };
  programs.tmux = {
    enable = true;
    terminal = "screen-256color";
    historyLimit = 50000;
    focusEvents = true;
    clock24 = true;
    keyMode = "vi";
    mouse = true;

    plugins = with pkgs.tmuxPlugins; [
      {
        plugin = yank;
        extraConfig = ''
          set -g @yank_action 'copy-pipe'
          set -g @yank_selection_mouse 'clipboard'
          set -g @yank_with_mouse on
        '';
      }
      resurrect
    ];

    extraConfig = ''
      # general Settings
      set -ga terminal-features "xterm-256color:RGB"
      set -g set-clipboard on
    '';
  };

  xdg.enable = true;
  xdg.configFile = {
    "nvim" = {
      source = pkgs.fetchFromGitHub {
        owner = "thebutlah";
        repo = "init.lua";
        rev = "2c3c458325dbe9ee46793d92cea4c541e3c2babd";
        hash = "sha256-MOtj/lCK36oTmnY2HxCLSW6LnzZ5jheaK34EUlKC2qs=";
      };
    };
    "zellij/config.kdl" = { source = "${xdg}/zellij.kdl"; };
    "atuin/config.toml" = { source = "${xdg}/atuin.toml"; };
  };

  fonts.fontconfig.enable = true;

  # Nicely reload system units when changing configs
  systemd.user.startServices = "sd-switch";

  # https://nixos.wiki/wiki/FAQ/When_do_I_update_stateVersion
  home.stateVersion = "23.11";
}
