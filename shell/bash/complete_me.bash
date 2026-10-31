# COMPLETE-ME for bash. Sourced from ~/.bashrc by install.sh.
#
#   TAB on a command with no completions   generates them (safely) and completes
#   running a new command                  pre-generates its completions in the background
#   TAB in a grep/rg/sed pattern           regex building blocks and recipes, quoted
#   TAB in find                            -type/-size/-mtime… values, operators, -exec {} \; / +
#   TAB after ssh -o                       every ssh_config option, then its values
#   TAB after git clone / curl / cd …      the clipboard, when it holds a git URL / URL / path
#   TAB on arguments                       what you used at this position before (history)
#   alt-r                                  live regex builder (fzf)
#   alt-x                                  explain the command line (find expressions, regexes, flags)
#   alt-j                                  snippet picker
#   ,for ,if ,fn ,sw ,findx … then space   snippet abbreviations (complete-me snippets list --shell bash)
#
# Opt out of parts: COMPLETE_ME_NO_KEYS=1 (alt-keys), COMPLETE_ME_NO_ABBRS=1 (,snippets on space),
# COMPLETE_ME_NO_HISTORY=1, COMPLETE_ME_NO_CLIPBOARD=1, COMPLETE_ME_NO_PREGEN=1. Set them before this file.
[[ $- == *i* ]] || [[ -n ${complete_me_force-} ]] || return 0
command -v complete-me >/dev/null 2>&1 || return 0
# bash 4.2+ (associative arrays, mapfile, compopt). macOS ships bash 3.2: `brew install bash`.
if (( BASH_VERSINFO[0] < 4 || BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 2 )); then
    [[ -z ${COMPLETE_ME_QUIET-} ]] && echo "complete-me: needs bash 4.2 or newer (this is $BASH_VERSION; on macOS: brew install bash)" >&2
    return 0
fi

_cm_cache="${XDG_CACHE_HOME:-$HOME/.cache}/complete-me/bash"
_cm_prev_default=""
if [[ $(complete -p -D 2>/dev/null) =~ -F\ ([^ ]+) ]] && [[ ${BASH_REMATCH[1]} != _complete_me_loader ]]; then
    _cm_prev_default=${BASH_REMATCH[1]}
fi
declare -gA _cm_orig 2>/dev/null || declare -A _cm_orig

# ---------------------------------------------------------------- generate on miss

_complete_me_loader() {
    local cmd="${1##*/}"
    if [[ -n $_cm_prev_default ]]; then
        "$_cm_prev_default" "$@"
        local rc=$?
        if [[ $rc == 124 ]] || complete -p "$cmd" >/dev/null 2>&1; then
            _cm_wrap "$cmd"
            return 124
        fi
    fi
    if [[ -r $_cm_cache/$cmd ]]; then
        source "$_cm_cache/$cmd" && _cm_wrap "$cmd" && return 124
    fi
    if [[ $cmd =~ ^[A-Za-z0-9_][A-Za-z0-9_.+-]*$ ]] && type -P "$cmd" >/dev/null 2>&1; then
        complete-me gen --quick --quiet -- "$cmd" 2>/dev/null
        if [[ -r $_cm_cache/$cmd ]]; then
            ( complete-me gen --quiet -- "$cmd" >/dev/null 2>&1 & )
            source "$_cm_cache/$cmd" && _cm_wrap "$cmd" && return 124
        fi
    fi
    # Nothing knows this command: files, plus history and clipboard.
    if [[ -n $cmd ]] && ! complete -p "$cmd" >/dev/null 2>&1; then
        _cm_orig[$cmd]=""
        complete -o bashdefault -o default -F _cm_aug "$cmd" 2>/dev/null && return 124
    fi
    compopt -o default 2>/dev/null
    COMPREPLY=()
    return 0
}
complete -D -F _complete_me_loader

# ---------------------------------------------------------------- extras on every TAB

# Re-register CMD's completion so the extras run after its own completion function.
_cm_wrap() {
    local cmd=$1 spec fn
    spec=$(complete -p "$cmd" 2>/dev/null) || return 0
    [[ $spec =~ -F\ ([^ ]+) ]] || return 0
    fn=${BASH_REMATCH[1]}
    [[ $fn == _cm_aug ]] && return 0
    _cm_orig[$cmd]=$fn
    eval "${spec/-F $fn/-F _cm_aug}" 2>/dev/null || true
    return 0
}

