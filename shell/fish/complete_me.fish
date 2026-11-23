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

# Container vendors can erase previously registered completions on autoload.
set -l __cm_container_adapters (status dirname)/completions
if not contains -- $__cm_container_adapters $fish_complete_path
    set -g fish_complete_path $__cm_container_adapters $fish_complete_path
end

# ---------------------------------------------------------------- subcommand paths

function __complete_me_at -d 'complete-me: is the command line at this subcommand path?'
    set -l id $argv[1]
    set -l want "$argv[2..]"
    set -l line (commandline -pc)
    if test "$__cm_at_key" != "$id|$line"
        set -l grammar __cm_grammar_$id
        if set -q $grammar
            set -l context (command complete-me _resolve -- $$grammar -- (commandline -xpc)[2..])
            set -g __cm_at_key "$id|$line"
            set -l fields (string split '|' -- "$context")
            set -g __cm_at_val "$fields[1]"
            set -g __cm_at_subcommands "$fields[5]"
            test "$__cm_at_val" = "$want"
            return
        end
        # Generated files from older releases still use the shell tree.
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
        set -g __cm_at_subcommands 1
    end
    test "$__cm_at_val" = "$want"
end

function __complete_me_subcommand
    __complete_me_at $argv; and test "$__cm_at_subcommands" = 1
end

# ---------------------------------------------------------------- history-ranked arguments

function __complete_me_hist_args -d 'complete-me: arguments used before at this position, most recent first'
    set -l prefix (string join ' ' -- $argv)
    set -l skip (count $argv)
    set -l seen
    for line in (history search --prefix --max 400 -- "$prefix ")
        set -l toks (commandline -I $line --tokens-raw 2>/dev/null | string unescape 2>/dev/null)
        string match -qr -- '^--?password(=|$)' $toks; and continue
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
        string match -qr -- '^--?password(=|$)' $toks; and continue
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
    set -l defined (complete -c $cmd | string match -v -- '*(__complete_me_live*')
    set -q defined[1]; and return 1
    return 0
end

function __complete_me_process_suffix --argument-names left
    set -l process (commandline -p | string collect)
    set -l left_len (string length -- "$left")
    if test "$left_len" -gt 0
        test (string sub -l $left_len -- "$process") = "$left"; or return 1
    end
    string sub -s (math 1 + $left_len) -- "$process"
    return 0
end

function __complete_me_inside_token
    set -l left (commandline -ct | string collect)
    set -l full (commandline -t | string collect)
    set -l at_cursor (commandline -pc | string collect)
    set -l right
    if test "$left" != "$full"
        set -l left_len (string length -- "$left")
        test $left_len -eq 0; or test (string sub -l $left_len -- "$full") = "$left"; or return 1
        set right (string sub -s (math 1 + $left_len) -- "$full")
    end
    # Prevent the native env wrapper from offering data in a different context.
    set -l scoped
    set -l tokens (commandline -pc --tokens-raw | string unescape)
    if string match -qr '(^|[[:space:]])(env|sudo|doas|nice|command|[A-Za-z_][A-Za-z0-9_]*=)' -- "$at_cursor"; or \
        string match -qr '(^|[[:space:]/])git[[:space:]].*[[:space:]]?config([[:space:]]|$)' -- "$at_cursor"; or \
        string match -qr '(^|[[:space:]/])hexdump[[:space:]].*[-]s[[:space:]]' -- "$at_cursor"; or \
        string match -qr '[[:space:]]--?password([[:space:]]|=)' -- "$at_cursor"; or \
        string match -qr '(^|/)aws$' -- $tokens
        set -l suffix (__complete_me_process_suffix "$at_cursor" | string collect)
        test "$pipestatus[1]" = 0; or return 0
        set scoped (printf '%s\0%s\0' "$at_cursor" "$suffix" | command complete-me live --fish-scoped --editor-input)
        if test "$scoped[1]" = blocked
            return 0
        end
    end
    set -l edits
    if test "$scoped[1]" = scoped
        set edits (begin
            printf '%s\0%s\0' "$left" "$right"
            printf '%s\n' $scoped[2..]
        end | command complete-me _fish-edit --editor-input)
    else if test -n "$right"
        set edits (begin
            printf '%s\0%s\0' "$left" "$right"
            complete --do-complete="$at_cursor"
        end | command complete-me _fish-edit --editor-input)
    else
        return 1
    end
    set -l selected
    switch (count $edits)
        case 0
            return 0
        case 1
            set selected $edits[1]
        case '*'
            if command -q fzf; and isatty stdout
                set selected (printf '%s\n' $edits | fzf --delimiter='\t' --with-nth=2,3 --prompt='Complete token> ')
            else
                printf '\n'
                for row in $edits
                    printf '%s\n' (string split -m1 \t -- "$row")[2]
                end
            end
            commandline -f repaint
    end
    if set -q selected[1]
        commandline -i -- (string split -m1 \t -- "$selected")[1]
    end
    return 0
