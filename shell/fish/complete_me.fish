# COMPLETE-ME for fish. Sourced from ~/.config/fish/conf.d/complete_me.fish.
#
#   TAB on a command with no completions   generates them (safely) and completes
#   running a new command                  pre-generates its completions in the background
#   alt-r                                  live regex builder (fzf) for grep / rg / sed / find / string
#   alt-x                                  explain the command line (find expressions, regexes)
#   alt-j                                  snippet picker: fish syntax + bash→fish phrasebook
#   ,for ,if ,fn ,sw ,findx …              snippet abbreviations (complete-me snippets list)

status is-interactive; or set -q complete_me_force; or return
command -q complete-me; or return

set -l __cache_home ~/.cache
set -q XDG_CACHE_HOME; and set __cache_home $XDG_CACHE_HOME
set -g __complete_me_cache $__cache_home/complete-me
set -g __complete_me_gen $__complete_me_cache/fish
test -d $__complete_me_gen; or mkdir -p $__complete_me_gen

# Generated completions rank after every real completion directory,
# but before fish's own man-page guesses (fish_update_completions).
if not contains -- $__complete_me_gen $fish_complete_path
    set -l idx 0
    set -l j 0
    for d in $fish_complete_path   # builtins only: shell startup must not fork
        set j (math $j + 1)
        if string match -q -- '*/fish/generated_completions' $d
            set idx $j
            break
        end
    end
    if test $idx -gt 1
        set -g fish_complete_path $fish_complete_path[1..(math $idx - 1)] $__complete_me_gen $fish_complete_path[$idx..]
    else if test $idx -eq 1
        set -g fish_complete_path $__complete_me_gen $fish_complete_path
    else
        set -ga fish_complete_path $__complete_me_gen
    end
end

# ---------------------------------------------------------------- subcommand paths

function __complete_me_at -d 'complete-me: is the command line at this subcommand path?'
    set -l id $argv[1]
    set -l want "$argv[2..]"
    set -l line (commandline -pc)
    if test "$__cm_at_key" != "$id|$line"
        set -l tv __cm_tree_$id
        set -l av __cm_alias_$id
        set -l ov __cm_argopts_$id
        set -l path
        set -l skip 0
        for t in (commandline -xpc)[2..]
            if test $skip = 1
                set skip 0
                continue
            end
            if string match -q -- '-*' $t
                contains -- $t $$ov; and set skip 1
                continue
            end
            set -l cand (string join ' ' -- $path $t)
            if contains -- "$cand" $$tv
                set -a path $t
                continue
            end
            set -l al (string match -- "$cand=*" $$av)
            if set -q al[1]
                set -a path (string split -m1 = -- $al[1])[2]
                continue
            end
            break
        end
        set -g __cm_at_key "$id|$line"
        set -g __cm_at_val "$path"
    end
    test "$__cm_at_val" = "$want"
end

# ---------------------------------------------------------------- history-ranked arguments

function __complete_me_hist_args -d 'complete-me: arguments used before at this position, most recent first'
    set -l prefix (string join ' ' -- $argv)
    set -l skip (count $argv)
    set -l seen
    for line in (history search --prefix --max 400 -- "$prefix ")
        set -l toks (commandline -I $line --tokens-raw 2>/dev/null | string unescape 2>/dev/null)
        for t in $toks[(math $skip + 1)..]
            string match -q -- '-*' $t; and continue
            contains -- $t $seen; and continue
            set -a seen $t
        end
        test (count $seen) -ge 25; and break
    end
    __complete_me_clip path
    for t in $seen
        if test -d "$t"
            printf '%s/\tdirectory you used here before\n' (string trim -r -c / -- $t)
        else if test -e "$t"
            printf '%s\tfile you used here before\n' $t
        else
            printf '%s\tused here before\n' $t
        end
    end
end

function __complete_me_hist_optvals -d 'complete-me: values given to this option before'
    set -l cmd $argv[1]
    set -l flag $argv[2]
    set -l seen
    for line in (history search --prefix --max 300 -- "$cmd ")
        set -l toks (commandline -I $line --tokens-raw 2>/dev/null | string unescape 2>/dev/null)
        for i in (seq 2 (count $toks))
            set -l v
            if test "$toks[$i]" = "$flag"; and set -q toks[(math $i + 1)]
                set v $toks[(math $i + 1)]
            else if string match -q -- "$flag=*" $toks[$i]
                set v (string replace -- "$flag=" '' $toks[$i])
            else
                continue
            end
            contains -- $v $seen; or set -a seen $v
        end
        test (count $seen) -ge 20; and break
    end
    set -q seen[1]; and printf '%s\tused before\n' $seen
