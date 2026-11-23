# Print what bash would offer: bash_capture.bash RUNTIME 'line' [WORD…]
# Loads bash-completion when it is installed (as most bash users have), then the runtime, then
# runs the completion for the line the way readline would (retrying after a 124 "spec loaded").
# WORDs, if given, are COMP_WORDS as bash would split them (for = and : word breaks).
rt=$1 line=$2
shift 2
set -o history
[[ -r ${HISTFILE-} ]] && history -r "$HISTFILE"
if [[ -z ${CM_NO_BASH_COMPLETION-} ]]; then
    for f in /usr/share/bash-completion/bash_completion /usr/local/share/bash-completion/bash_completion \
        /opt/homebrew/share/bash-completion/bash_completion /home/linuxbrew/.linuxbrew/share/bash-completion/bash_completion; do
        [[ -r $f ]] && { source "$f"; break; }
    done
fi
complete_me_force=1 COMPLETE_ME_NO_PREGEN=1 COMPLETE_ME_NO_KEYS=1 source "$rt"
COMP_LINE=$line
COMP_POINT=${#line}
if (( $# )); then
    COMP_WORDS=("$@")
else
    read -r -a COMP_WORDS <<<"$line"
    [[ $line == *' ' ]] && COMP_WORDS+=("")
fi
COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))
cmd=${COMP_WORDS[0]}
for _ in 1 2 3; do
    spec=$(complete -p "$cmd" 2>/dev/null || complete -p -D)
    [[ $spec =~ -F\ ([^ ]+) ]] && fn=${BASH_REMATCH[1]}
    COMPREPLY=()
    "$fn" "$cmd" "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
    [[ $? == 124 ]] || break
done
printf '%s\n' "${COMPREPLY[@]}"
