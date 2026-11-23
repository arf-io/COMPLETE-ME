# COMPLETE-ME for zsh. Sourced from ~/.zshrc by install.sh (after compinit, or it runs compinit itself).
#
#   TAB on a command with no completion    generates one (safely), loads it, and completes
#   running a new command                  pre-generates its completion in the background
#   TAB in a grep/rg/sed pattern           regex building blocks and recipes, quoted
#   TAB in find                            -type/-size/-mtime… values, operators, -exec {} \; / +
#   TAB after ssh -o                       every ssh_config option, then its values
#   TAB after git clone / curl / cd …      the clipboard, when it holds a git URL / URL / path
#   TAB on arguments                       what you used at this position before (history), grouped
#   alt-r                                  live regex builder (fzf)
#   alt-x                                  explain the command line (find expressions, regexes, flags)
#   alt-j                                  snippet picker
#   ,for ,if ,fn ,sw ,findx … then space   snippet abbreviations (complete-me snippets list --shell zsh)
#   gray suggestions from history          zsh-autosuggestions, if installed (→ accepts); plus
#                                          zsh-syntax-highlighting, if installed
#
# Commands that already have completions keep them; the extras are added alongside.
# Opt out of parts: COMPLETE_ME_NO_KEYS=1, COMPLETE_ME_NO_ABBRS=1, COMPLETE_ME_NO_HISTORY=1,
# COMPLETE_ME_NO_CLIPBOARD=1, COMPLETE_ME_NO_PREGEN=1, COMPLETE_ME_NO_PLUGINS=1. Set them before this file.
[[ -o interactive || -n ${complete_me_force-} ]] || return 0
(( $+commands[complete-me] )) || return 0

if (( ! $+functions[compdef] )); then
    autoload -Uz compinit && compinit -i
fi

typeset -g _cm_zsh_cache="${XDG_CACHE_HOME:-$HOME/.cache}/complete-me/zsh"
typeset -g _cm_prev_default="${_comps[-default-]:-_default}"
[[ $_cm_prev_default == _complete_me_loader ]] && _cm_prev_default=_default

# ---------------------------------------------------------------- generate on miss