end

# ---------------------------------------------------------------- clipboard

# The clipboard is offered as a completion only when it is ONE line that fits the slot:
# a git URL after `git clone`, a URL for curl/wget/…, or a path that exists. Anything else
# (passwords, prose, code) is never shown. Reads time out quickly so TAB never hangs.
# One reader for all three shells (src/extras.rs): wl-paste / xclip / xsel / pbpaste with its own
# 0.4 s timeout (macOS has no `timeout` command), COMPLETE_ME_CLIPBOARD for tests.
function __complete_me_clip -a want -d 'complete-me: the clipboard, if it is a URL/git URL/path (want: git|url|path|dir)'
    command complete-me clip $want 2>/dev/null
end

complete -c git -n '__fish_seen_subcommand_from clone' -k -a '(__complete_me_clip git)'
complete -c git -n '__fish_seen_subcommand_from remote; and __fish_seen_subcommand_from add set-url' -k -a '(__complete_me_clip git)'
complete -c git -n '__fish_seen_subcommand_from submodule; and __fish_seen_subcommand_from add' -k -a '(__complete_me_clip git)'
for __cm_c in curl wget xh http https aria2c yt-dlp mpv xdg-open open firefox chromium links lynx w3m
    complete -c $__cm_c -k -a '(__complete_me_clip url)'
end
for __cm_c in cat less more bat vim nvim vi nano hx code subl cp mv ln tar unzip file stat du ls head tail \
        diff source xdg-open open rsync scp chmod chown wc sha256sum md5sum
    complete -c $__cm_c -k -a '(__complete_me_clip path)'
end
complete -c cd -k -a '(__complete_me_clip dir)'
set -e __cm_c

# ---------------------------------------------------------------- generate on miss

function __complete_me_wants -a cmd -d 'complete-me: true for an external command with no completions yet'
    string match -qr '^[A-Za-z0-9_][\w.+-]*$' -- $cmd; or return 1
    contains -- $cmd $__cm_checked; and return 1
    set -ga __cm_checked $cmd
    command -q -- $cmd; or return 1
    functions -q -- $cmd; and return 1
    contains -- $cmd (builtin -n); and return 1
    status get-file completions/$cmd.fish >/dev/null 2>&1; and return 1
    for d in $fish_complete_path
        string match -q -- '*/fish/generated_completions' $d; and continue
        if test "$d" = "$__complete_me_gen"; and test -e $d/$cmd.fish
            # ours: refresh in the background once it is 30 days old (the tool may have changed)
            if test (math (date +%s) - (path mtime -- $d/$cmd.fish)) -gt 2592000
                command complete-me gen --force --quiet -- $cmd &>/dev/null &
                disown 2>/dev/null
            end
            return 1
        end
        test -e $d/$cmd.fish; and return 1
    end
    set -l defined (complete -c $cmd)
    set -q defined[1]; and return 1
    return 0
end

function __complete_me_tab -d 'complete-me: generate completions on the first TAB, then complete'
    set -l toks (commandline -pc --tokens-raw)[1]
    if set -q toks[1]; and __complete_me_wants $toks[1]
        complete-me gen --quick --quiet -- $toks[1] 2>/dev/null
        if test -e $__complete_me_gen/$toks[1].fish
            source $__complete_me_gen/$toks[1].fish
            command complete-me gen --quiet -- $toks[1] &>/dev/null &
            disown 2>/dev/null
        end
    end
    commandline -f complete
end

function __complete_me_postexec --on-event fish_postexec -d 'complete-me: pre-generate for commands you run'
    set -l cmd (string split -f1 ' ' -- (string trim -- $argv[1]))
    test -n "$cmd"; or return
    __complete_me_wants $cmd; or return
    command complete-me gen --quiet -- $cmd &>/dev/null &
    disown 2>/dev/null
end

# ---------------------------------------------------------------- regex

