set CHRUBY_VERSION '0.3.9'
set RUBIES
for dir in "$PREFIX"'/opt/rubies' "$HOME"'/.rubies'
  test -d "$dir" && test -n (ls -A "$dir" | string collect; or echo) && set -a RUBIES "$dir"/*
end
set -e dir
function chruby_reset
  test -z "$RUBY_ROOT" && return
  set PATH ':'"$PATH"':'
  set PATH (string replace --all ':'"$RUBY_ROOT"'/bin:' ':' "$PATH")
  test -n "$GEM_ROOT" && set PATH (string replace --all ':'"$GEM_ROOT"'/bin:' ':' "$PATH")
  if test (id -ru) -ne 0
    test -n "$GEM_HOME" && set PATH (string replace --all ':'"$GEM_HOME"'/bin:' ':' "$PATH")
    set GEM_PATH ':'"$GEM_PATH"':'
    test -n "$GEM_HOME" && set GEM_PATH (string replace --all ':'"$GEM_HOME"':' ':' "$GEM_PATH")
    test -n "$GEM_ROOT" && set GEM_PATH (string replace --all ':'"$GEM_ROOT"':' ':' "$GEM_PATH")
    set GEM_PATH (string replace -r '^(\\.?:)' '' "$GEM_PATH")
    set GEM_PATH (string replace -r '(:\\.?)$' '' "$GEM_PATH")
    set -e GEM_HOME
    test -z "$GEM_PATH" && set -e GEM_PATH
  end
  set PATH (string replace -r '^(\\.?:)' '' "$PATH")
  set PATH (string replace -r '(:\\.?)$' '' "$PATH")
  set -e RUBY_ROOT; set -e RUBY_ENGINE; set -e RUBY_VERSION; set -e RUBYOPT; set -e GEM_ROOT
  true
end
function chruby_use
  if test ! -x $argv[1]'/bin/ruby'
    echo 'chruby: '$argv[1]'/bin/ruby not executable' >&2
    return 1
  end
  test -n "$RUBY_ROOT" && chruby_reset
  set -gx RUBY_ROOT $argv[1]
  set -gx RUBYOPT $argv[2]
  set -gx PATH "$RUBY_ROOT"'/bin:'"$PATH"
  eval (RUBYGEMS_GEMDEPS='' "$RUBY_ROOT"'/bin/ruby' - <(echo 'puts "export RUBY_ENGINE=#{Object.const_defined?(:RUBY_ENGINE) ? RUBY_ENGINE : \'ruby\'};"
puts "export RUBY_VERSION=#{RUBY_VERSION};"
begin; require \'rubygems\'; puts "export GEM_ROOT=#{Gem.default_dir.inspect};"; rescue LoadError; end
'| psub) | string collect; or echo)
  set -gx PATH (test -n "$GEM_ROOT" && echo "$GEM_ROOT"'/bin:' || echo)"$PATH"
  if test (id -ru) -ne 0
    set -gx GEM_HOME "$HOME"'/.gem/'"$RUBY_ENGINE"'/'"$RUBY_VERSION"
    set -gx GEM_PATH "$GEM_HOME"(test -n "$GEM_ROOT" && echo ':'"$GEM_ROOT" || echo)(test -n "$GEM_PATH" && echo ':'"$GEM_PATH" || echo)
    set -gx PATH "$GEM_HOME"'/bin:'"$PATH"
  end
  true
end
function chruby
  switch $argv[1]
  case '-h' '--help'
    echo 'usage: chruby [RUBY|VERSION|system] [RUBYOPT...]'
  case '-V' '--version'
    echo 'chruby: '"$CHRUBY_VERSION"
  case ''
    set -l dir $dir; set -l ruby $ruby
    for dir in $RUBIES
      set dir (string replace -r '(/)$' '' "$dir")
      set ruby (string replace -r '^(.*/)' '' "$dir")
      if test "$dir" = "$RUBY_ROOT"
        echo ' * '"$ruby"' '"$RUBYOPT"
      else
        echo '   '"$ruby"
      end
    end
  case 'system'
    chruby_reset
  case '*'
    set -l dir $dir; set -l ruby $ruby; set -l match $match
    for dir in $RUBIES
      set dir (string replace -r '(/)$' '' "$dir")
      set ruby (string replace -r '^(.*/)' '' "$dir")
      switch "$ruby"
      case $argv[1]
        set match "$dir" && break
      case '*'$argv[1]'*'
        set match "$dir"
      end
    end
    if test -z "$match"
      echo 'chruby: unknown Ruby: '$argv[1] >&2
      return 1
    end
    set -e argv[1]
    chruby_use "$match" "$argv"
  end
end
