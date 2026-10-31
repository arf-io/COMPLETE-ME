# COMPLETE-ME for bash. Sourced from ~/.bashrc by install.sh.
# On TAB for a command with no completion spec: chain to bash-completion's loader first;
# if nothing loads, generate one safely, source it, and ask bash to retry (return 124).
[[ $- == *i* ]] || return 0
command -v complete-me >/dev/null 2>&1 || return 0

_cm_cache="${XDG_CACHE_HOME:-$HOME/.cache}/complete-me/bash"
_cm_prev_default=""
if [[ $(complete -p -D 2>/dev/null) =~ -F\ ([^ ]+) ]] && [[ ${BASH_REMATCH[1]} != _complete_me_loader ]]; then
    _cm_prev_default=${BASH_REMATCH[1]}
fi

_complete_me_loader() {
    local cmd="${1##*/}"
    if [[ -n $_cm_prev_default ]]; then
        "$_cm_prev_default" "$@"
        local rc=$?
        if [[ $rc == 124 ]] || complete -p "$cmd" >/dev/null 2>&1; then
            return 124
        fi
    fi
    if [[ -r $_cm_cache/$cmd ]]; then
        source "$_cm_cache/$cmd" && return 124
    fi
    if [[ $cmd =~ ^[A-Za-z0-9_][A-Za-z0-9_.+-]*$ ]] && type -P "$cmd" >/dev/null 2>&1; then
        complete-me gen --quick --quiet -- "$cmd" 2>/dev/null
        if [[ -r $_cm_cache/$cmd ]]; then
            ( complete-me gen --quiet -- "$cmd" >/dev/null 2>&1 & )
            source "$_cm_cache/$cmd" && return 124
        fi
    fi
    compopt -o default 2>/dev/null
    COMPREPLY=()
    return 0
}
complete -D -F _complete_me_loader

# Aliases made with `complete-me alias … --shell bash` complete like the command they wrap.
declare -gA _cm_alias_words
_complete_me_alias_register() {
    local name=$1
    shift
    _cm_alias_words[$name]="$*"
    complete -F _complete_me_alias_complete "$name"
}
_complete_me_alias_complete() {
    local name=$1 expansion=${_cm_alias_words[$1]}
    local -a words
    read -r -a words <<<"$expansion"
    local cmd=${words[0]}
    if ! complete -p "$cmd" >/dev/null 2>&1; then
        _complete_me_loader "$cmd" >/dev/null 2>&1
    fi
    local fn=""
    [[ $(complete -p "$cmd" 2>/dev/null) =~ -F\ ([^ ]+) ]] && fn=${BASH_REMATCH[1]}
    if [[ -z $fn ]]; then
        COMPREPLY=($(compgen -f -- "${COMP_WORDS[COMP_CWORD]}"))
        return 0
    fi
    local extra=$(( ${#words[@]} - 1 ))
    COMP_WORDS=("${words[@]}" "${COMP_WORDS[@]:1}")
    COMP_CWORD=$(( COMP_CWORD + extra ))
    COMP_POINT=$(( COMP_POINT + ${#expansion} - ${#name} ))
    COMP_LINE="$expansion${COMP_LINE#"$name"}"
    "$fn" "$cmd" "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
}

_cm_aliases="${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/aliases.bash"
[[ -r $_cm_aliases ]] && source "$_cm_aliases"