function __complete_me_regex_slot -d 'complete-me: is the cursor in the pattern slot?'
    set -l toks (commandline -xpc)
    set -q toks[1]; or return 1
    set -l tool (path basename -- $toks[1])
    set -l prev $toks[-1]
    set -l argopts
    switch $tool
        case find
            contains -- $prev -regex -iregex
            return
        case string
            contains -- "$toks[2]" match replace; or return 1
            contains -- -r $toks; or contains -- --regex $toks; or return 1
            for t in $toks[3..]
                string match -q -- '-*' $t; or return 1
            end
            return 0
        case rg
            set argopts -A -B -C -m -d -g -t -T -j -M -r -E --max-count --glob --iglob --type --type-not \
                --threads --max-columns --replace --max-depth --encoding --sort --sortr --pre --pre-glob --colors
        case sed gsed
            set argopts -l
        case '*'
            set argopts -A -B -C -m -d -D --max-count --include --exclude --exclude-dir --label --group-separator
    end
    contains -- $prev -e --regexp --expression; and return 0
    set -l skip 0
    for t in $toks[2..]
        if test $skip = 1
            set skip 0
            continue
        end
        switch $t
            case -e --regexp -f --file --expression
                return 1
            case '-*'
                contains -- $t $argopts; and set skip 1
            case '*'
                return 1
        end
    end
    return 0
end

function __complete_me_regex_cands
    complete-me regex complete --current (commandline -ct) -- (commandline -xpc)
end

for __cm_c in grep egrep rgrep zgrep ugrep rg sed gsed string
    complete -c $__cm_c -n __complete_me_regex_slot -k -x -a '(__complete_me_regex_cands)'
end
complete -c find -n __complete_me_regex_slot -k -x -a '(__complete_me_regex_cands)'
set -e __cm_c

function __complete_me_quote -d 'complete-me: single-quote a string for the fish command line'
    printf "'%s'" (string replace -a -- '\\' '\\\\' $argv[1] | string replace -a -- "'" "\\'")
end

function __complete_me_regex_builder -d 'complete-me: live regex builder'
    set -l toks (commandline -xpc)
    set -l cur (commandline -ct)
    set -l q (complete-me regex build --current "$cur" -- $toks)
    if test $status -eq 0; and test -n "$q"
        # q arrives already quoted for fish
        if test -z (string trim -- (commandline))
            commandline -r -- "rg $q "
        else
            commandline -t -- $q
        end
    end
    commandline -f repaint
end

# ---------------------------------------------------------------- find

function __complete_me_find_exec_block -d 'complete-me: words of the open -exec block, if any'
    set -l toks (commandline -xpc)
    set -l idx 0
    for i in (seq (count $toks))
        switch $toks[$i]
            case -exec -execdir -ok -okdir
                set idx $i
            case ';'
                set idx 0
            case '+'
                test $i -gt 1; and test "$toks[(math $i - 1)]" = '{}'; and set idx 0
        end
    end
    test $idx -gt 0; or return 1
    test $idx -lt (count $toks); and printf '%s\n' $toks[(math $idx + 1)..]
    return 0
end

function __complete_me_find_in_exec
    __complete_me_find_exec_block >/dev/null
end

function __complete_me_find_exec -d 'complete-me: completions inside find -exec … \;'
    set -l block (__complete_me_find_exec_block)
    if not set -q block[1]
        __fish_complete_command
        return
    end
    if not contains -- '{}' $block
        printf '%s\t%s\n' '{}' 'the path of each file find found'
    else if test "$block[-1]" = '{}'
        printf '%s\t%s\n' ';' 'end -exec: run the command once per file' '+' 'end -exec: one run with many files (faster)'
    else
        printf '%s\t%s\n' ';' 'end -exec: run the command once per file'
    end
    complete -C (string join -- ' ' (string escape -- $block) (commandline -ct))
end

function __complete_me_find_expr_slot
    __complete_me_find_in_exec; and return 1
    for t in (commandline -xpc)[2..]
        string match -q -- '-*' $t; and return 0
    end
    return 1
end

# Values hang off the options themselves: after an option that takes an argument,
# fish only consults that option's own argument completions.
for __cm_t in type xtype size mtime atime ctime mmin amin cmin perm name iname lname ilname path ipath \
        wholename maxdepth mindepth regextype printf newermt newerct newerat
    complete -c find -o $__cm_t -k -x -a "(complete-me find values -- -$__cm_t)"
