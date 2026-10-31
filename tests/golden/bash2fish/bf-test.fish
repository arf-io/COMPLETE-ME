#!/usr/bin/env bash
# Prevent this file from being sourced by child shells.
set -gx __NIX_DARWIN_SET_ENVIRONMENT_DONE '1'
set A '2'
C='3' echo 23
set -gx A $A
set -gx PATH "$HOME"'/.nix-profile/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/bin:/usr/bin:/usr/sbin:/bin:/sbin'
set -gx EDITOR 'nano'
set -gx NIX_PATH 'darwin-config='"$HOME"'/dotfiles/darwin.nix:/nix/var/nix/profiles/per-user/root/channels:'"$HOME"'/.nix-defexpr/channels'
set -gx NIX_SSL_CERT_FILE '/etc/ssl/certs/ca-certificates.crt'
set -gx PAGER 'less -R'
echo 123 | source
cat (echo 123 | psub)
cat <test.bash
function cool
  cat | cat
end
echo (cat test.bash | cool | fish -c 'cool | cool | fish -c \'echo \\\'cool\\\' | cool\'')
test -e /var/file.sh && babelfish < /var/file.sh | source
if [ -z "$SSH_AUTH_SOCK" ]
  set -gx SSH_AUTH_SOCK (/bin/gpgconf --list-dirs agent-ssh-socket | string collect; or echo)
end
if [ -d '/share/gsettings-schemas/name' ]
  set -gx whatevs "$whatevs"(test -n "$whatevs" && echo ':' || echo)'/share/gsettings-schemas/name'
else if false
  true
else
  true
end
echo (set -q cool && echo 'a' || echo)
echo (test -n "$cool" && echo 'a' || echo)
echo (set -q cool && echo "$cool" || echo 'a')
echo (test -n "$cool" && echo "$cool" || echo 'a')
set -e ASPELL_CONF
for i in a b c
  if [ -d "$i"'/lib/aspell' ]
    set -gx ASPELL_CONF 'dict-dir '"$i"'/lib/aspell'
  end
  echo yes
end
for cmd in $argv
  echo "$cmd"
end
time sleep 1
while true
  echo 1
  echo 2
end
while not true
  echo 1
  echo 2
end
call $me
echo (count $argv)
echo (count $cool)
echo (string length "$cool")
set a (ok | string collect; or echo)
set a (ok | string collect; or echo)
babelfish < /etc/bashrc | source
test 123 != 0
