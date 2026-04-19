FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash zsh git git-lfs ca-certificates curl sudo xz-utils \
 && rm -rf /var/lib/apt/lists/*


ENV USER="ubuntu"
ENV HOME="/home/${USER}"

# Create non-root devcontainer user
RUN echo "${USER} ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/${USER} \
    && chmod 0440 /etc/sudoers.d/${USER}

USER ${USER}

# Install Nix files, but no init integration.
# The Nix daemon won't work as systemd is disabled, thus we
# install Nix in single user more
RUN /bin/bash -c "sh <(curl -L https://nixos.org/nix/install) --no-daemon"

# Unfortunately docker doesn't load the user's env variables automatically.
# Similar to:
# https://github.com/NixOS/nix/blob/0eb9946e1d3621cfc2fcffc9378dba334b25fb26/docker.nix#L259-L264
ENV PATH="${PATH}:/home/${USER}/.nix-profile/bin"

# As we have a local Nix installation, we need to edit the local config
RUN mkdir -p ${HOME}/.config/nix \
    && cat >> "${HOME}/.config/nix/nix.conf" <<'EOF'
experimental-features = nix-command flakes
sandbox = false
max-jobs = auto
EOF

# Install packages
RUN nix profile add \
    nixpkgs#awscli2 \
    nixpkgs#direnv \
    nixpkgs#neovim \
    nixpkgs#starship \
    nixpkgs#vim

# Configure shell
RUN cat >> ${HOME}/.bashrc <<'EOF'
eval "$(direnv hook bash)"
eval "$(starship init bash)"
EOF
RUN cat >> ${HOME}/.zshrc <<'EOF'
eval "$(direnv hook zsh)"
eval "$(starship init zsh)"
EOF
COPY ./starship.toml ${HOME}/.config/starship.toml

WORKDIR /home/${USER}

CMD ["/bin/bash"]
