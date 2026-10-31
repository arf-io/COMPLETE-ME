# COMPLETE-ME for zsh. Sourced from ~/.zshrc by install.sh (after compinit, or it runs compinit itself).
# On TAB for a command with no completion: generate one safely, load it, and complete. Commands that
# already have completions are untouched; anything else falls back to whatever handled it before.
[[ -o interactive ]] || return 0
(( $+commands[complete-me] )) || return 0

if (( ! $+functions[compdef] )); then
    autoload -Uz compinit && compinit -i
fi

typeset -g _cm_zsh_cache="${XDG_CACHE_HOME:-$HOME/.cache}/complete-me/zsh"
typeset -g _cm_prev_default="${_comps[-default-]:-_default}"
[[ $_cm_prev_default == _complete_me_loader ]] && _cm_prev_default=_default

_complete_me_loader() {
    local cmd=${words[1]:t} f
    if [[ $cmd == [A-Za-z0-9_][A-Za-z0-9_.+-]# ]] && (( $+commands[$cmd] )); then
        f=$_cm_zsh_cache/$cmd.zsh
        if [[ ! -r $f ]]; then
            complete-me gen --quick --quiet -- $cmd 2>/dev/null
            [[ -r $f ]] && { complete-me gen --quiet -- $cmd &>/dev/null &! }
        fi
        if [[ -r $f ]]; then
            source $f
            local fn=_complete_me_${cmd//[^A-Za-z0-9_]/_}
            if (( $+functions[$fn] )); then
                $fn "$@"
                return
            fi
        fi
    fi
    $_cm_prev_default "$@"
}
compdef _complete_me_loader -default-

# Aliases made with `complete-me alias … --shell zsh` complete like the command they wrap.
typeset -g _cm_aliases="${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/aliases.zsh"
[[ -r $_cm_aliases ]] && source $_cm_aliases
