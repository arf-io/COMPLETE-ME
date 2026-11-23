set -gx NIX_PATH 'nixpkgs=/nix/var/nix/profiles/per-user/root/channels/nixos:nixos-config=/etc/nixos/configuration.nix'
set -gx NIX_PATH "$HOME"'/.nix-defexpr/channels'(test -n "$NIX_PATH" && echo ':'"$NIX_PATH" || echo)
