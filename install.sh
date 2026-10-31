#!/usr/bin/env bash
# COMPLETE-ME installer: everything a first-time user needs, in one go. Safe to re-run.
#
#   ./install.sh                  interactive: checks deps, offers to install them, hooks up your shells,
#                                 runs a self-test and shows a short tour
#   ./install.sh --yes            same, answering yes to everything (non-interactive)
#   ./install.sh --no-deps        don't install packages; just link and hook up
#   ./install.sh --shells fish    only these shells (space or comma separated: fish bash zsh)
#   ./install.sh --dry-run        print what would happen, change nothing
#   ./install.sh --uninstall      remove everything this script added (generated caches are kept)
#
#   curl -fsSL <raw url>/install.sh | bash     clones into ~/.local/share/complete-me, then installs
#
# What it changes, and nothing else:
#   packages (only with your OK)                   bubblewrap, fzf, babelfish, a clipboard reader, python3
#   ~/.local/bin/complete-me                       symlink to bin/complete-me in the checkout
#   ~/.config/fish/conf.d/complete_me.fish         one guarded `source` line
#   ~/.bashrc, ~/.zshrc                            one marked line each, only if the file exists, with your OK
set -euo pipefail

REPO_URL="${COMPLETE_ME_REPO:-https://github.com/arf-io/COMPLETE-ME.git}"
BIN_DIR="$HOME/.local/bin"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
FISH_CONF="$CONF/fish/conf.d/complete_me.fish"
MARK="# complete-me"

YES=0 DEPS=1 DRY=0 SHELLS="fish bash zsh" UNINSTALL=0
while [ $# -gt 0 ]; do
    case "$1" in
        -y|--yes) YES=1 ;;
        --no-deps) DEPS=0 ;;
        --dry-run) DRY=1 ;;
        --shells) SHELLS="${2//,/ }"; shift ;;
        --uninstall) UNINSTALL=1 ;;
        -h|--help) sed -n '2,20p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

# ------------------------------------------------------------------ helpers
if [ -t 1 ]; then B=$'\e[1m' G=$'\e[32m' Y=$'\e[33m' R=$'\e[31m' D=$'\e[2m' X=$'\e[0m'; else B= G= Y= R= D= X=; fi
step() { printf '\n%s==>%s %s%s%s\n' "$G" "$X" "$B" "$*" "$X"; }
ok()   { printf '  %s✓%s %s\n' "$G" "$X" "$*"; }
warn() { printf '  %s!%s %s\n' "$Y" "$X" "$*"; }
fail() { printf '  %s✗%s %s\n' "$R" "$X" "$*" >&2; }
run()  { if [ "$DRY" = 1 ]; then printf '  %s[dry-run]%s %s\n' "$D" "$X" "$*"; else "$@"; fi; }
have() { command -v "$1" >/dev/null 2>&1; }
want_shell() { case " $SHELLS " in *" $1 "*) return 0;; esac; return 1; }
ask() {
    [ "$YES" = 1 ] || [ "$DRY" = 1 ] && return 0
    if { true </dev/tty; } 2>/dev/null; then
        printf '  %s?%s %s [Y/n] ' "$Y" "$X" "$1" >/dev/tty
        local a; read -r a </dev/tty || a=n
        case "$a" in ""|y|Y|yes|YES) return 0;; esac
    fi
    return 1
}

# ------------------------------------------------------------------ where is the checkout?
SELF="${BASH_SOURCE[0]:-}"
if [ -n "$SELF" ] && [ -f "$SELF" ] && [ -f "$(dirname "$SELF")/bin/complete-me" ]; then
    REPO="$(cd "$(dirname "$SELF")" && pwd -P)"
else
    REPO="${COMPLETE_ME_HOME:-$HOME/.local/share/complete-me}"
fi

if [ "$UNINSTALL" = 1 ]; then
    step "Uninstalling COMPLETE-ME"
    run rm -f "$BIN_DIR/complete-me" "$FISH_CONF"
    for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
        if [ -f "$rc" ] && grep -q "$MARK\$" "$rc"; then
            run sed -i.bak "/$MARK\$/d" "$rc"; ok "cleaned $rc (backup: $rc.bak)"
        fi
    done
    ok "removed. Generated completions remain in ${XDG_CACHE_HOME:-$HOME/.cache}/complete-me; packages were left installed."
    exit 0
fi

printf '%sCOMPLETE-ME%s — tab completion for everything\n' "$B" "$X"
[ "$DRY" = 1 ] && warn "dry run: nothing will be changed"

