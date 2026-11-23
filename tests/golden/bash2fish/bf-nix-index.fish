#!/bin/sh
# for bash 4
# this will be called when a command is entered
# but not found in the user’s path + environment
function command_not_found_handle
  # TODO: use "command not found" gettext translations
  # taken from http://www.linuxjournal.com/content/bash-command-not-found
  # - do not run when inside Midnight Commander or within a Pipe
  if [ -n (set -q MC_SID && echo "$MC_SID" || echo '') ] || ! [ -t 1 ]
    echo $argv[1]': command not found' >&2
    return 127
  end
  # nixpkgs should always be available even in NixOS
  set toplevel 'nixpkgs'
  set cmd $argv[1]
  set attrs (@out@/bin/nix-locate --minimal --no-group --type x --type s --top-level --whole-name --at-root '/bin/'"$cmd" | string collect; or echo)
  set len (echo -n "$attrs" | grep -c '^' | string collect; or echo)
  switch "$len"
  case '0'
    echo "$cmd"': command not found' >&2
  case '1'
    # if only 1 package provides this, then we can invoke it
    # without asking the users if they have opted in with one
    # of 2 environment variables
    # they are based on the ones found in
    # command-not-found.sh:
    #   NIX_AUTO_INSTALL : install the missing command into the
    #                      user’s environment
    #   NIX_AUTO_RUN     : run the command transparently inside of
    #                      nix shell
    # these will not return 127 if they worked correctly
    if ! [ -z (set -q NIX_AUTO_INSTALL && echo "$NIX_AUTO_INSTALL" || echo '') ]
      cat >&2 <(echo 'The program \''"$cmd"'\' is currently not installed. It is provided by
the package \''"$toplevel"'.'"$attrs"'\', which I will now install for you.
'| psub)
      nix-env -iA $toplevel.$attrs
      if [ "$status" -eq 0 ]
        # TODO: handle pipes correctly if AUTO_RUN/INSTALL is possible
        $argv
        return $status
      else
        cat >&2 <(echo 'Failed to install '"$toplevel"'.attrs.
'"$cmd"': command not found
'| psub)
      end
    else if ! [ -z (set -q NIX_AUTO_RUN && echo "$NIX_AUTO_RUN" || echo '') ]
      nix-build --no-out-link -A $attrs '<'"$toplevel"'>'
      if [ "$status" -eq 0 ]
        # how nix-shell handles commands is weird
        # $(echo $@) is need to handle this
        nix-shell -p $attrs --run (echo $argv | string collect; or echo)
        return $status
      else
        cat >&2 <(echo 'Failed to install '"$toplevel"'.attrs.
'"$cmd"': command not found
'| psub)
      end
    else
      cat >&2 <(echo 'The program \''"$cmd"'\' is currently not installed. You can install it
by typing:
  nix-env -iA '"$toplevel"'.'"$attrs"'
'| psub)
    end
  case '*'
    cat >&2 <(echo 'The program \''"$cmd"'\' is currently not installed. It is provided by
several packages. You can install it by typing one of the following:
'| psub)
    # ensure we get each element of attrs
    # in a cross platform way
    while read attr
      echo '  nix-env -iA '"$toplevel"'.'"$attr" >&2
    end <(echo "$attrs"| psub)
  end
  # command not found should always exit with 127
  return 127
end
# for zsh...
# we just pass it to the bash handler above
# apparently they work identically
function command_not_found_handler
  command_not_found_handle $argv
  return $status
end
