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
    local cmd="${1##*/}" inherited=""
    if [[ -n $_cm_prev_default ]]; then
        "$_cm_prev_default" "$@"
        local rc=$?
        inherited=$(complete -p "$cmd" 2>/dev/null) || inherited=""
        if [[ $rc == 124 || -n $inherited ]]; then
            case "$inherited" in
                # bash-completion registers a generic filename handler for unknown commands.
                *' -F _comp_complete_minimal '*|*' -F _minimal '*) ;;
                *) _cm_wrap "$cmd"; return 124 ;;
            esac
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
    if [[ -n $inherited ]]; then
        _cm_wrap "$cmd"
        return 124
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
    local left=${COMP_LINE:0:COMP_POINT} cur=${COMP_WORDS[COMP_CWORD]-} out recent=0
    local right=${COMP_LINE:COMP_POINT}
    local -a flags=(--shell bash --breaks "$COMP_WORDBREAKS" --editor-input)
    [[ -n ${COMPLETE_ME_NO_CLIPBOARD-} ]] && flags+=(--no-clipboard)
    # History ranks arguments for commands complete-me knows (or nothing knows); a hand-written
    # completion like git's already knows better than history what fits.
    if [[ -z ${COMPLETE_ME_NO_HISTORY-} && ( -z $orig || $orig == _complete_me_* || $orig == _comp_complete_minimal || $orig == _minimal ) ]]; then
        out=$({ printf '%s\0%s\0' "$left" "$right"; fc -lnr -500 2>/dev/null; } | command complete-me extras "${flags[@]}" --history 2>/dev/null)
    else
        out=$(printf '%s\0%s\0' "$left" "$right" | command complete-me extras "${flags[@]}" 2>/dev/null)
    fi
    local head mode line word
    local -a rows=() add=() more=()
    mapfile -t rows <<<"$out"
    head=${rows[0]-}
    mode=${head%%$'\t'*}
    local in_word=0
    if [[ $mode == *-in-word ]]; then
        in_word=1
        mode=${mode%-in-word}
    fi
    COMPREPLY=()
    if [[ $mode != replace && $mode != block && -n $orig ]]; then
        "$orig" "$@"
        rc=$?
        [[ $rc == 124 ]] && _cm_wrap "$cmd"
    fi
    if (( in_word )); then
        if [[ $mode == add && ${#COMPREPLY[@]} -gt 0 ]]; then
            local edits
            edits=$({ printf '%s\0%s\0' "$left" "$right"; printf '%s\n' "${COMPREPLY[@]}"; } | command complete-me _bash-edit --editor-input --breaks "$COMP_WORDBREAKS")
            COMPREPLY=()
            [[ -z $edits ]] || mapfile -t COMPREPLY <<<"$edits"
        fi
        compopt +o default +o bashdefault +o filenames -o nospace 2>/dev/null
    fi
    # Rows arrive rewritten to replace exactly what readline replaces (after an open quote, or
    # after the last COMP_WORDBREAKS character): see bash_word in src/extras.rs.
    for line in "${rows[@]:1}"; do
        [[ $line == hist* ]] && recent=1
        word=${line#*$'\t'}
        word=${word%%$'\t'*}
        [[ -n $word ]] && add+=("$word")
    done
    # Native functions can request a retry after loading nested completions.
    # Extras for this slot are already ready to use.
    if [[ $mode == add ]] && (( rc == 124 && ${#add[@]} == 0 )); then
        return 124
    fi
    if [[ $mode == replace || $mode == block ]]; then
        COMPREPLY=("${add[@]}")
        compopt +o default +o bashdefault +o filenames 2>/dev/null
    else
        if (( ! in_word && ${#COMPREPLY[@]} == 0 )) && [[ ( -z $orig || $orig == _comp_complete_minimal || $orig == _minimal ) && ${cur} != -* ]]; then
            compopt -o filenames 2>/dev/null
            if [[ $cmd == cd || $cmd == pushd ]]; then
                mapfile -t more < <(compgen -d -- "$cur" | LC_ALL=C sort)
            else
                mapfile -t more < <(compgen -f -- "$cur" | LC_ALL=C sort)
            fi
        fi
        if (( recent )); then
            COMPREPLY=("${add[@]}" "${COMPREPLY[@]}" "${more[@]}")
        else
            COMPREPLY+=("${add[@]}" "${more[@]}")
        fi
    fi
    # keep history most-recent-first (bash 4.4+)
    (( recent )) && compopt -o nosort 2>/dev/null
    if (( ${#COMPREPLY[@]} == 1 )) && [[ ${COMPREPLY[0]} == *[=/:] ]]; then
        compopt -o nospace 2>/dev/null
    fi
    return 0
}

# Commands whose extras matter even before anything has completed them once.
_cm_extra_cmds=(grep egrep rgrep zgrep ugrep rg sed gsed find ssh scp sftp git tmux docker docker-compose podman gh just task zerotier-cli pct qm pvesm vzdump qmrestore zfs zpool aws vultr-cli hf huggingface-cli codex claude gemini agy hermes fx cline opencode aider cursor-agent copilot droid qwen goose amp pi dd hexdump tcpdump tshark wireshark ip route nmap kubectl systemctl journalctl npm pnpm yarn bun brew cd pushd curl wget xh aria2c yt-dlp
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
if complete -p complete-me >/dev/null 2>&1; then
    _cm_wrap complete-me
else
    complete -F _cm_aug complete-me
fi

# Respect an existing command with this name.
if ! builtin type -t proxmox >/dev/null; then
    function proxmox {
        command complete-me proxmox "$@"
    }
    complete -F _cm_aug proxmox
fi

if ! builtin type -t kube >/dev/null; then
    function kube {
        command complete-me kube "$@"
    }
    complete -F _cm_aug kube
fi
if ! builtin type -t cm-cloud >/dev/null; then
    function cm-cloud {
        command complete-me cloud "$@"
    }
    complete -F _cm_aug cm-cloud
fi

for _cm_sub in alias bash2fish bytes doctor env explain find forget func gen list live path recipe regex show snippets ssh-options; do
    if ! builtin type -t "cm-$_cm_sub" >/dev/null; then
        alias "cm-$_cm_sub=complete-me $_cm_sub"
        [[ $_cm_sub != recipe ]] || complete -F _cm_aug cm-recipe
    fi
done
unset _cm_sub
if ! builtin type -t path-why >/dev/null; then
    alias 'path-why=complete-me path why'
fi
if ! builtin type -t zfs-path >/dev/null; then
    function zfs-path {
        command complete-me zfs-path "$@"
    }
    complete -F _cm_aug zfs-path
fi

# The child binary cannot inspect Readline or completion bindings on its own.
if ! declare -F complete-me >/dev/null; then
    function complete-me {
        if [[ $# == 3 && $1 == path && $2 == why && $3 != -* ]]; then
            local kind file=
            kind=$(builtin type -t -- "$3")
            [[ $kind != file ]] || file=$(builtin type -p -- "$3")
            command complete-me path _why "$3" "$kind" "$file"
            return
        fi
        if [[ ${1-} != doctor ]]; then
            command complete-me "$@"
            return
        fi
        shift
        local target binding=unbound handler=none spec orig
        target=$(command complete-me doctor --target-only "$@") || return
        spec=$(bind -p 2>/dev/null)
        if [[ $(bind -s 2>/dev/null) == *'"\C-i": "\e[27;5;9~\e[27;6;9~"'* ]]; then
            binding=complete-me
        elif [[ $spec == *'"\C-i": complete'* || $spec == *'"\C-i": menu-complete'* ]]; then
            binding=native
            [[ $(complete -p -D 2>/dev/null) == *'_complete_me_loader'* ]] && binding=complete-me
        elif [[ $spec == *'"\C-i":'* || $(bind -X 2>/dev/null) == *'"\C-i":'* ]]; then
            binding=custom
        fi
        if [[ -n $target && $target != *$'\n'* ]]; then
            spec=$(complete -p "$target" 2>/dev/null)
            orig=${_cm_orig[$target]-}
            if [[ $spec == *'-F _complete_me_'* || $orig == _complete_me_* ]]; then
                handler=generated
            elif [[ $spec == *'-F _cm_aug'* ]]; then
                if [[ -n $orig ]]; then handler=native; else handler=live-adapter; fi
            elif [[ -n $spec ]]; then
                handler=native
            elif [[ $binding == complete-me ]]; then
                handler=loader
            fi
        fi
        command complete-me doctor "$@" --shell=bash --binding="$binding" --handler="$handler"
    }
fi

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

_complete_me_recipe_edit() {
    local LC_ALL=C expanded
    [[ $READLINE_LINE =~ ^[[:blank:]]*(complete-me[[:blank:]]+recipe|cm-recipe)[[:blank:]] ]] || return 0
    expanded=$(builtin printf '%s' "$READLINE_LINE" | command complete-me recipe _edit bash) || return 0
    READLINE_LINE="${READLINE_LINE%%[![:blank:]]*}$expanded"
    READLINE_POINT=${#READLINE_LINE}
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

# Readline runs a two-key macro. The first widget chooses whether the second
# key should run the user's existing completion function or do nothing.
_complete_me_calc_tab() {
    local LC_ALL=C map=$1 left=${READLINE_LINE:0:READLINE_POINT} right=${READLINE_LINE:READLINE_POINT} reply insertion
    local native=${_cm_calc_native[$map]} position="$map:$HISTCMD:$READLINE_POINT:$READLINE_LINE"
    if [[ $left =~ ^[[:space:][:alnum:].+*/%^\(\),!=_-]+$ && $left == *[=+*/%^\(,-] ]]; then
        if reply=$(builtin printf '%s\0%s\0' "$left" "$right" | command complete-me _calc-edit "${_cm_calc_answer-}"); then
            insertion=${reply#*$'\t'}
            _cm_calc_answer=${reply%%$'\t'*}
            READLINE_LINE="$left$insertion$right"
            READLINE_POINT=$(( ${#left} + ${#insertion} ))
            unset _cm_calc_prior_tab
            bind -m "$map" '"\e[27;6;9~": ""'
            return
        fi
    fi
    # The calculator widget interrupts Readline's own consecutive-Tab detection.
    if [[ $native == complete && ${_cm_calc_prior_tab-} == "$position" ]]; then
        native=possible-completions
    fi
    _cm_calc_prior_tab=$position
    bind -m "$map" "\"\\e[27;6;9~\": $native"
}
if [[ -z ${COMPLETE_ME_NO_CALC-} ]]; then
    declare -gA _cm_calc_native
    for _cm_m in emacs vi-insert; do
        _cm_key=$(bind -m "$_cm_m" -p 2>/dev/null | grep -F '"\C-i": ')
        _cm_key=${_cm_key##*: }
        case $_cm_key in
            complete|menu-complete|menu-complete-backward|possible-completions)
                _cm_calc_native[$_cm_m]=$_cm_key
                bind -m "$_cm_m" -x "\"\\e[27;5;9~\": _complete_me_calc_tab $_cm_m"
                bind -m "$_cm_m" '"\C-i": "\e[27;5;9~\e[27;6;9~"'
                ;;
        esac
    done
    unset _cm_m _cm_key
fi

if [[ -z ${COMPLETE_ME_NO_KEYS-} ]]; then
    for _cm_m in emacs vi-insert; do
        bind -m $_cm_m -x '"\er": _complete_me_regex_builder' 2>/dev/null
        bind -m $_cm_m -x '"\ex": _complete_me_explain' 2>/dev/null
        bind -m $_cm_m -x '"\ej": _complete_me_snippet' 2>/dev/null
        bind -m $_cm_m -x '"\eR": _complete_me_recipe_edit' 2>/dev/null
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