end
set -e __cm_t
complete -c find -o exec -o execdir -o ok -o okdir -k -x -a '(__complete_me_find_exec)'
complete -c find -n __complete_me_find_in_exec -k -a '(__complete_me_find_exec)'
complete -c find -n __complete_me_find_expr_slot -k -a '(complete-me find operators)'

# ---------------------------------------------------------------- ssh -oKey=Value

for __cm_c in ssh scp sftp
    complete -c $__cm_c -s o -x -a '(complete-me ssh-options --current (string replace -r -- "^-o" "" (commandline -ct)))'
end
set -e __cm_c

# ---------------------------------------------------------------- explain & snippets

function __complete_me_explain -d 'complete-me: explain the command line'
    set -l toks (commandline -x)
    test -n "$toks[1]"; or return
    echo
    complete-me explain -- $toks
    commandline -f repaint
end

function __complete_me_snippet -d 'complete-me: insert a snippet'
    set -l s (complete-me snippets pick | string collect)
    if test -n "$s"
        set -l parts (string split -m1 -- '%' $s)
        commandline -i -- $parts[1]
        set -l keep (commandline -C)
        commandline -i -- "$parts[2]"
        commandline -C $keep
    end
    commandline -f repaint
end

if not set -q complete_me_no_abbrs
    # Cached so shell startup never runs anything; rebuilt when the complete-me binary changes.
    set -l abbrs $__complete_me_cache/abbrs.fish
    set -l src (path resolve (command -s complete-me))
    if not test -f $abbrs; or test (path mtime -- $src) -gt (path mtime -- $abbrs)
        complete-me snippets fish-abbrs >$abbrs.tmp; and mv $abbrs.tmp $abbrs
    end
    test -f $abbrs; and source $abbrs
end

# ---------------------------------------------------------------- bash typed into fish

# On Enter: if the line is not valid fish but is valid bash, translate it (complete-me's own
# bash parser and translator),
# show the fish version, and run it, unless the safety check flags something. Then it waits
# for a second Enter (ctrl-c cancels). Turn off with: set -U complete_me_no_bash 1
function __complete_me_enter -d 'complete-me: run bash typed into fish (translated, safety-checked)'
    if set -q complete_me_no_bash; or commandline --paging-mode
        commandline -f execute
        return
    end
    set -l line (commandline | string collect)
    commandline --is-valid
    set -l valid $status
    if test -z "$line"; or test $valid -eq 0
        commandline -f execute
        return
    end
    set -l out (command complete-me bash2fish -- "$line" 2>&1)
    set -l rc $status
    if test $rc -eq 2               # not bash either: let fish report / continue the line
        commandline -f execute
        return
    end
    set -l sep (contains -i -- ---FISH--- $out)
    if test $rc -ne 0; or test -z "$sep"
        printf '\n\e[33mcomplete-me:\e[0m this looks like bash, but it could not be translated: %s\n' "$out[-1]"
        commandline -f repaint
        return
    end
    set -l fishcode (string join \n -- $out[(math $sep + 1)..] | string collect)
    set -l risky (string match -r -- '^RISK\t.*' $out | string replace -r '^RISK\t' '')
    commandline -r -- $fishcode
    if set -q risky[1]
        printf '\n\e[33mbash → fish\e[0m (translated). \e[31mHeld for a check:\e[0m\n'
        printf '  \e[31m⚠\e[0m %s\n' $risky
        printf '  Review the fish version below. \e[1mEnter\e[0m runs it, \e[1mctrl-c\e[0m cancels.\n'
        commandline -f repaint
        return
    end
    printf '\n\e[2mbash → fish\e[0m\n'
    commandline -f repaint execute
end

# ---------------------------------------------------------------- key bindings

function __complete_me_bind -d 'complete-me: install key bindings'
    for mode in default insert
        bind -M $mode tab __complete_me_tab 2>/dev/null
        bind -M $mode alt-r __complete_me_regex_builder 2>/dev/null
        bind -M $mode alt-x __complete_me_explain 2>/dev/null
        bind -M $mode alt-j __complete_me_snippet 2>/dev/null
        bind -M $mode enter __complete_me_enter 2>/dev/null
    end
end
function __complete_me_rebind --on-variable fish_key_bindings
    __complete_me_bind
end
__complete_me_bind