end

function __complete_me_calc_tab
    set -q COMPLETE_ME_NO_CALC; and return 1
    set -l buffer (commandline | string collect)
    set -l cursor (commandline -C)
    test "$cursor" -gt 0; or return 1
    set -l left (string sub -l $cursor -- "$buffer")
    string match -qr '[=+*/%^(,-]$' -- "$left"; or return 1
    set -l right (string sub -s (math "$cursor + 1") -- "$buffer")
    set -l reply (printf '%s\0%s\0' "$left" "$right" | command complete-me _calc-edit "$__complete_me_calc_answer" | string split -m 1 \t)
    set -q reply[2]; or return 1
    set -g __complete_me_calc_answer "$reply[1]"
    commandline -i -- "$reply[2]"
    return 0
end

function __complete_me_tab -d 'complete-me: generate completions on the first TAB, then complete'
    __complete_me_calc_tab; and return
    set -l toks (commandline -pc --tokens-raw)[1]
    if set -q toks[1]; and __complete_me_wants $toks[1]
        complete-me gen --quick --quiet -- $toks[1] 2>/dev/null
        if test -e $__complete_me_gen/$toks[1].fish
            source $__complete_me_gen/$toks[1].fish
            command complete-me gen --quiet -- $toks[1] &>/dev/null &
            disown 2>/dev/null
        end
    end
    __complete_me_inside_token; or commandline -f complete
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

function __complete_me_current
    set -l raw (commandline -ct)
    string unescape -- "$raw"; or printf '%s\n' "$raw"
end

for __cm_c in ssh scp sftp
    complete -c $__cm_c -s o -x -a '(command complete-me ssh-options --current=(__complete_me_current))'
end
set -e __cm_c

# ---------------------------------------------------------------- live state

function __complete_me_live
    set -l left (commandline -pc | string collect)
    # Fish's native completion engine truncates its view at the cursor.
    # Only the COMPLETE-ME Tab widget can safely resolve S3 object scope.
    printf '%s\0\0' "$left" | command complete-me live --editor-input --fish-native $argv
end

for __cm_c in tmux docker-compose gh just task zerotier-cli git pct qm pvesm vzdump qmrestore zfs zpool aws vultr-cli hf huggingface-cli codex claude gemini agy hermes fx cline opencode aider cursor-agent copilot droid qwen goose amp pi dd tcpdump tshark wireshark ip route nmap kubectl systemctl journalctl npm pnpm yarn bun brew
    complete -c $__cm_c -k -a '(__complete_me_live)'
end
set -e __cm_c
complete -c vzdump -c qmrestore -l storage -x -a '(__complete_me_live --fish-option=--storage)'
complete -c pct -l storage -n '__fish_seen_subcommand_from restore' -x -a '(__complete_me_live --fish-option=--storage)'
if not type -q proxmox
    function proxmox -d 'Run a Proxmox node command over SSH'
        command complete-me proxmox $argv
    end
    complete -c proxmox -k -x -a '(__complete_me_live)'
end
if not type -q kube
    function kube -d 'Select a Kubernetes context and namespace'
        command complete-me kube $argv
    end
    complete -c kube -k -a '(__complete_me_live)'