# ------------------------------------------------------------------ platform
step "Checking your system"
OS="$(uname -s)"
PM=""
BREW="$(command -v brew || { [ -x /home/linuxbrew/.linuxbrew/bin/brew ] && echo /home/linuxbrew/.linuxbrew/bin/brew; } || { [ -x /opt/homebrew/bin/brew ] && echo /opt/homebrew/bin/brew; } || true)"
if [ "$OS" = Darwin ]; then
    [ -n "$BREW" ] && PM=brew
else
    for p in pacman apt-get dnf zypper apk xbps-install; do have "$p" && { PM=$p; break; }; done
fi
ok "OS: $OS${PM:+, package manager: $PM}${BREW:+, Homebrew available}"

SUDO=""
[ "$(id -u)" != 0 ] && { have sudo && SUDO=sudo || { have doas && SUDO=doas; }; }
root_ok() {   # can we run the system package manager as root now (or prompt for it)?
    [ "$PM" = brew ] && return 0
    [ "$(id -u)" = 0 ] && return 0
    [ -n "$SUDO" ] || return 1
    $SUDO -n true 2>/dev/null && return 0
    { true </dev/tty; } 2>/dev/null
}
pkg_name() {
    case "$PM:$1" in
        pacman:python3|brew:python3) echo python ;;
        *:bwrap) echo bubblewrap ;;
        *:wl-paste) echo wl-clipboard ;;
        *) echo "$1" ;;
    esac
}
sys_install() {
    case "$PM" in
        pacman)       run $SUDO pacman -S --needed --noconfirm "$@" ;;
        apt-get)      run $SUDO apt-get install -y "$@" ;;
        dnf)          run $SUDO dnf install -y "$@" ;;
        zypper)       run $SUDO zypper --non-interactive install "$@" ;;
        apk)          run $SUDO apk add "$@" ;;
        xbps-install) run $SUDO xbps-install -y "$@" ;;
        brew)         run "$BREW" install "$@" ;;
        *) return 1 ;;
    esac
}

# ------------------------------------------------------------------ dependencies
step "Dependencies"
sys_need=() brew_only=()
dep() {   # dep command "why" required
    if have "$1"; then ok "$1 — $2"; return; fi
    if [ "$3" = 1 ]; then fail "$1 missing — $2 (required)"; else warn "$1 missing — $2"; fi
    if [ "$1" = babelfish ]; then brew_only+=(babelfish); else sys_need+=("$(pkg_name "$1")"); fi
}
dep python3 "runs complete-me" 1
dep git "fetches COMPLETE-ME" 0
[ "$OS" != Darwin ] && dep bwrap "sandbox for --help probing (strongly recommended)" 0
dep fzf "live regex builder (alt-r) and snippet picker (alt-j)" 0
dep babelfish "runs bash you type into fish (translated to fish first)" 0
if [ "$OS" != Darwin ]; then
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then dep wl-paste "clipboard-aware completion" 0
    elif [ -n "${DISPLAY:-}" ] && ! have xsel; then dep xclip "clipboard-aware completion" 0; fi
fi
have fish || warn "fish not found — complete-me shines in fish (bash still gets generated completions)"

if [ "$DEPS" = 1 ] && [ "${#sys_need[@]}" -gt 0 ]; then
    if [ -n "$PM" ] && root_ok && ask "Install ${sys_need[*]} with $PM?"; then
        sys_install "${sys_need[@]}" && ok "installed ${sys_need[*]}" || warn "$PM failed; continuing"
    else
        # No root here: Homebrew can still provide fzf without sudo.
        rest=()
        for p in "${sys_need[@]}"; do
            if [ "$p" = fzf ] && [ -n "$BREW" ] && ask "No sudo available — install fzf with Homebrew instead?"; then
                run "$BREW" install fzf && ok "fzf installed via Homebrew"
            else rest+=("$p"); fi
        done
        [ "${#rest[@]}" -gt 0 ] && warn "install these yourself when you can:  ${SUDO:+$SUDO }${PM:-<package manager>} install ${rest[*]}"
    fi
fi
if [ "$DEPS" = 1 ] && [ "${#brew_only[@]}" -gt 0 ]; then
    if [ -n "$BREW" ] && ask "Install babelfish (bash→fish translator) with Homebrew?"; then
        run "$BREW" install babelfish && ok "babelfish installed"
    elif have go && ask "Install babelfish with 'go install'?"; then
        run go install github.com/bouk/babelfish@latest && ok "babelfish installed to $(go env GOPATH)/bin"
    else
        warn "babelfish not installed: bash typed into fish will get a hint instead of running (brew install babelfish)"
    fi
