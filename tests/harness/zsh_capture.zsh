# Print what zsh would offer for a command line: zsh_capture.zsh RUNTIME 'line'
# Runs an interactive zsh in a pty with the runtime loaded, overrides compadd to print matches
# (group<TAB>match), presses TAB, and collects the output. The markers are split with '' so the
# terminal's echo of the command never looks like its output.
zmodload zsh/zpty || exit 2
rt=$1 line=$2
zpty z zsh -f -i
zpty -w z "PS1='' PROMPT_EOL_MARK=''; unsetopt zle_bracketed_paste 2>/dev/null"
zpty -w z "autoload -Uz compinit && compinit -i -D; [[ -r \$HISTFILE ]] && fc -R \$HISTFILE"
zpty -w z "COMPLETE_ME_NO_PLUGINS=1 source ${(q)rt}"
zpty -w z 'compadd() {
    if [[ ${@[1,(i)(-|--)]} == *-(O|A|D)\ * ]]; then builtin compadd "$@"; return; fi
    local -a __h; local __g=${${@[(r)cm-*]}:-native}
    builtin compadd -A __h "$@"
    (( $#__h )) && print -rl -- "@@${__g}"$'\''\t'\''${^__h}
    builtin compadd "$@"
}'
zpty -w z "zstyle ':completion:*' list-prompt ''; zstyle ':completion:*' menu no; print -r -- '@@RE''ADY'"
local out
while zpty -r z out; do [[ $out == *@@READY* ]] && break; done
zpty -w -n z "$line"$'\t'
zpty -w z $'\x15print -r -- @@DO\'\'NE'
while zpty -r z out; do
    [[ $out == *@@DONE* ]] && break
    [[ $out == *@@*$'\t'* ]] && print -r -- "${${out#*@@}%%$'\r'*}"
done
zpty -d z