end
if not type -q cm-cloud
    function cm-cloud -d 'Query supported cloud inventories'
        command complete-me cloud $argv
    end
    complete -c cm-cloud -k -x -a '(__complete_me_live)'
end
for __cm_flag in profile region
    complete -c aws -l $__cm_flag -x -a "(__complete_me_live --fish-option=--$__cm_flag)"
end
for __cm_slot in ec2:instance-ids eks:name ecs:cluster lambda:function-name rds:db-instance-identifier s3api:bucket s3api:key s3api:prefix
    set -l __cm_fields (string split : -- $__cm_slot)
    complete -c aws -l $__cm_fields[2] -n "__fish_seen_subcommand_from $__cm_fields[1]" -r -a "(__complete_me_live --fish-option=--$__cm_fields[2])"
end
for __cm_tool in codex claude gemini agy hermes cline opencode aider cursor-agent copilot droid qwen goose pi
    complete -c $__cm_tool -l model -r -a '(__complete_me_live --fish-option=--model)'
end
for __cm_tool in claude gemini hermes
    complete -c $__cm_tool -l resume -s r -r -a '(__complete_me_live --fish-option=--resume)'
end
complete -c agy -l conversation -r -a '(__complete_me_live --fish-option=--conversation)'
complete -c opencode -l session -s s -r -a '(__complete_me_live --fish-option=--session)'
complete -c vultr-cli -l region -r -a '(__complete_me_live --fish-option=--region)'
set -e __cm_flag __cm_slot __cm_tool
for __cm_capture in tcpdump wireshark tshark
    complete -c $__cm_capture -s i -x -k -a '(__complete_me_live --fish-option=-i)'
    complete -c $__cm_capture -s r -r -F
    complete -c $__cm_capture -s w -r -F
end
set -e __cm_capture
for __cm_capture in wireshark tshark
    for __cm_filter in f Y
        complete -c $__cm_capture -s $__cm_filter -x -a '(__complete_me_live)'
    end
end
set -e __cm_capture __cm_filter
for __cm_sub in alias bash2fish bytes doctor env explain find forget func gen list live path regex show snippets ssh-options
    if not type -q cm-$__cm_sub
        function cm-$__cm_sub --inherit-variable __cm_sub --wraps "complete-me $__cm_sub"
            complete-me $__cm_sub $argv
        end
    end
end
if not type -q path-why
    function path-why --wraps 'complete-me path why'
        complete-me path why $argv
    end
end
if not type -q zfs-path
    function zfs-path -d 'Resolve a mounted local ZFS path'
        command complete-me zfs-path $argv
    end
    complete -c zfs-path -k -x -a '(__complete_me_live)'
end
if not type -q cm-recipe
    function cm-recipe -d 'Work with COMPLETE-ME recipes'
        command complete-me recipe $argv
    end
    complete -c cm-recipe -k -x -a '(__complete_me_recipe_candidates)'
end
complete -c git -n '__fish_seen_subcommand_from config' -f
complete -c git -n '__fish_seen_subcommand_from config' -l type -s t -x -a '(__complete_me_live --fish-option=--type)'
complete -c git -n '__fish_seen_subcommand_from config' -l file -s f -r -F
for __cm_flag in t s c
    complete -c tmux -s $__cm_flag -r -k -a "(__complete_me_live --fish-option=-$__cm_flag)"