_cm_aug() {
    local cmd=${1##*/} orig=${_cm_orig[${1##*/}]-} rc=0
    if [[ -n $orig ]]; then
        "$orig" "$@"
        rc=$?
        [[ $rc == 124 ]] && { _cm_wrap "$cmd"; return 124; }
    else
        COMPREPLY=()
    fi
    local left=${COMP_LINE:0:COMP_POINT} out
    local -a flags=(--shell bash --breaks "$COMP_WORDBREAKS")
    [[ -n ${COMPLETE_ME_NO_CLIPBOARD-} ]] && flags+=(--no-clipboard)
    # History ranks arguments for commands complete-me knows (or nothing knows); a hand-written
    # completion like git's already knows better than history what fits.
    if [[ -z ${COMPLETE_ME_NO_HISTORY-} && ( -z $orig || $orig == _complete_me_* || $orig == _comp_complete_minimal || $orig == _minimal ) ]]; then
        out=$(fc -lnr -500 2>/dev/null | complete-me extras "${flags[@]}" --history -- "$left" 2>/dev/null)
    else
        out=$(complete-me extras "${flags[@]}" -- "$left" 2>/dev/null)
    fi
    local head mode line word
    local -a rows=() add=() more=()
    mapfile -t rows <<<"$out"
    head=${rows[0]-}
    mode=${head%%$'\t'*}
    # Rows arrive rewritten to replace exactly what readline replaces (after an open quote, or
    # after the last COMP_WORDBREAKS character): see bash_word in src/extras.rs.
    local cur=${COMP_WORDS[COMP_CWORD]-} recent=0
    for line in "${rows[@]:1}"; do
        [[ $line == hist* ]] && recent=1
        word=${line#*$'\t'}
        word=${word%%$'\t'*}
        [[ -n $word ]] && add+=("$word")
    done
    if [[ $mode == replace ]]; then
        COMPREPLY=("${add[@]}")
        compopt +o default +o bashdefault +o filenames 2>/dev/null
    else
        COMPREPLY+=("${add[@]}")
        if [[ ( -z $orig || $orig == _comp_complete_minimal || $orig == _minimal ) && ${cur} != -* ]]; then
            compopt -o filenames 2>/dev/null
            if [[ $cmd == cd || $cmd == pushd ]]; then
                mapfile -t more < <(compgen -d -- "$cur" | LC_ALL=C sort)
            else
                mapfile -t more < <(compgen -f -- "$cur" | LC_ALL=C sort)
            fi
            COMPREPLY+=("${more[@]}")
        fi
    fi
    # keep history most-recent-first (bash 4.4+)
    (( recent )) && compopt -o nosort 2>/dev/null
    if (( ${#COMPREPLY[@]} == 1 )) && [[ ${COMPREPLY[0]} == *[=/] ]]; then
        compopt -o nospace 2>/dev/null
    fi
    return 0
}

# Commands whose extras matter even before anything has completed them once.
_cm_extra_cmds=(grep egrep rgrep zgrep ugrep rg sed gsed find ssh scp sftp git cd pushd curl wget xh aria2c yt-dlp
    mpv xdg-open open links lynx w3m cat less more bat vim nvim vi nano hx code cp mv ln tar unzip file stat du ls head
    tail diff rsync chmod chown wc sha256sum md5sum)
for _cm_c in "${_cm_extra_cmds[@]}"; do
    if complete -p "$_cm_c" >/dev/null 2>&1; then
        _cm_wrap "$_cm_c"
    elif [[ -z $_cm_prev_default ]] && type -P "$_cm_c" >/dev/null 2>&1 || [[ $_cm_c == cd || $_cm_c == pushd ]]; then
        # no bash-completion to lazy-load from: files/dirs plus the extras
        _cm_orig[$_cm_c]=""
        if [[ $_cm_c == cd || $_cm_c == pushd ]]; then
            complete -o nospace -o dirnames -F _cm_aug "$_cm_c"
        else
            complete -o bashdefault -o default -F _cm_aug "$_cm_c"
        fi
    fi
done
unset _cm_c

# ---------------------------------------------------------------- pre-generate for commands you run

_cm_last_hist=""
_complete_me_pregen() {
    local h n cmd
    h=$(HISTTIMEFORMAT= history 1 2>/dev/null) || return 0
    n=${h%%[^ 0-9]*}
    [[ $n == "$_cm_last_hist" ]] && return 0
    _cm_last_hist=$n
    read -r _ cmd _ <<<"$h"
    [[ $cmd =~ ^[A-Za-z0-9_][A-Za-z0-9_.+-]*$ ]] || return 0
    [[ -e $_cm_cache/$cmd ]] && return 0
    complete -p "$cmd" >/dev/null 2>&1 && return 0
    type -P "$cmd" >/dev/null 2>&1 || return 0
    local d
    for d in "${BASH_COMPLETION_USER_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion}/completions" \
        /usr/share/bash-completion/completions /usr/local/share/bash-completion/completions \
        /opt/homebrew/share/bash-completion/completions /etc/bash_completion.d; do
        [[ -e $d/$cmd || -e $d/$cmd.bash || -e $d/_$cmd ]] && return 0
    done
    ( complete-me gen --quiet -- "$cmd" >/dev/null 2>&1 & )
}
if [[ -z ${COMPLETE_ME_NO_PREGEN-} && ${PROMPT_COMMAND-} != *_complete_me_pregen* ]]; then
    if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then
        PROMPT_COMMAND+=(_complete_me_pregen)
    else
        PROMPT_COMMAND="_complete_me_pregen${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
    fi
fi

# ---------------------------------------------------------------- keys: alt-r, alt-x, alt-j, ,snippets

# Byte offsets: READLINE_POINT counts bytes, so slice in the C locale.
_cm_insert() {   # insert $1 at the cursor; a % in it marks where the cursor goes
    local LC_ALL=C text=$1 before after
    before=${text%%%*}
    if [[ $text == *%* ]]; then after=${text#*%}; else after=""; fi
    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT}$before$after${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$(( READLINE_POINT + ${#before} ))
}

_complete_me_regex_builder() {
    local LC_ALL=C left=${READLINE_LINE:0:READLINE_POINT} word q
    q=$(LC_ALL= complete-me regex build --shell bash --line -- "$left") || return 0
    [[ -n $q ]] || return 0
    if [[ -z ${READLINE_LINE//[[:space:]]/} ]]; then
        READLINE_LINE="rg $q "
        READLINE_POINT=${#READLINE_LINE}
        return 0
    fi
    word=${left##*[[:space:]]}
    READLINE_LINE="${left:0:${#left}-${#word}}$q${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$(( ${#left} - ${#word} + ${#q} ))
}

_complete_me_explain() {
    [[ -n ${READLINE_LINE//[[:space:]]/} ]] || return 0
    printf '\n'
    complete-me explain --line "$READLINE_LINE"
}

_complete_me_snippet() {
    local s
    s=$(complete-me snippets pick --shell bash) || return 0
    [[ -n $s ]] && _cm_insert "$s"
}

_complete_me_space() {
    local LC_ALL=C left=${READLINE_LINE:0:READLINE_POINT} name body
    if [[ $left =~ (^|[[:space:]\;\|\&\(])(,[a-z]+)$ ]]; then
        name=${BASH_REMATCH[2]}
        if body=$(complete-me snippets expand --shell bash "$name"); then
            READLINE_LINE="${left:0:${#left}-${#name}}${READLINE_LINE:READLINE_POINT}"
            READLINE_POINT=$(( ${#left} - ${#name} ))
            _cm_insert "$body"
            return 0
        fi
    fi
    _cm_insert " "
}

if [[ -z ${COMPLETE_ME_NO_KEYS-} ]]; then
    for _cm_m in emacs vi-insert; do
        bind -m $_cm_m -x '"\er": _complete_me_regex_builder' 2>/dev/null
        bind -m $_cm_m -x '"\ex": _complete_me_explain' 2>/dev/null
        bind -m $_cm_m -x '"\ej": _complete_me_snippet' 2>/dev/null
        # ,snippets on space only where space is a plain space: a magic-space binding is left alone
        if [[ -z ${COMPLETE_ME_NO_ABBRS-} ]] && [[ $(bind -m $_cm_m -p 2>/dev/null | grep -F '" ":') != *magic-space* ]]; then
            bind -m $_cm_m -x '" ": _complete_me_space' 2>/dev/null
        fi
    done
    unset _cm_m
fi

# ---------------------------------------------------------------- aliases

# Aliases made with `complete-me alias … --shell bash` complete like the command they wrap.
declare -gA _cm_alias_words 2>/dev/null || declare -A _cm_alias_words
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
        mapfile -t COMPREPLY < <(compgen -f -- "${COMP_WORDS[COMP_CWORD]}")
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
true
