# Created by complete-me: aliases and functions. Sourced from your rc file.
gsw() { git switch "$@"; }  # cm:gsw
_complete_me_alias_register gsw git switch  # cm:gsw
logs() { journalctl -u "$1" -f "${@:2}"; }  # cm:logs
_complete_me_alias_register logs journalctl -u  # cm:logs
ls() { command ls -la "$@"; }  # cm:ls
_complete_me_alias_register ls ls -la  # cm:ls