end
set -e __cm_flag
function __complete_me_load_container_completions
    set -l tool $argv[1]
    set -l adapter (path resolve -- $argv[2])
    set -l loaded 0
    for directory in $fish_complete_path
        set -l vendor "$directory/$tool.fish"
        if test -r "$vendor"; and test (path resolve -- "$vendor") != "$adapter"
            source "$vendor"
            set loaded 1
            break
        end
    end
    if test $loaded -eq 0
        status get-file "completions/$tool.fish" 2>/dev/null | source
    end

    complete -c $tool -k -a '(__complete_me_live)'
    complete -c $tool -l network -n '__fish_seen_subcommand_from run create' -r -a '(__complete_me_live --fish-option=--network)'
    complete -c $tool -l volume -s v -n '__fish_seen_subcommand_from run create' -r -a '(__complete_me_live --fish-option=--volume)'
    for flag in mount volumes-from restart
        complete -c $tool -l $flag -n '__fish_seen_subcommand_from run create update' -r -a "(__complete_me_live --fish-option=--$flag)"
    end
    complete -c $tool -l filter -s f -n '__fish_seen_subcommand_from ps ls list images' -r -a '(__complete_me_live --fish-option=--filter)'
    complete -c $tool -l type -n '__fish_seen_subcommand_from inspect' -r -a '(__complete_me_live --fish-option=--type)'
    if test $tool = docker
        complete -c docker -l link -n '__fish_seen_subcommand_from run create' -r -a '(__complete_me_live --fish-option=--link)'
        complete -c docker -l context -s c -x -a '(__complete_me_live --fish-option=--context)'
        complete -c docker -l profile -n '__fish_seen_subcommand_from compose' -r -a '(__complete_me_live --fish-option=--profile)'
        for flag in scale attach no-attach exit-code-from
            complete -c docker -l $flag -n '__fish_seen_subcommand_from compose; and __fish_seen_subcommand_from up create' -r -a "(__complete_me_live --fish-option=--$flag)"
        end
    else
        for flag in pod secret
            complete -c podman -l $flag -n '__fish_seen_subcommand_from run create' -r -a "(__complete_me_live --fish-option=--$flag)"
        end
        complete -c podman -l connection -s c -x -a '(__complete_me_live --fish-option=--connection)'
    end
end
complete -c docker-compose -l profile -r -a '(__complete_me_live --fish-option=--profile)'
for __cm_flag in scale attach no-attach exit-code-from
    complete -c docker-compose -l $__cm_flag -n '__fish_seen_subcommand_from up create' -r -a "(__complete_me_live --fish-option=--$__cm_flag)"
end
set -e __cm_flag
for __cm_tool in kube kubectl
    complete -c $__cm_tool -l container -s c -n '__fish_seen_subcommand_from logs exec attach' -r -a '(__complete_me_live --fish-option=--container)'
end
set -e __cm_tool
complete -c kubectl -l context -r -a '(__complete_me_live --fish-option=--context)'
complete -c kubectl -s n -r -a '(__complete_me_live --fish-option=-n)'
complete -c kubectl -l namespace -r -a '(__complete_me_live --fish-option=--namespace)'
complete -c journalctl -s u -r -a '(__complete_me_live --fish-option=-u)'
complete -c journalctl -l unit -r -a '(__complete_me_live --fish-option=--unit)'
complete -c journalctl -l user-unit -r -a '(__complete_me_live --fish-option=--user-unit)'

# ---------------------------------------------------------------- fish functions, aliases and PATH

function __complete_me_define
    set -l mode $argv[1]
    set -e argv[1]
    command complete-me $mode $argv; or return
    # Reload the exact file created by the CLI, including in a fresh fish whose
    # function directory did not exist at startup.
    argparse --ignore-unknown h/help list rm abbr 'shell=' 'desc=' 'wraps=' -- $argv 2>/dev/null; or return 0
    if set -q _flag_help; or set -q _flag_list; or set -q _flag_rm
        return 0
    end
    if set -q _flag_shell[-1]; and not contains -- $_flag_shell[-1] fish all
        return 0
    end
    set -l config ~/.config
    set -q XDG_CONFIG_HOME[1]; and set config $XDG_CONFIG_HOME
    set -q argv[1]; or return 0
    if set -q _flag_abbr
        source "$config/fish/conf.d/complete_me_abbrs.fish"
    else
        source "$config/fish/functions/$argv[1].fish"
    end
end

