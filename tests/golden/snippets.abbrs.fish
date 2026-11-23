abbr -a --set-cursor -- ,for 'for x in %
    echo $x
end'  # loop over values
abbr -a --set-cursor -- ,forf 'for f in *.%
    echo $f
end'  # loop over files matching a glob
abbr -a --set-cursor -- ,fori 'for i in (seq %)
    echo $i
end'  # count from 1 to N
abbr -a --set-cursor -- ,forl 'for line in (%)
    echo $line
end'  # loop over the lines of a command's output
abbr -a --set-cursor -- ,while 'while read -l line
    echo $line
end < %'  # read a file line by line
abbr -a --set-cursor -- ,if 'if test %
    
end'  # if a test is true
abbr -a --set-cursor -- ,ife 'if test %
    
else
    
end'  # if / else
abbr -a --set-cursor -- ,ifc 'if %
    
end'  # if a command succeeds
abbr -a --set-cursor -- ,iff 'if test -f %
    
end'  # if a file exists
abbr -a --set-cursor -- ,ifd 'if test -d %
    
end'  # if a directory exists
abbr -a --set-cursor -- ,ifv 'if set -q %
    
end'  # if a variable is set
abbr -a --set-cursor -- ,sw 'switch %
    case \'a*\'
        
    case \'*\'
        
end'  # switch on a value
abbr -a --set-cursor -- ,fn 'function % --description \'\'
    
end'  # define a function (passes all args)
abbr -a --set-cursor -- ,fna 'function % --description \'\'
    argparse h/help \'n/name=\' -- $argv
    or return
    if set -q _flag_help
        echo "usage: "(status function)" [-n NAME] ARGS…"
        return 0
    end
    echo $_flag_name $argv
end'  # function with parsed options (argparse)
abbr -a --set-cursor -- ,wrap 'function % --wraps CMD
    CMD $argv
end'  # function that wraps a command and keeps its completions
abbr -a --set-cursor -- ,ev 'function % --on-event fish_postexec
    
end'  # run a function when an event fires
abbr -a --set-cursor -- ,onvar 'function % --on-variable PWD
    
end'  # run a function when a variable changes
abbr -a --set-cursor -- ,set 'set -gx % '  # export a variable
abbr -a --set-cursor -- ,path 'fish_add_path %'  # prepend to PATH (deduplicated)
abbr -a --set-cursor -- ,sub 'set out (%)'  # capture a command's output
abbr -a --set-cursor -- ,st 'if test $status -ne 0
    %
end'  # check the last exit status
abbr -a --set-cursor -- ,and '%; and '  # run the next command only if this succeeds
abbr -a --set-cursor -- ,or '%; or '  # run the next command only if this fails
abbr -a --set-cursor -- ,psub '(% | psub)'  # pass command output as a file (bash <(…))
abbr -a --set-cursor -- ,repl 'string replace -r -- \'%\' \'\' $x'  # string replace (regex)
abbr -a --set-cursor -- ,split 'string split -- \'%\' $x'  # split a string
abbr -a --set-cursor -- ,match 'string match -rg -- \'%\' $x'  # regex match with capture groups
abbr -a --set-cursor -- ,math 'math "%"'  # arithmetic
abbr -a --set-cursor -- ,count 'count %'  # count items
abbr -a --set-cursor -- ,ask 'read -l -P \'% [y/N] \' answer
if string match -qi \'y*\' -- $answer
    
end'  # ask the user a yes/no question
abbr -a --set-cursor -- ,trap 'function __cleanup --on-event fish_exit
    %
end'  # run cleanup when the shell exits
abbr -a --set-cursor -- ,bg '% &; disown'  # run in the background and forget it
abbr -a --set-cursor -- ,findx 'find . -type f -name \'%\' -exec CMD {} \\;'  # find files and run a command on each
abbr -a --set-cursor -- ,findb 'find . -type f -name \'%\' -exec CMD {} +'  # find files and run one command on all of them
abbr -a --set-cursor -- ,findn 'find . -type f -mtime -1 %'  # find files changed in the last day
abbr -a --set-cursor -- ,findbig 'find % -type f -size +100M -exec ls -lh {} +'  # find big files
abbr -a --set-cursor -- ,findgit 'find . \\( -path ./.git -o -path ./node_modules \\) -prune -o -type f -name \'%\' -print'  # find, skipping .git and node_modules
abbr -a --set-cursor -- ,xargs 'find . -type f -name \'%\' -print0 | xargs -0 CMD'  # safe find | xargs for odd file names
abbr -a --set-cursor -- ,rgf 'rg -l \'%\''  # ripgrep: list files that match
abbr -a --set-cursor -- ,rgr 'rg \'%\' -r \'REPLACEMENT\''  # ripgrep: search and preview a replacement
abbr -a --set-cursor -- ,sedi 'sed -i -E \'s/%//g\' FILE'  # replace text in a file in place
abbr -a --set-cursor -- ,tunnel 'ssh -N -L %:localhost:PORT HOST'  # ssh local port forward
