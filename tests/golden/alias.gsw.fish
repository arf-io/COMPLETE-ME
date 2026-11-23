# Created by complete-me (alias). Remove: complete-me alias --rm gsw
function gsw --wraps 'git switch' --description 'alias gsw=git switch'
    git switch $argv
end