function __complete_me_doctor
    set -l target (command complete-me doctor --target-only $argv); or return
    set -l binding unbound
    set -l active (bind -M $fish_bind_mode tab 2>/dev/null)
    if not set -q active[1]
        set active (bind -M default tab 2>/dev/null)
    end
    if string match -q '*__complete_me_tab*' -- $active
        set binding complete-me
    else if string match -q '* complete' -- $active
        set binding native
    else if set -q active[1]
        set binding custom
    end
    set -l handler none
    if set -q target[1]
        set -l specs (complete -c $target)
        if string match -qr '__complete_me_(at|subcommand|hist_args|hist_optvals)|__cm_v_' -- $specs
            set handler generated
        else if status get-file completions/$target.fish >/dev/null 2>&1
            set handler native
        else if string match -q '*__complete_me_live*' -- $specs
            set handler live-adapter
        else if test "$binding" = complete-me
            set handler loader
        end
    end
    command complete-me doctor $argv --shell=fish --binding=$binding --handler=$handler
end

function __complete_me_path_why -a __cm_name
    if test (count $argv) -ne 1; or string match -q -- '-*' "$__cm_name"
        command complete-me path why $argv
        return
    end
    set -l __cm_kind (type -t -- "$__cm_name" 2>/dev/null)
    set -l __cm_file
    switch "$__cm_kind"
        case file
            set __cm_file (type -p -- "$__cm_name" | string collect)
        case function
            if builtin functions --details --verbose -- "$__cm_name" | string match -qr '^alias [^[:blank:]=]+[[:blank:]=]'
                set __cm_kind alias
            end
    end
    command complete-me path _why "$__cm_name" "$__cm_kind" "$__cm_file"
end

function __complete_me_env_restore
    set -l __cm_index 1
    set -l __cm_failed 0
    while set -q argv[$__cm_index]
        set -l __cm_name $argv[$__cm_index]
        set -l __cm_scope $argv[(math $__cm_index + 1)]
        set -l __cm_path $argv[(math $__cm_index + 2)]
        set -l __cm_count $argv[(math $__cm_index + 3)]
        set __cm_index (math $__cm_index + 4)
        if test "$__cm_scope" = absent
            if set -qg $__cm_name
                builtin set -eg -- $__cm_name; or set __cm_failed 1
            end
        else
            set -l __cm_values
            if test $__cm_count -gt 0
                set __cm_values $argv[$__cm_index..(math $__cm_index + $__cm_count - 1)]
            end
            true
            builtin set -g $__cm_scope $__cm_path -- $__cm_name $__cm_values; or set __cm_failed 1
        end
        set __cm_index (math $__cm_index + $__cm_count)
    end
    return $__cm_failed
end

function __complete_me_env_apply
    set -l __cm_confirm 1
    if test "$argv[1]" = --yes
        set __cm_confirm 0
        set -e argv[1]
    end
    if test (count $argv) -ne 1
        printf '%s\n' 'Usage: complete-me env apply [--yes] FILE|-' >&2
        return 2
    end
    set -l --unexport __cm_plan
    command complete-me env _plan "$argv[1]" | while builtin read -z -l __cm_record
        set -a __cm_plan "$__cm_record"
    end
    test $pipestatus[1] -eq 0; or return 1
    test "$__cm_plan[-1]" = ready; or return 1
    set -e __cm_plan[-1]
    if not set -q __cm_plan[1]
        printf '%s\n' 'No environment changes.'
        return
    end

    set -l --unexport __cm_before
    set -l __cm_index 1
    while set -q __cm_plan[$__cm_index]
        set -l __cm_name $__cm_plan[(math $__cm_index + 1)]
        if set -qU $__cm_name
            printf 'complete-me env: %s has a universal value\n' "$__cm_name" >&2
            return 1
        end
        if set -qg $__cm_name
            set -l __cm_scope --unexport
            set -l __cm_path --unpath
            set -qgx $__cm_name; and set __cm_scope --export
            set -qg --path $__cm_name; and set __cm_path --path
            set -a __cm_before $__cm_name $__cm_scope $__cm_path (count $$__cm_name) $$__cm_name
        else
            set -a __cm_before $__cm_name absent - 0
        end
        builtin printf '%s %s\n' "$__cm_plan[$__cm_index]" "$__cm_name"
        set __cm_index (math $__cm_index + 3)
    end
    if test $__cm_confirm -eq 1
        builtin read -l -P 'Apply to this fish session? [y/N] ' __cm_answer </dev/tty; or return 1
        string match -qi -- y "$__cm_answer"; or return 1
    end

    set __cm_index 1
    while set -q __cm_plan[$__cm_index]
        set -l __cm_name $__cm_plan[(math $__cm_index + 1)]
        set -l __cm_value $__cm_plan[(math $__cm_index + 2)]
        if test "$__cm_plan[$__cm_index]" = remove
            builtin set -eg -- "$__cm_name"
        else
            true
            builtin set -gx -- "$__cm_name" "$__cm_value"
        end
        if test $status -ne 0
            __complete_me_env_restore $__cm_before
            printf '%s\n' 'complete-me env: unable to apply environment changes' >&2
            return 1
        end
        set __cm_index (math $__cm_index + 3)
    end
    set -g --unexport --unpath __cm_env_undo $__cm_before
