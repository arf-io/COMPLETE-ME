# Created by complete-me (alias). Remove: complete-me alias --rm logs
function logs --wraps 'journalctl -u' --description 'alias logs=journalctl -u $1 -f'
    journalctl -u $argv[1] -f $argv[2..]
end
