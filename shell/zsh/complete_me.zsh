# COMPLETE-ME for zsh. Sourced from ~/.zshrc by install.sh.
# Generated zsh completions are not implemented yet; this loads aliases/functions made with
# `complete-me alias … --shell zsh`, which complete like the command they wrap via compdef.
[[ -o interactive ]] || return 0
_cm_aliases="${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/aliases.zsh"
[[ -r $_cm_aliases ]] && source "$_cm_aliases"