end

function __complete_me_env
    switch "$argv[1]"
        case apply undo
            if set -q __cm_env_busy
                printf '%s\n' 'complete-me env: an environment change is in progress' >&2
                return 1
            end
            set -g __cm_env_busy 1
            if test "$argv[1]" = apply
                __complete_me_env_apply $argv[2..]
            else if set -q argv[2]
                printf '%s\n' 'Usage: complete-me env undo' >&2
                false
            else if not set -q __cm_env_undo[1]
                printf '%s\n' 'complete-me env: no changes to undo' >&2
                false
            else if __complete_me_env_restore $__cm_env_undo
                set -eg __cm_env_undo
                printf '%s\n' 'Restored the previous environment.'
            else
                false
            end
            set -l __cm_status $status
            set -eg __cm_env_busy
            return $__cm_status
        case '*'
            command complete-me env $argv
    end
end

function complete-me -d 'Generate completions and manage fish functions, aliases and PATH'
    switch "$argv[1]"
        case doctor
            __complete_me_doctor $argv[2..]
        case env
            __complete_me_env $argv[2..]
        case path
            switch "$argv[2]"
                case '' list
                    printf '%s\n' $PATH
                case add
                    fish_add_path $argv[3..]
                case why
                    __complete_me_path_why $argv[3..]
                case '*'
                    printf '%s\n' 'Usage: complete-me path list | add [fish_add_path options] DIRECTORY… | why COMMAND' >&2
                    return 2
            end
        case alias
            if test "$argv[2]" = --list; and test (count $argv) -eq 2
                command complete-me alias --list; or return
                alias
                return
            end
            __complete_me_define $argv
        case func
            switch "$argv[2]"
                case --save
                    funcsave $argv[3..]
                case --list
                    functions --names
                case '*'
                    __complete_me_define $argv
            end
        case '*'
            command complete-me $argv
    end
end

function __complete_me_recipe_slot
    set -l toks (commandline -xpc)
    test "$toks[2]" = recipe
end

function __complete_me_recipe_candidates
    set -l toks (commandline -xpc)
    set -l current (__complete_me_current)
    if test "$toks[1]" = cm-recipe
        command complete-me recipe _complete fish $toks[2..] "$current"
    else
        command complete-me recipe _complete fish $toks[3..] "$current"
    end
end

function __complete_me_recipe_edit
    set -l line (commandline | string collect)
    string match -qr '^[[:blank:]]*(complete-me[[:blank:]]+recipe|cm-recipe)[[:blank:]]' -- "$line"; or return 0
    set -l expanded (builtin printf '%s' "$line" | command complete-me recipe _edit fish | string collect)
    if test -n "$expanded"
        set -l leading (string match -r '^[[:blank:]]+' -- "$line")
        commandline -r -- "$leading$expanded"
        commandline -C (string length -- (commandline | string collect))
    end
end