fi
have python3 || { fail "python3 is required; install it and re-run"; exit 1; }
python3 -c 'import sys; sys.exit(sys.version_info < (3, 9))' || { fail "python3 >= 3.9 is required"; exit 1; }

# ------------------------------------------------------------------ checkout
if [ ! -f "$REPO/bin/complete-me" ]; then
    step "Fetching COMPLETE-ME"
    have git || { fail "git is needed to fetch COMPLETE-ME"; exit 1; }
    run mkdir -p "$(dirname "$REPO")"
    run git clone --depth 1 "$REPO_URL" "$REPO"
    ok "cloned into $REPO"
fi

# ------------------------------------------------------------------ install
step "Installing"
run mkdir -p "$BIN_DIR"
run ln -sfn "$REPO/bin/complete-me" "$BIN_DIR/complete-me"
ok "$BIN_DIR/complete-me → $REPO/bin/complete-me"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *)
    warn "$BIN_DIR is not on your PATH"
    if have fish && want_shell fish; then run fish -c "fish_add_path -U $BIN_DIR" && ok "added it to fish's PATH"; fi
    warn "for bash/zsh add:  export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

hooked=()
if want_shell fish && have fish; then
    run mkdir -p "$(dirname "$FISH_CONF")"
    if [ "$DRY" = 1 ]; then echo "  [dry-run] write $FISH_CONF"; else
        # Guarded: if the checkout moves or a branch without it is checked out, shells still start cleanly.
        printf '%s\ntest -f %q; and source %q\n' "$MARK: generated by install.sh; remove with ./install.sh --uninstall" \
            "$REPO/shell/fish/complete_me.fish" "$REPO/shell/fish/complete_me.fish" > "$FISH_CONF"
    fi
    ok "fish: $FISH_CONF"; hooked+=(fish)
fi
for shell in bash zsh; do
    want_shell "$shell" || continue
    rc="$HOME/.${shell}rc"
    [ -f "$rc" ] || continue
    line="[ -f '$REPO/shell/$shell/complete_me.$shell' ] && source '$REPO/shell/$shell/complete_me.$shell'  $MARK"
    if grep -qF "$MARK" "$rc"; then ok "$shell: already hooked in $rc"
    elif ask "Add one line to $rc for $shell support?"; then
        if [ "$DRY" = 1 ]; then echo "  [dry-run] append to $rc"; else printf '\n%s\n' "$line" >> "$rc"; fi
        ok "$shell: $rc"; hooked+=("$shell")
    fi
done
[ "${#hooked[@]}" -eq 0 ] && warn "no shell hooked up (fish not installed and no ~/.bashrc / ~/.zshrc?)"

# ------------------------------------------------------------------ self-test
step "Self-test"
if [ "$DRY" = 1 ]; then echo "  [dry-run] would run the test suite and a sample parse"; else
    if out=$(cd "$REPO" && python3 -m unittest discover -s tests 2>&1); then
        ok "test suite: $(printf '%s\n' "$out" | grep -Eo 'Ran [0-9]+ tests' | head -1), all passed"
    else
        fail "some tests failed:"; printf '%s\n' "$out" | tail -15 | sed 's/^/      /'
    fi
    if have bwrap; then
        if bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --tmpfs /run --unshare-all --die-with-parent -- true 2>/dev/null; then
            ok "sandbox works (read-only, no network, private /tmp and /run)"
        else
            warn "bwrap can't create a sandbox here (user namespaces off?): scripts will be read, never run"
        fi
    else
        warn "no sandbox: scripts are read, never run; binaries get --help only (deny-list applies)"
    fi
    "$REPO/bin/complete-me" show ls >/dev/null 2>&1 && ok "parsed 'ls' as a smoke test" || warn "'complete-me show ls' failed"
fi

# ------------------------------------------------------------------ tour
step "Done. Open a new terminal, then try:"
cat <<EOF
  ${B}<TAB>${X} on any command          completions appear even for tools that never shipped any
  ${B}git clone <TAB>${X}              a git URL you just copied is offered first
  ${B}grep -E 'err<TAB>${X}             regex pieces, each with what it means
  ${B}alt-r${X}                         live regex builder: type, watch matches, ENTER to insert
  ${B}alt-x${X}                         explain the command line (find expressions, regexes, flags)
  ${B}alt-j${X}                         snippets + bash→fish phrasebook   (or type ${B},for${X} then space)
  ${B}for i in {1..3}; do echo \$i; done${X}   bash typed into fish is translated, checked, and run
  ${B}find . -exec rm <TAB>${X}         {}  then  \\;  or  +
  ${B}complete-me alias gsw git switch${X}   aliases that keep their completions

  Uninstall any time:  $REPO/install.sh --uninstall
EOF