_complete_me_loader() {
    local cmd=${words[1]:t} f
    # The command hash can miss an executable created after the first Tab.
    if [[ $cmd == [A-Za-z0-9_][A-Za-z0-9_.+-]# ]] &&
        { (( $+commands[$cmd] )) || whence -p -- "$cmd" >/dev/null; }; then
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

# ---------------------------------------------------------------- extras on every TAB

# A completer that runs before the usual ones. It adds its own groups (regex, find, ssh,
# clipboard, history) and returns 1 so the normal completion still runs, unless the slot is one
# only complete-me understands (a regex pattern, ssh -o values, find values), where it returns 0.
_complete_me_extras() {
    [[ $compstate[context] == command ]] || return 1
    (( CURRENT > 1 )) || return 1
    local -a flags=(--shell zsh --editor-input) rows
    [[ -n ${COMPLETE_ME_NO_CLIPBOARD-} ]] && flags+=(--no-clipboard)
    local out
    if [[ -z ${COMPLETE_ME_NO_HISTORY-} ]]; then
        out=$({ printf '%s\0%s\0' "$LBUFFER" "$RBUFFER"; fc -lnr -500 2>/dev/null; } | command complete-me extras $flags --history 2>/dev/null)
    else
        out=$(printf '%s\0%s\0' "$LBUFFER" "$RBUFFER" | command complete-me extras $flags 2>/dev/null)
    fi
    rows=("${(@f)out}")
    local mode=${rows[1]%%$'\t'*} row g w d
    local in_word=0
    if [[ $mode == *-in-word ]]; then
        in_word=1
        mode=${mode%-in-word}
    fi
    local -A label=(regex 'regex building blocks' find 'find' ssh 'ssh options' live 'live values' clip 'from the clipboard'
        hist 'used here before' histopt 'given to this option before')
    local -a groups=(regex find ssh live clip histopt hist)
    local added=0
    for g in $groups; do
        local -a ws=() ds=()
        for row in ${rows[2,-1]}; do
            [[ $row == $g$'\t'* ]] || continue
            w=${row#*$'\t'}; d=${w#*$'\t'}; w=${w%%$'\t'*}
            # inside an open quote zsh does the quoting: hand it the bare pattern
            [[ -n $compstate[quote] && ( $g == regex || $g == find ) ]] && w=${(Q)w}
            ws+=("$w")
            if [[ -n $d ]]; then printf -v d '%-28s  -- %s' "$w" "$d"; ds+=("$d"); else ds+=("$w"); fi
        done
        (( $#ws )) || continue
        # history keeps its most-recent-first order (-V); regex and find words arrive quoted (-Q)
        local -a opts=(-U -l -X "%B${label[$g]}%b" -d ds)
        if [[ $g == hist* ]]; then opts+=(-V "cm-$g"); else opts+=(-J "cm-$g"); fi
        [[ ( $g == regex || $g == find ) && -z $compstate[quote] ]] && opts+=(-Q)
        if (( in_word )); then
            opts+=(-S '')
        elif [[ $g == ssh || $g == live ]]; then
            if [[ $RBUFFER == [[:space:]]* ]] || (( ${ws[(I)*[=/:]]} )); then
                opts+=(-S '')
            fi
        fi
        compadd "${opts[@]}" -a ws && added=1
    done
    [[ $mode == block || ( $mode == replace && $added == 1 ) ]] && return 0
    return 1
}

# Put the extras first in the completer list, keeping whatever the user configured.
() {
    local -a cs
    zstyle -a ':completion:*' completer cs || cs=(_complete _ignored)
    (( ${cs[(I)_complete_me_extras]} )) || zstyle ':completion:*' completer _complete_me_extras $cs
}

if (( ! $+functions[proxmox] && ! $+commands[proxmox] && ! $+aliases[proxmox] )); then
    function proxmox {
        command complete-me proxmox "$@"
    }
fi

if (( ! $+functions[kube] && ! $+commands[kube] && ! $+aliases[kube] )); then
    function kube {
        command complete-me kube "$@"
    }
fi
if (( ! $+functions[cm-cloud] && ! $+commands[cm-cloud] && ! $+aliases[cm-cloud] )); then
    function cm-cloud {
        command complete-me cloud "$@"
    }
fi

for _cm_sub in alias bash2fish bytes doctor env explain find forget func gen list live path recipe regex show snippets ssh-options; do
    _cm_short="cm-$_cm_sub"
    if (( ! $+functions[$_cm_short] && ! $+commands[$_cm_short] && ! $+aliases[$_cm_short] )); then
        alias "$_cm_short=complete-me $_cm_sub"
    fi
done
unset _cm_sub _cm_short
if (( ! $+functions[path-why] && ! $+commands[path-why] && ! $+aliases[path-why] )); then
    alias 'path-why=complete-me path why'
fi
if (( ! $+functions[zfs-path] && ! $+commands[zfs-path] && ! $+aliases[zfs-path] )); then
    function zfs-path {
        command complete-me zfs-path "$@"
    }
fi

if (( ! $+functions[complete-me] )); then
    function complete-me {
        if [[ $# == 3 && $1 == path && $2 == why && $3 != -* ]]; then
            local kind file=
            kind=$(builtin whence -w -- "$3")
            kind=${kind##*: }
            if [[ $kind == command || $kind == hashed ]]; then
                file=$(builtin whence -- "$3")
            fi
            command complete-me path _why "$3" "$kind" "$file"
            return
        fi
        if [[ ${1-} != doctor ]]; then
            command complete-me "$@"
            return
        fi
        shift
        local target binding=unbound handler=none spec
        local -a completers
        target=$(command complete-me doctor --target-only "$@") || return
        spec=$(bindkey '^I' 2>/dev/null)
        case $spec in
            (*' _complete_me_calc_tab') binding=complete-me ;;
            (*' expand-or-complete'|*' complete-word'|*' menu-complete')
                binding=native
                zstyle -a ':completion:*' completer completers
                (( ${completers[(I)_complete_me_extras]} )) && binding=complete-me ;;
            (*' undefined-key'|'') ;;
            (*) binding=custom ;;
        esac
        if [[ -n $target && $target != *$'\n'* ]]; then
            case ${_comps[$target]-} in
                (_complete_me_*) handler=generated ;;
                ('') [[ $binding == complete-me ]] && handler=loader ;;
                (*) handler=native ;;
            esac
        fi
        command complete-me doctor "$@" --shell=zsh --binding="$binding" --handler="$handler"
    }
fi

# ---------------------------------------------------------------- pre-generate for commands you run

_complete_me_pregen() {
    [[ -n ${COMPLETE_ME_NO_PREGEN-} ]] && return 0
    local cmd=${${(z)1}[1]}
    [[ $cmd == [A-Za-z0-9_][A-Za-z0-9_.+-]# ]] || return 0
    (( $+_comps[$cmd] )) && return 0
    (( $+commands[$cmd] )) || return 0
    [[ -e $_cm_zsh_cache/$cmd.zsh ]] && return 0
    { complete-me gen --quiet -- $cmd &>/dev/null &! }
}
autoload -Uz add-zsh-hook && add-zsh-hook preexec _complete_me_pregen

# ---------------------------------------------------------------- keys: alt-r, alt-x, alt-j, ,snippets

_cm_insert() {   # insert $1 at the cursor; a % in it marks where the cursor goes
    local text=$1
    if [[ $text == *%* ]]; then
        LBUFFER+=${text%%\%*}
        RBUFFER=${text#*\%}$RBUFFER
    else
        LBUFFER+=$text
    fi
}

_complete_me_regex_builder() {
    local q word
    zle -I
    q=$(complete-me regex build --shell zsh --line -- "$LBUFFER" </dev/tty) || { zle reset-prompt; return 0 }
    if [[ -n $q ]]; then
        if [[ -z ${BUFFER//[[:space:]]/} ]]; then
            BUFFER="rg $q "
            CURSOR=$#BUFFER
        else
            word=${LBUFFER##*[[:space:]]}
            LBUFFER="${LBUFFER%$word}$q"
        fi
    fi
    zle reset-prompt
}

_complete_me_explain() {
    [[ -n ${BUFFER//[[:space:]]/} ]] || return 0
    zle -I
    print
    complete-me explain --line "$BUFFER"
    zle reset-prompt
}

_complete_me_snippet() {
    local s
    zle -I
    s=$(complete-me snippets pick --shell zsh </dev/tty)
    [[ -n $s ]] && _cm_insert "$s"
    zle reset-prompt
}

_complete_me_recipe_edit() {
    local expanded
    [[ $BUFFER =~ '^[[:blank:]]*(complete-me[[:blank:]]+recipe|cm-recipe)[[:blank:]]' ]] || return 0
    expanded=$(builtin printf '%s' "$BUFFER" | command complete-me recipe _edit zsh) || return 0
    BUFFER="${BUFFER%%[![:blank:]]*}$expanded"
    CURSOR=$#BUFFER
}

_complete_me_calc_tab() {
    local result
    if [[ $LBUFFER == *[=+*/%^\(,-] ]]; then
        if result=$(printf '%s\0%s\0' "$LBUFFER" "$RBUFFER" | command complete-me _calc-edit "${_cm_calc_answer-}"); then
            typeset -g _cm_calc_answer=${result%%$'\t'*}
            LBUFFER+=${result#*$'\t'}
            return 0
        fi
    fi
    zle "${_cm_calc_native[$KEYMAP]:-expand-or-complete}"
}
if [[ -z ${COMPLETE_ME_NO_CALC-} ]]; then
    typeset -gA _cm_calc_native
    zle -N _complete_me_calc_tab
    for _cm_m in emacs viins main; do
        _cm_key=${${(z)"$(bindkey -M $_cm_m '^I' 2>/dev/null)"}[-1]}
        if [[ -n $_cm_key && $_cm_key != _complete_me_calc_tab && $_cm_key != undefined-key ]]; then
            _cm_calc_native[$_cm_m]=$_cm_key
        fi
    done
    for _cm_m in emacs viins main; do
        if [[ -n ${_cm_calc_native[$_cm_m]} ]]; then
            bindkey -M "$_cm_m" '^I' _complete_me_calc_tab
        fi
    done
    unset _cm_m _cm_key
fi

typeset -g _cm_space_widget=${${(z)"$(bindkey -M main ' ' 2>/dev/null)"}[-1]}
[[ $_cm_space_widget == _complete_me_space ]] || (( $+widgets[$_cm_space_widget] )) || _cm_space_widget=self-insert
[[ $_cm_space_widget == _complete_me_space ]] && _cm_space_widget=self-insert
_complete_me_space() {
    local name body
    if [[ $LBUFFER =~ '(^|[[:space:];|&(])(,[a-z]+)$' ]]; then
        name=$match[2]
        if body=$(complete-me snippets expand --shell zsh $name); then
            LBUFFER=${LBUFFER%$name}
            _cm_insert "$body"
            return 0
        fi
    fi
    zle $_cm_space_widget
}

if [[ -z ${COMPLETE_ME_NO_KEYS-} ]]; then
    setopt COMPLETE_IN_WORD
    zle -N _complete_me_regex_builder
    zle -N _complete_me_explain
    zle -N _complete_me_snippet
    zle -N _complete_me_recipe_edit
    for _cm_m in emacs viins; do
        bindkey -M $_cm_m '^[r' _complete_me_regex_builder
        bindkey -M $_cm_m '^[x' _complete_me_explain
        bindkey -M $_cm_m '^[j' _complete_me_snippet
        bindkey -M $_cm_m '^[R' _complete_me_recipe_edit
    done
    if [[ -z ${COMPLETE_ME_NO_ABBRS-} ]]; then
        zle -N _complete_me_space
        for _cm_m in emacs viins; do bindkey -M $_cm_m ' ' _complete_me_space; done
        # don't expand inside a search
        bindkey -M isearch ' ' self-insert 2>/dev/null
    fi
    unset _cm_m
fi

# ---------------------------------------------------------------- aliases

# Aliases made with `complete-me alias … --shell zsh` complete like the command they wrap.
typeset -g _cm_aliases="${XDG_CONFIG_HOME:-$HOME/.config}/complete-me/aliases.zsh"
[[ -r $_cm_aliases ]] && source $_cm_aliases

# ---------------------------------------------------------------- the fish feel: suggestions + colours

# Gray, fish-style suggestions from history as you type (→ or End accepts), and command colouring.
# Loaded only if installed and not already loaded; syntax highlighting must come last, so it does.
if [[ -z ${COMPLETE_ME_NO_PLUGINS-} ]]; then
    () {
        local base f
        local -a roots=(/usr/share/zsh/plugins /usr/share /usr/local/share /opt/homebrew/share /home/linuxbrew/.linuxbrew/share
            ${HOMEBREW_PREFIX:+$HOMEBREW_PREFIX/share} ${XDG_DATA_HOME:-$HOME/.local/share}/zsh/plugins)
        if (( ! $+functions[_zsh_autosuggest_start] )); then
            for base in $roots; do
                f=$base/zsh-autosuggestions/zsh-autosuggestions.zsh
                if [[ -r $f ]]; then
                    typeset -ga ZSH_AUTOSUGGEST_STRATEGY
                    (( $#ZSH_AUTOSUGGEST_STRATEGY )) || ZSH_AUTOSUGGEST_STRATEGY=(history)
                    source $f
                    break
                fi
            done
        fi
        if (( ! $+functions[_zsh_highlight] )); then
            for base in $roots; do
                f=$base/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh
                [[ -r $f ]] && { source $f; break }
            done
        fi
    }
fi
true