complete -c complete-me -n '__fish_use_subcommand' -xa 'gen show forget list explain doctor env recipe regex find ssh-options alias func path proxmox kube cloud zfs-path bytes snippets bash2fish'
complete -c complete-me -n '__fish_seen_subcommand_from proxmox kube cloud zfs-path' -k -x -a '(__complete_me_live)'
complete -c complete-me -n __complete_me_recipe_slot -x -k -a '(__complete_me_recipe_candidates)'
complete -c complete-me -n '__fish_seen_subcommand_from env; and not __fish_seen_subcommand_from diff apply undo' -xa 'diff apply undo'
complete -c complete-me -n '__fish_seen_subcommand_from env; and __fish_seen_subcommand_from apply' -l yes -d 'Apply without prompting'
complete -c complete-me -n '__fish_seen_subcommand_from alias func' -l list -d 'List definitions in fish'
complete -c complete-me -n '__fish_seen_subcommand_from alias func' -l wraps -xa '(__fish_complete_command)' -d 'Inherit completions from this command'
complete -c complete-me -n '__fish_seen_subcommand_from alias func' -l desc -x -d 'Set a description'
complete -c complete-me -n '__fish_seen_subcommand_from alias func' -l shell -xa 'fish bash zsh all' -d 'Write for this shell'
complete -c complete-me -n '__fish_seen_subcommand_from func' -l save -xa '(functions --names)' -d 'Save a fish function'
complete -c complete-me -n '__fish_seen_subcommand_from alias' -l abbr -d 'Create a fish abbreviation'
complete -c complete-me -n '__fish_seen_subcommand_from alias' -l rm -x -d 'Remove a managed definition'
complete -c complete-me -n '__fish_seen_subcommand_from path; and not __fish_seen_subcommand_from add list why' -xa 'list add why'
complete -c complete-me -n '__fish_seen_subcommand_from path; and __fish_seen_subcommand_from why' -xa '(__fish_complete_command)'
complete -c complete-me -n '__fish_seen_subcommand_from path; and __fish_seen_subcommand_from add' -a '(__fish_complete_directories (__complete_me_current))' -f
for __cm_flag in append prepend global universal path move verbose dry-run
    complete -c complete-me -n '__fish_seen_subcommand_from path; and __fish_seen_subcommand_from add' -l $__cm_flag
end
set -e __cm_flag

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

# On Enter: translate Bash syntax and simple standalone exports. Show the fish
# version and run it. Risky Bash translations wait for a second Enter.
# Turn off with: set -U complete_me_no_bash 1
function __complete_me_enter -d 'complete-me: run bash typed into fish (translated, safety-checked)'
    if set -q complete_me_no_bash; or commandline --paging-mode
        commandline -f execute
        return
    end
    set -l line (commandline | string collect)
    commandline --is-valid
    set -l valid $status
    if test -z "$line"
        commandline -f execute
        return
    end
    set -l mode
    if test $valid -eq 0
        if not string match -qr '^[[:blank:]]*export[[:blank:]]' -- "$line"
            commandline -f execute
            return
        end
        set -l definition (functions --details export)
        if not contains -- "$definition" embedded:functions/export.fish "$__fish_data_dir/functions/export.fish"
            commandline -f execute
            return
        end
        set mode --export
    end
    set -l out (builtin printf '%s' "$line" | command complete-me bash2fish $mode 2>&1)
    set -l rc $status
    if test $rc -eq 2               # Fish handles this form or continues an incomplete line.
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
    set -l risky
    if test $sep -gt 1
        set risky (string match -r -- '^RISK\t.*' $out[1..(math $sep - 1)] | string replace -r '^RISK\t' '')
    end
    # A leading space keeps the submission out of fish history.
    set -l leading (string match -r '^[[:blank:]]+' -- "$line")
    commandline -r -- "$leading$fishcode"
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
        bind -M $mode alt-R __complete_me_recipe_edit 2>/dev/null
        bind -M $mode enter __complete_me_enter 2>/dev/null
    end
end
function __complete_me_rebind --on-variable fish_key_bindings
    __complete_me_bind
end
__complete_me_bind
